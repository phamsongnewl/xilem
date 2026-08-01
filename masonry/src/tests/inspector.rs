// Copyright 2026 the Xilem Authors
// SPDX-License-Identifier: Apache-2.0

//! Tests for the inspector-facing accessors (`WidgetId::to_raw`, `QueryCtx` getters)
//! and the inspector event listener (`InspectorEvent`, pointer pass hook).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use masonry_testing::TestHarness;

use crate::core::{InspectorEvent, NewWidget, WidgetId};
use crate::theme::test_property_set;
use crate::widgets::{Button, ButtonPress, Flex, Label};

fn harness_with_button() -> TestHarness<Flex> {
    let button = Button::new(NewWidget::new(Label::new("Hi")));
    let flex = Flex::row().with_fixed(NewWidget::new(button));
    TestHarness::create_with_size(test_property_set(), NewWidget::new(flex), (400, 300))
}

/// Id of the button (only child of the root flex).
fn harness_button_id(harness: &TestHarness<Flex>) -> WidgetId {
    harness
        .get_widget_with_id(harness.root_id())
        .children()
        .first()
        .unwrap()
        .id()
}

#[test]
fn widget_id_to_raw_roundtrips() {
    let harness = harness_with_button();
    let root_id = harness.root_id();
    let button_id = harness
        .get_widget_with_id(root_id)
        .children()
        .first()
        .unwrap()
        .id();

    let root_raw = root_id.to_raw();
    let button_raw = button_id.to_raw();
    assert!(root_raw > 0, "to_raw must preserve the nonzero value");
    assert!(button_raw > 0, "to_raw must preserve the nonzero value");
    assert_ne!(
        root_raw, button_raw,
        "distinct ids map to distinct raw values"
    );
}

#[test]
fn border_box_is_finite_after_create() {
    let harness = harness_with_button();
    // No explicit render() yet — the border box must be finite (not NaN).
    let root = harness.get_widget_with_id(harness.root_id());
    let rect = root.ctx().border_box();
    assert!(
        rect.x0.is_finite() && rect.y0.is_finite() && rect.x1.is_finite() && rect.y1.is_finite(),
        "pre-render border box is finite: {rect:?}"
    );
}

#[test]
fn query_ctx_layout_accessors_after_layout() {
    let mut harness = harness_with_button();
    harness.render();
    let root_id = harness.root_id();
    let root = harness.get_widget_with_id(root_id);
    let rect = root.ctx().border_box();
    assert!(
        rect.width() > 0.0 && rect.height() > 0.0,
        "laid-out root has a non-empty border box: {rect:?}"
    );
    assert!(!root.ctx().is_focus_target());
    assert!(!root.ctx().is_disabled());
    assert!(!root.ctx().is_stashed());
}

#[test]
fn query_ctx_disabled_flag_reflects_disable() {
    let mut harness = harness_with_button();
    // Disable via an edit pass, then re-query.
    harness.edit_root_widget(|mut root| {
        root.ctx.set_disabled(true);
    });
    harness.render();
    let root = harness.get_widget_with_id(harness.root_id());
    assert!(root.ctx().is_disabled());
}

// --- Inspector event listener ---

#[test]
fn listener_receives_pointer_down_with_target_and_path() {
    let mut harness = harness_with_button();
    harness.render();
    let button_id = harness_button_id(&harness);

    // InspectorEvent borrows pass data; the listener converts to owned facts
    // (raw ids) so the summary can outlive the pass.
    let summary: Rc<RefCell<Vec<(Option<u64>, Vec<u64>)>>> = Rc::new(RefCell::new(Vec::new()));
    let summary2 = summary.clone();
    harness
        .render_root()
        .set_inspector_event_listener(Some(Box::new(move |ev| {
            if let InspectorEvent::Pointer { target, path, .. } = ev {
                summary2.borrow_mut().push((
                    target.map(|t| t.to_raw()),
                    path.iter().map(|p| p.to_raw()).collect(),
                ));
            }
        })));

    // Click the button: Move, Down, Up.
    harness.mouse_click_on(button_id, None);

    let got = summary.borrow();
    assert_eq!(got.len(), 3, "Move, Down and Up each observed: {got:?}");
    for (target, path) in got.iter() {
        assert!(target.is_some(), "click has a hit target: {got:?}");
        assert!(path.len() >= 2, "path goes root -> leaf, got {path:?}");
        assert_eq!(path.last(), target.as_ref(), "path leaf is the target");
    }
}

#[test]
fn listener_pointer_event_on_empty_area_has_none_target() {
    let mut harness = harness_with_button();
    harness.render();
    let summary: Rc<RefCell<Vec<Option<u64>>>> = Rc::new(RefCell::new(Vec::new()));
    let s2 = summary.clone();
    harness
        .render_root()
        .set_inspector_event_listener(Some(Box::new(move |ev| {
            if let InspectorEvent::Pointer { target, .. } = ev {
                s2.borrow_mut().push(target.map(|t| t.to_raw()));
            }
        })));
    // Move, press, release outside the window: no widget covers that point.
    harness.mouse_move((500.0, 500.0));
    harness.mouse_button_press(None);
    harness.mouse_button_release(None);
    let got = summary.borrow();
    assert!(
        got.contains(&None),
        "empty-area events fire with target None: {got:?}"
    );
}

#[test]
fn listener_not_set_does_not_fire() {
    let mut harness = harness_with_button();
    harness.render();
    let button_id = harness_button_id(&harness);
    // No listener set: the pass must dispatch exactly as before.
    harness.mouse_click_on(button_id, None);
    assert!(
        harness.pop_action::<ButtonPress>().is_some(),
        "click still reaches the button without a listener"
    );
}

#[test]
fn clearing_listener_stops_events() {
    let mut harness = harness_with_button();
    harness.render();
    let button_id = harness_button_id(&harness);
    let count: Rc<Cell<usize>> = Rc::new(Cell::new(0));
    let c2 = count.clone();
    harness
        .render_root()
        .set_inspector_event_listener(Some(Box::new(move |_| {
            c2.set(c2.get() + 1);
        })));
    harness.render_root().set_inspector_event_listener(None);
    harness.mouse_click_on(button_id, None);
    assert_eq!(count.get(), 0);
}

#[test]
fn listener_receives_key_event_with_focused_widget() {
    let mut harness = harness_with_button();
    harness.render();
    let button_id = harness_button_id(&harness);
    let got: Rc<RefCell<Vec<(bool, Option<u64>)>>> = Rc::new(RefCell::new(Vec::new()));
    let g2 = got.clone();
    harness
        .render_root()
        .set_inspector_event_listener(Some(Box::new(move |ev| {
            if let InspectorEvent::Text { focused, .. } = ev {
                g2.borrow_mut().push((true, focused.map(|f| f.to_raw())));
            }
        })));
    harness.focus_on(Some(button_id));
    harness.keyboard_type_chars("a");
    let got = got.borrow();
    // Focus the button first so the Text event carries a focused id; the harness
    // may deliver one or more Text events for 'a', so assert at least one.
    assert!(
        got.iter().any(|(is_text, _)| *is_text),
        "key events observed: {got:?}"
    );
    assert_eq!(
        got.iter().find(|(is_text, _)| *is_text).map(|(_, f)| *f),
        Some(Some(button_id.to_raw())),
        "Text event carries the focused button id: {got:?}"
    );
}

#[test]
fn listener_receives_action_with_source() {
    let mut harness = harness_with_button();
    harness.render();
    let button_id = harness_button_id(&harness);
    let got: Rc<Cell<Option<u64>>> = Rc::new(Cell::new(None));
    let g2 = got.clone();
    harness
        .render_root()
        .set_inspector_event_listener(Some(Box::new(move |ev| {
            if let InspectorEvent::Action { source, .. } = ev {
                g2.set(Some(source.to_raw()));
            }
        })));
    harness.mouse_click_on(button_id, None);
    assert_eq!(
        got.get(),
        Some(button_id.to_raw()),
        "Action event carries the clicked widget as source"
    );
}

#[test]
fn listener_receives_focus_change() {
    let mut harness = harness_with_button();
    harness.render();
    let button_id = harness_button_id(&harness);
    let got: Rc<Cell<Option<Option<u64>>>> = Rc::new(Cell::new(None));
    let g2 = got.clone();
    harness
        .render_root()
        .set_inspector_event_listener(Some(Box::new(move |ev| {
            if let InspectorEvent::Focus { widget } = ev {
                g2.set(Some(widget.map(|w| w.to_raw())));
            }
        })));
    harness.focus_on(Some(button_id));
    let value = got.get().flatten();
    assert_eq!(value, Some(button_id.to_raw()), "focus change observed");
}
