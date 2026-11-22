/// A floating mod 200x100, docked on the window's left edge, in a 1000x600 window.
fn open_float_window(cx: &mut gpui::TestAppContext) -> (gpui::Entity<super::AppWindow>, &mut gpui::VisualTestContext) {
    let probe = r#"import { View, div } from "gpui-kit";
export default class extends View { render() { return div().w(200).h(100); } }
"#;
    let (view, cx) = open_window(cx, "float-probe", probe, serde_json::json!({
        "id": "root", "type": "row",
        "children": [{ "id": "main", "type": "block", "content": { "tab": "active" } }],
        "floating": [{ "id": "float", "content": { "mod": "float-probe" }, "edge": "left" }]
    }));
    cx.simulate_resize(gpui::size(gpui::px(1000.0), gpui::px(600.0)));
    float_redraw(cx);
    (view, cx)
}

/// Twice: a float is measured on its first frame and shown on the next.
fn float_redraw(cx: &mut gpui::VisualTestContext) {
    for _ in 0..2 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    }
}

fn float_center(view: &gpui::Entity<super::AppWindow>, cx: &mut gpui::VisualTestContext) -> (f32, f32) {
    view.update(cx, |this, _| {
        let b = this.bounds.borrow()["float:float"];
        (f32::from(b.center().x), f32::from(b.center().y))
    })
}

/// Edge and align as mods see them: while dragged, where it would land.
fn float_placement(view: &gpui::Entity<super::AppWindow>, cx: &mut gpui::VisualTestContext) -> (serde_json::Value, serde_json::Value) {
    view.update(cx, |this, _| {
        let p = &this.float_placements()["float"];
        (p["edge"].clone(), p["align"].clone())
    })
}

/// Presses at `from`, then moves with the button held to each of `to`.
fn float_press_and_move(cx: &mut gpui::VisualTestContext, from: (f32, f32), to: &[(f32, f32)]) {
    use gpui::{Modifiers, MouseButton, point, px};
    cx.simulate_mouse_move(point(px(from.0), px(from.1)), None, Modifiers::default());
    cx.simulate_mouse_down(point(px(from.0), px(from.1)), MouseButton::Left, Modifiers::default());
    float_redraw(cx);
    for p in to {
        cx.simulate_mouse_move(point(px(p.0), px(p.1)), MouseButton::Left, Modifiers::default());
        float_redraw(cx);
    }
}

#[test]
fn a_float_released_outside_the_window_lands_at_its_drop_target() {
    in_own_process("float_released_outside");
}

#[gpui::test]
fn float_released_outside(cx: &mut gpui::TestAppContext) {
    use gpui::{Modifiers, point, px};
    if !running_alone("float_released_outside") {
        return;
    }
    let (view, cx) = open_float_window(cx);
    let start = float_placement(&view, cx);
    let from = float_center(&view, cx);
    float_press_and_move(cx, from, &[(700.0, 300.0), (950.0, 550.0)]);
    let target = float_placement(&view, cx);
    assert_ne!(target, start, "the drag picked a new spot");
    // Released outside the window: no mouse up comes, and the next move, back
    // over the window, has no button and is somewhere else.
    cx.simulate_mouse_move(point(px(100.0), px(100.0)), None, Modifiers::default());
    float_redraw(cx);
    assert!(view.update(cx, |this, _| this.float_drag.is_none()));
    assert_eq!(float_placement(&view, cx), target, "lands where the drag was last carried");
    assert!(view.update(cx, |this, _| this.float_anims.contains_key("float")), "springs into place");
}

#[test]
fn a_float_press_that_never_became_a_drag_changes_nothing() {
    in_own_process("float_press_without_drag");
}

#[gpui::test]
fn float_press_without_drag(cx: &mut gpui::TestAppContext) {
    use gpui::{Modifiers, point, px};
    if !running_alone("float_press_without_drag") {
        return;
    }
    let (view, cx) = open_float_window(cx);
    let start = float_placement(&view, cx);
    let from = float_center(&view, cx);
    float_press_and_move(cx, from, &[(from.0 + 1.0, from.1 + 1.0)]);
    cx.simulate_mouse_move(point(px(900.0), px(500.0)), None, Modifiers::default());
    float_redraw(cx);
    assert!(view.update(cx, |this, _| this.float_drag.is_none()));
    assert_eq!(float_placement(&view, cx), start);
    assert!(view.update(cx, |this, _| this.float_anims.is_empty()));
}
