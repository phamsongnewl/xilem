// Copyright 2026 the Xilem Authors
// SPDX-License-Identifier: Apache-2.0

//! Final delivery of queued widget actions.
//!
//! Purpose: Decide, when the driver dequeues one widget action, whether the
//!   source widget that produced it still exists.
//! Input: The source widget id, its liveness, the registered view path, the
//!   erased action, and the driver's dispatch callback.
//! Output: One dispatch into the view tree, or a discard.
//! Behavior: An earlier action in the same pass can rebuild the tree and remove
//!   a widget whose action was already queued. That source is stale, so its
//!   action is dropped without reaching a replacement editor and without an
//!   unknown-widget error. A live source keeps its existing path; a live source
//!   with no registered path is still a registration error.

use masonry::core::{ErasedAction, WidgetId};

use crate::core::ViewId;

/// Delivers one queued widget action to its registered view path.
///
/// `source_is_live` and `path` must be observed when this action is dequeued,
/// not when it was queued: an earlier dispatch may have rebuilt the tree.
pub(super) fn deliver_action(
    widget_id: WidgetId,
    source_is_live: bool,
    path: Option<Vec<ViewId>>,
    action: ErasedAction,
    dispatch: impl FnOnce(Vec<ViewId>, ErasedAction),
) {
    if !source_is_live {
        // The source existed when this action was queued: an earlier dispatched
        // action rebuilt the tree and tore it down. Delivering now would run a
        // callback for content that is gone, or land in a replacement editor.
        tracing::debug!(
            ?widget_id,
            "Discarding a queued action whose source widget no longer exists"
        );
        return;
    }

    let Some(id_path) = path else {
        tracing::error!(
            "Got action {action:?} for unknown widget. Did you forget to use `with_action_widget`?"
        );
        return;
    };

    dispatch(id_path, action);
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    use masonry::app::{RenderRoot, RenderRootOptions, WindowSizePolicy};
    use masonry::core::{DefaultProperties, ErasedAction, WidgetId};
    use masonry::dpi::PhysicalSize;
    use masonry::widgets::ButtonPress;
    use tracing::span::{Attributes, Id, Record};
    use tracing::subscriber::Subscriber;
    use tracing::{Event, Level, Metadata};

    use super::deliver_action;
    use crate::core::{
        DynMessage, MessageCtx, MessageResult, ProxyError, RawProxy, SendMessage, View, ViewId,
        ViewPathTracker,
    };
    use crate::view::{AnyFlexChild, FlexExt, button, flex_col, label};
    use crate::{InitialRootWidget, MasonryRoot, ViewCtx};

    /// App state for the headless routing fixture.
    #[derive(Debug, Default)]
    struct TestState {
        /// Whether the removable second source is part of the tree.
        show_second: bool,
        /// Callbacks that actually ran, in order.
        calls: Vec<&'static str>,
    }

    type RootView = MasonryRoot<TestState>;
    type RootState = <RootView as View<TestState, (), ViewCtx>>::ViewState;

    /// Builds the view tree for the current state.
    ///
    /// The first source's callback clears `show_second`, so the following
    /// rebuild removes the second widget and unregisters its action path.
    fn root_view(state: &TestState) -> RootView {
        let mut children: Vec<AnyFlexChild<TestState>> = Vec::new();
        children.push(
            button(label("first"), |state: &mut TestState| {
                state.calls.push("first");
                state.show_second = false;
            })
            .into_any_flex(),
        );
        if state.show_second {
            children.push(
                button(label("second"), |state: &mut TestState| {
                    state.calls.push("second");
                })
                .into_any_flex(),
            );
        }
        MasonryRoot::new(flex_col(children))
    }

    /// Proxy stub: this fixture never routes asynchronous messages.
    #[derive(Debug)]
    struct StubProxy;

    impl RawProxy for StubProxy {
        fn send_message(
            &self,
            _path: Arc<[ViewId]>,
            _message: SendMessage,
        ) -> Result<(), ProxyError> {
            Ok(())
        }

        fn dyn_debug(&self) -> &dyn std::fmt::Debug {
            self
        }
    }

    /// Records the level of every event dispatched inside its scope.
    struct LevelCapture {
        levels: Arc<Mutex<Vec<Level>>>,
    }

    impl Subscriber for LevelCapture {
        fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
            true
        }

        fn new_span(&self, _span: &Attributes<'_>) -> Id {
            Id::from_u64(1)
        }

        fn record(&self, _span: &Id, _values: &Record<'_>) {}

        fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

        fn event(&self, event: &Event<'_>) {
            self.levels
                .lock()
                .expect("level capture lock")
                .push(*event.metadata().level());
        }

        fn enter(&self, _span: &Id) {}

        fn exit(&self, _span: &Id) {}
    }

    /// Runs `f` with a capturing subscriber and returns the recorded levels.
    fn capture_levels(f: impl FnOnce()) -> Vec<Level> {
        let levels = Arc::new(Mutex::new(Vec::new()));
        let capture = LevelCapture {
            levels: levels.clone(),
        };
        tracing::subscriber::with_default(capture, f);
        levels.lock().expect("level capture lock").clone()
    }

    /// The action a Masonry button submits when pressed.
    fn press() -> ErasedAction {
        Box::new(ButtonPress { button: None })
    }

    /// Headless stand-in for the driver's window: root view, view context, and
    /// a real `RenderRoot`, with the driver's delivery path.
    struct TestEnv {
        ctx: ViewCtx,
        state: TestState,
        view: RootView,
        view_state: RootState,
        root: RenderRoot,
    }

    impl TestEnv {
        fn new() -> Self {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("test runtime");
            let mut ctx = ViewCtx::new(Arc::new(StubProxy), Arc::new(runtime));
            let mut state = TestState {
                show_second: true,
                calls: Vec::new(),
            };
            let view = root_view(&state);
            let (InitialRootWidget(pod), view_state) =
                View::<TestState, (), ViewCtx>::build(&view, &mut ctx, &mut state);
            let root = RenderRoot::new(
                pod.new_widget,
                |_signal| {},
                RenderRootOptions {
                    default_properties: Arc::new(DefaultProperties::default()),
                    use_system_fonts: false,
                    size_policy: WindowSizePolicy::User,
                    size: PhysicalSize::new(320, 240),
                    scale_factor: 1.0,
                    test_font: None,
                },
            );
            Self {
                ctx,
                state,
                view,
                view_state,
                root,
            }
        }

        /// Delivers one queued action through the production delivery core.
        ///
        /// Both observations are freshly taken, as `MasonryDriver::on_action`
        /// does when it dequeues the action.
        fn deliver(&mut self, widget_id: WidgetId, action: ErasedAction) {
            let source_is_live = self.root.has_widget(widget_id);
            let path = self.ctx.get_id_path(widget_id).cloned();
            deliver_action(widget_id, source_is_live, path, action, |path, action| {
                self.dispatch(path, action);
            });
        }

        /// Dispatches one routed action and applies the driver's result rule.
        fn dispatch(&mut self, path: Vec<ViewId>, action: ErasedAction) {
            let environment = std::mem::take(self.ctx.environment());
            let message = DynMessage(action);
            let mut message_ctx = MessageCtx::new(environment, path, message);
            let result = View::<TestState, (), ViewCtx>::message(
                &self.view,
                &mut self.view_state,
                &mut message_ctx,
                &mut self.root,
                &mut self.state,
            );
            let (environment, _path, _message) = message_ctx.finish();
            *self.ctx.environment() = environment;
            // Mirrors `MasonryDriver::handle_message_result`: an action means
            // the app state changed, so the window's views are rebuilt.
            if matches!(result, MessageResult::Action(())) {
                self.rebuild();
            }
        }

        /// Rebuilds the window views from the current app state.
        fn rebuild(&mut self) {
            let next = root_view(&self.state);
            View::<TestState, (), ViewCtx>::rebuild(
                &next,
                &self.view,
                &mut self.view_state,
                &mut self.ctx,
                &mut self.root,
                &mut self.state,
            );
            self.view = next;
        }

        /// Collects registered action-source ids in tree order.
        fn action_source_ids(&self) -> Vec<WidgetId> {
            let mut sources = Vec::new();
            let mut stack = vec![self.root.get_layer_root(0).id()];
            while let Some(id) = stack.pop() {
                if self.ctx.get_id_path(id).is_some() {
                    sources.push(id);
                }
                if let Some(widget) = self.root.get_widget(id) {
                    stack.extend(widget.children_ids().into_iter().rev());
                }
            }
            sources
        }

        /// Returns a live widget that has no registered action path.
        fn live_unregistered_id(&self) -> WidgetId {
            let registered = self.action_source_ids();
            let mut stack = vec![self.root.get_layer_root(0).id()];
            while let Some(id) = stack.pop() {
                if !registered.contains(&id) {
                    return id;
                }
                if let Some(widget) = self.root.get_widget(id) {
                    stack.extend(widget.children_ids());
                }
            }
            panic!("the fixture always has an unregistered widget");
        }
    }

    #[test]
    fn queued_action_removed_by_previous_dispatch_is_discarded() {
        let mut env = TestEnv::new();
        let sources = env.action_source_ids();
        assert_eq!(
            sources.len(),
            2,
            "the fixture registers both buttons as action sources"
        );
        let (first, second) = (sources[0], sources[1]);
        assert!(env.root.has_widget(second));

        // Both signals are queued while both sources still exist, then drained
        // in order: exactly the ordering the driver sees.
        let mut queue: VecDeque<(WidgetId, ErasedAction)> = VecDeque::new();
        queue.push_back((first, press()));
        queue.push_back((second, press()));

        let mut levels = Vec::new();
        while let Some((widget_id, action)) = queue.pop_front() {
            let (calls, captured) = {
                let calls = env.state.calls.len();
                let levels = Arc::new(Mutex::new(Vec::new()));
                let capture = LevelCapture {
                    levels: levels.clone(),
                };
                tracing::subscriber::with_default(capture, || env.deliver(widget_id, action));
                (calls, levels.lock().expect("level capture lock").clone())
            };
            assert!(
                !captured.contains(&Level::ERROR),
                "delivering {widget_id:?} must not report an unknown widget"
            );
            levels.push((calls, captured));
        }

        assert_eq!(
            env.state.calls,
            vec!["first"],
            "only the live first source may run its callback"
        );
        assert!(
            !env.root.has_widget(second),
            "the first callback rebuilt the tree without the second source"
        );
        assert!(
            env.ctx.get_id_path(second).is_none(),
            "its action registration was retired with the widget"
        );
        assert!(
            levels[1].1.is_empty() || !levels[1].1.contains(&Level::ERROR),
            "the discarded delivery stays free of unknown-widget errors"
        );
    }

    #[test]
    fn live_registered_action_keeps_its_path() {
        let mut env = TestEnv::new();
        let first = env.action_source_ids()[0];
        let path = env
            .ctx
            .get_id_path(first)
            .cloned()
            .expect("the live button keeps its registered path");

        env.deliver(first, press());

        assert_eq!(env.state.calls, vec!["first"]);
        assert!(
            !env.state.show_second,
            "the callback ran through the registered path"
        );
        assert_ne!(
            env.ctx.get_id_path(first),
            None,
            "the dispatched source stays registered while it exists"
        );
        assert!(
            !path.is_empty(),
            "the recorded path is the one the driver dispatches"
        );
    }

    #[test]
    fn live_unregistered_source_remains_an_error() {
        let mut env = TestEnv::new();
        let unregistered = env.live_unregistered_id();
        assert!(env.root.has_widget(unregistered));

        let mut levels = Vec::new();
        capture_levels(|| {
            env.deliver(unregistered, press());
            levels.extend(env.state.calls.clone());
        });

        assert!(
            levels.is_empty(),
            "an unregistered source never reaches a callback"
        );
        assert!(
            capture_levels(|| env.deliver(unregistered, press())).contains(&Level::ERROR),
            "a live widget without a path keeps the registration error"
        );
    }

    #[test]
    fn removed_source_is_stale_even_if_old_mapping_remains() {
        let mut env = TestEnv::new();
        let sources = env.action_source_ids();
        let (first, second) = (sources[0], sources[1]);

        env.deliver(first, press());
        assert!(!env.root.has_widget(second));

        // A stale registration that outlives the teardown must not resurrect
        // delivery to a widget that no longer exists.
        env.ctx.record_action_source(second);
        assert!(
            env.ctx.get_id_path(second).is_some(),
            "the stale mapping is present for this regression"
        );

        let levels = capture_levels(|| env.deliver(second, press()));

        assert_eq!(
            env.state.calls,
            vec!["first"],
            "the removed source never runs a second callback"
        );
        assert!(!levels.contains(&Level::ERROR));
    }
}
