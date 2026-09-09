// Copyright 2025 the Xilem Authors
// SPDX-License-Identifier: Apache-2.0

use std::borrow::Cow;
use std::marker::PhantomData;

use masonry::accesskit::{Node, Role};
use tracing::{Span, trace_span};

use masonry::core::{
    AccessCtx, ChildrenIds, LayoutCtx, MeasureCtx, NewWidget, NoAction, PaintCtx, PropertiesRef,
    RegisterCtx, Widget, WidgetId, WidgetMut, WidgetPod,
};
use masonry::imaging::Painter;
use masonry::kurbo::{Axis, Point, Size};
use masonry::layout::{LenReq, Length};

use crate::core::{MessageCtx, MessageResult, Mut, View, ViewMarker};
use crate::{Pod, ViewCtx, WidgetView};

/// A pass-through widget that wraps a single child and only adds a debug name to it.
///
/// The debug name is reported through [`Widget::get_debug_text`], so it shows up in
/// the widget tree dump and in the inspector's debug output, without changing any
/// other behavior: layout, events and accessibility are identical to the bare child.
///
/// An empty name reports `None` from [`Widget::get_debug_text`], which behaves
/// exactly like not having a debug name at all.
pub struct DebugNameWidget {
    inner: WidgetPod<dyn Widget>,
    name: Cow<'static, str>,
}

impl DebugNameWidget {
    /// Creates a new `DebugNameWidget` wrapping the given `child`, reporting `name`
    /// in debug output.
    pub fn new(child: NewWidget<impl Widget + ?Sized>, name: Cow<'static, str>) -> Self {
        Self {
            inner: child.erased().to_pod(),
            name,
        }
    }

    /// Returns a mutable reference to the wrapped child widget.
    pub fn child_mut<'t>(this: &'t mut WidgetMut<'_, Self>) -> WidgetMut<'t, dyn Widget> {
        this.ctx.get_mut(&mut this.widget.inner)
    }

    /// Sets the debug name reported by this widget.
    pub fn set_name(this: &mut WidgetMut<'_, Self>, name: Cow<'static, str>) {
        this.widget.name = name;
    }
}

// --- MARK: IMPL WIDGET
impl Widget for DebugNameWidget {
    type Action = NoAction;

    fn register_children(&mut self, ctx: &mut RegisterCtx<'_>) {
        ctx.register_child(&mut self.inner);
    }

    fn measure(
        &mut self,
        ctx: &mut MeasureCtx<'_>,
        _props: &PropertiesRef<'_>,
        axis: Axis,
        _len_req: LenReq,
        cross_length: Option<Length>,
    ) -> Length {
        ctx.redirect_measurement(&mut self.inner, axis, cross_length)
    }

    fn layout(&mut self, ctx: &mut LayoutCtx<'_>, _props: &PropertiesRef<'_>, size: Size) {
        ctx.run_layout(&mut self.inner, size);
        ctx.place_child(&mut self.inner, Point::ORIGIN);
        ctx.derive_baselines(&self.inner);
    }

    fn paint(
        &mut self,
        _ctx: &mut PaintCtx<'_>,
        _props: &PropertiesRef<'_>,
        _painter: &mut Painter<'_>,
    ) {
    }

    fn accessibility_role(&self) -> Role {
        Role::GenericContainer
    }

    fn accessibility(
        &mut self,
        _ctx: &mut AccessCtx<'_>,
        _props: &PropertiesRef<'_>,
        _node: &mut Node,
    ) {
    }

    fn children_ids(&self) -> ChildrenIds {
        ChildrenIds::from_slice(&[self.inner.id()])
    }

    fn get_debug_text(&self) -> Option<String> {
        (!self.name.is_empty()).then(|| self.name.to_string())
    }

    fn make_trace_span(&self, id: WidgetId) -> Span {
        trace_span!("DebugNameWidget", id = id.trace())
    }
}

/// A view that adds a debug name to the widget built by the child view.
///
/// The child's widget is wrapped in a [`DebugNameWidget`], which reports the name
/// through [`Widget::get_debug_text`] so it shows up in the widget tree dump and in
/// the inspector's debug output. All other behavior is identical to the bare child.
///
/// It can be constructed by using [`WidgetView::debug_name`](DebugNameExt::debug_name).
#[must_use = "View values do nothing unless provided to Xilem."]
pub struct DebugName<V, State, Action> {
    pub(crate) name: Cow<'static, str>,
    pub(crate) child: V,
    pub(crate) phantom: PhantomData<fn() -> (State, Action)>,
}

impl<V, State: 'static, Action> ViewMarker for DebugName<V, State, Action> {}
impl<V, State, Action> View<State, Action, ViewCtx> for DebugName<V, State, Action>
where
    State: 'static,
    Action: 'static,
    V: WidgetView<State, Action>,
{
    type Element = Pod<DebugNameWidget>;
    type ViewState = V::ViewState;

    fn build(&self, ctx: &mut ViewCtx, app_state: &mut State) -> (Self::Element, Self::ViewState) {
        let (child_pod, child_state) = self.child.build(ctx, app_state);
        let widget = DebugNameWidget::new(child_pod.new_widget, self.name.clone());
        (ctx.create_pod(widget), child_state)
    }

    fn rebuild(
        &self,
        prev: &Self,
        view_state: &mut Self::ViewState,
        ctx: &mut ViewCtx,
        mut element: Mut<'_, Self::Element>,
        app_state: &mut State,
    ) {
        self.child.rebuild(
            &prev.child,
            view_state,
            ctx,
            DebugNameWidget::child_mut(&mut element).downcast(),
            app_state,
        );
        if self.name != prev.name {
            DebugNameWidget::set_name(&mut element, self.name.clone());
        }
    }

    fn teardown(
        &self,
        view_state: &mut Self::ViewState,
        ctx: &mut ViewCtx,
        mut element: Mut<'_, Self::Element>,
    ) {
        self.child.teardown(
            view_state,
            ctx,
            DebugNameWidget::child_mut(&mut element).downcast(),
        );
    }

    fn message(
        &self,
        view_state: &mut Self::ViewState,
        message: &mut MessageCtx,
        mut element: Mut<'_, Self::Element>,
        app_state: &mut State,
    ) -> MessageResult<Action> {
        self.child.message(
            view_state,
            message,
            DebugNameWidget::child_mut(&mut element).downcast(),
            app_state,
        )
    }
}

/// Extension trait adding [`debug_name`](DebugNameExt::debug_name) to [`WidgetView`]s.
pub trait DebugNameExt<State: 'static, Action>: WidgetView<State, Action> + Sized {
    /// Adds a debug name to this view's widget, reported through [`Widget::get_debug_text`].
    ///
    /// This shows up in the widget tree dump and in the inspector's debug output,
    /// making it easier to identify widgets. An empty name is a no-op.
    fn debug_name(self, name: impl Into<Cow<'static, str>>) -> DebugName<Self, State, Action> {
        DebugName {
            name: name.into(),
            child: self,
            phantom: PhantomData,
        }
    }
}

impl<State: 'static, Action, V> DebugNameExt<State, Action> for V where V: WidgetView<State, Action> {}

// --- MARK: TESTS
#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use masonry::core::{DefaultProperties, Widget, WidgetTag};
    use masonry::widgets::Label;
    use masonry_testing::TestHarness;

    use super::{DebugName, DebugNameExt, DebugNameWidget};
    use crate::ViewCtx;
    use crate::core::{ProxyError, RawProxy, SendMessage, View, ViewId};
    use crate::view::label;

    const DEBUG_NAME_TAG: WidgetTag<DebugNameWidget> = WidgetTag::named("debug-name-test");

    struct StubProxy;

    impl std::fmt::Debug for StubProxy {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("StubProxy")
        }
    }

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

    fn new_view_ctx() -> ViewCtx {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        ViewCtx::new(Arc::new(StubProxy), Arc::new(runtime))
    }

    #[test]
    fn debug_name_widget_reports_name() {
        let inner = Label::new("hi").prepare();
        let wrapper = DebugNameWidget::new(inner, "LoginLabel".into());
        let mut harness = TestHarness::create_with_size(
            DefaultProperties::default(),
            wrapper.prepare().with_tag(DEBUG_NAME_TAG),
            (100, 30),
        );
        harness.render();
        let widget = harness.get_widget::<DebugNameWidget>(DEBUG_NAME_TAG);
        assert_eq!(widget.get_debug_text().as_deref(), Some("LoginLabel"));
    }

    #[test]
    fn debug_name_widget_empty_name_is_none() {
        let inner = Label::new("hi").prepare();
        let wrapper = DebugNameWidget::new(inner, "".into());
        let mut harness = TestHarness::create_with_size(
            DefaultProperties::default(),
            wrapper.prepare().with_tag(DEBUG_NAME_TAG),
            (100, 30),
        );
        harness.render();
        let widget = harness.get_widget::<DebugNameWidget>(DEBUG_NAME_TAG);
        assert_eq!(widget.get_debug_text(), None);
    }

    #[test]
    fn debug_name_view_builds_wrapper_widget() {
        let view: DebugName<_, (), ()> = label("hi").debug_name("LoginLabel");
        let mut ctx = new_view_ctx();
        let (pod, _view_state) = View::build(&view, &mut ctx, &mut ());
        let mut harness = TestHarness::create_with_size(
            DefaultProperties::default(),
            pod.new_widget.with_tag(DEBUG_NAME_TAG),
            (100, 30),
        );
        harness.render();
        let widget = harness.get_widget::<DebugNameWidget>(DEBUG_NAME_TAG);
        assert_eq!(widget.get_debug_text().as_deref(), Some("LoginLabel"));
    }

    #[test]
    fn debug_name_view_rebuild_updates_name() {
        let view: DebugName<_, (), ()> = label("hi").debug_name("LoginLabel");
        let mut ctx = new_view_ctx();
        let (pod, mut view_state) = View::build(&view, &mut ctx, &mut ());
        let mut harness = TestHarness::create_with_size(
            DefaultProperties::default(),
            pod.new_widget.with_tag(DEBUG_NAME_TAG),
            (100, 30),
        );
        harness.render();

        let new_view: DebugName<_, (), ()> = label("hi").debug_name("RenamedLabel");
        harness.edit_root_widget(|element| {
            View::rebuild(
                &new_view,
                &view,
                &mut view_state,
                &mut ctx,
                element,
                &mut (),
            );
        });

        harness.render();
        let widget = harness.get_widget::<DebugNameWidget>(DEBUG_NAME_TAG);
        assert_eq!(widget.get_debug_text().as_deref(), Some("RenamedLabel"));
    }
}
