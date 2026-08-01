// Copyright 2026 the Xilem Authors
// SPDX-License-Identifier: Apache-2.0

//! Tests for the inspector-facing accessors (`WidgetId::to_raw`, `QueryCtx` getters).

use masonry_testing::TestHarness;

use crate::core::NewWidget;
use crate::theme::test_property_set;
use crate::widgets::{Button, Flex, Label};

fn harness_with_button() -> TestHarness<Flex> {
    let button = Button::new(NewWidget::new(Label::new("Hi")));
    let flex = Flex::row().with_fixed(NewWidget::new(button));
    TestHarness::create_with_size(test_property_set(), NewWidget::new(flex), (400, 300))
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
