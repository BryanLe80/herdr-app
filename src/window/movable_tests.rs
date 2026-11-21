/// Reports its root's size to `shared("probe.size")`, as a mod in a window
/// would learn it was resized.
const SIZE_PROBE: &str = r#"import { View, div } from "gpui-kit";
import { setShared } from "herdr";
export default class extends View {
  render() {
    return div().id("root").size_full()
      .on_size_change((size, cx) => { setShared("probe.size", [size.width, size.height]); cx.notify(); });
  }
}
"#;

fn open_probe_window<'a>(cx: &'a mut gpui::TestAppContext, spec: serde_json::Value) -> (gpui::Entity<super::AppWindow>, &'a mut gpui::VisualTestContext) {
    let (view, cx) = open_window(cx, "probe", SIZE_PROBE, serde_json::json!({
        "id": "root", "type": "block", "content": { "tab": "active" }
    }));
    cx.simulate_resize(gpui::size(gpui::px(1000.0), gpui::px(600.0)));
    view.update_in(cx, |this, window, cx| this.open_overlay("w".into(), spec, window, cx));
    redraw(cx);
    (view, cx)
}

/// Twice: a window is measured on its first frame and shown on the next.
fn redraw(cx: &mut gpui::VisualTestContext) {
    for _ in 0..2 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    }
}

/// The window as drawn, border included, rounded to whole px.
fn drawn(view: &gpui::Entity<super::AppWindow>, cx: &mut gpui::VisualTestContext) -> [f32; 4] {
    view.update(cx, |this, _| {
        let o = this.overlays.iter().find(|o| o.id == "w").unwrap();
        let b = this.bounds.borrow()[&o.slot];
        let b = if o.bare { b } else { b.dilate(gpui::px(1.0)) };
        [b.origin.x, b.origin.y, b.size.width, b.size.height].map(|v| f32::from(v).round())
    })
}

fn drag(cx: &mut gpui::VisualTestContext, from: (f32, f32), to: (f32, f32)) {
    use gpui::{Modifiers, MouseButton, point, px};
    cx.simulate_mouse_move(point(px(from.0), px(from.1)), None, Modifiers::default());
    cx.simulate_mouse_down(point(px(from.0), px(from.1)), MouseButton::Left, Modifiers::default());
    redraw(cx);
    cx.simulate_mouse_move(point(px(to.0), px(to.1)), MouseButton::Left, Modifiers::default());
    redraw(cx);
    cx.simulate_mouse_up(point(px(to.0), px(to.1)), MouseButton::Left, Modifiers::default());
    redraw(cx);
}

fn nums(v: Option<serde_json::Value>) -> Vec<f64> {
    v.and_then(|v| v.as_array().map(|a| a.iter().filter_map(serde_json::Value::as_f64).collect())).unwrap_or_default()
}

fn stored() -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(crate::mods::app_dir().join("windows.json")).unwrap_or_else(|_| "{}".into())).unwrap()
}

#[test]
fn resizable_windows_follow_their_grips_and_remember_the_size() {
    in_own_process("window_grips_resize");
}

#[gpui::test]
fn window_grips_resize(cx: &mut gpui::TestAppContext) {
    use serde_json::json;
    if !running_alone("window_grips_resize") {
        return;
    }
    let spec = json!({ "kind": "window", "mod": "probe", "key": "probe", "resizable": true, "width": 300, "height": 200, "at": { "x": 100, "y": 100 } });
    let (view, cx) = open_probe_window(cx, spec.clone());
    assert_eq!(drawn(&view, cx), [100.0, 100.0, 300.0, 200.0]);
    let copy = view.update(cx, |this, _| this.copies["overlay:w"].root.as_ref().map(|r| r.entity_id()));

    // Just outside the bottom right corner: only its grip is there.
    drag(cx, (401.0, 301.0), (461.0, 341.0));
    assert_eq!(drawn(&view, cx), [100.0, 100.0, 360.0, 240.0]);
    assert_eq!(nums(crate::mods::shared("probe.size")), [358.0, 238.0], "the mod hears its new size (inside the border)");
    assert_eq!(view.update(cx, |this, _| this.copies["overlay:w"].root.as_ref().map(|r| r.entity_id())), copy, "the same copy of the mod, not a new one");
    assert_eq!(stored()["sizes"]["probe"], json!([360.0, 240.0]));
    assert_eq!(stored()["places"]["probe"], json!([100.0, 100.0]));

    // The left side moves the window; its right side stays.
    drag(cx, (99.0, 200.0), (49.0, 230.0));
    assert_eq!(drawn(&view, cx), [50.0, 100.0, 410.0, 240.0]);

    // Never below the minimum (96 × 48 without minWidth/minHeight).
    drag(cx, (49.0, 200.0), (900.0, 200.0));
    assert_eq!(drawn(&view, cx), [364.0, 100.0, 96.0, 240.0]);

    // A side dragged past the app stops at its edge.
    drag(cx, (461.0, 341.0), (5000.0, 5000.0));
    assert_eq!(drawn(&view, cx), [364.0, 100.0, 635.0, 499.0]);
    assert_eq!(stored()["sizes"]["probe"], json!([635.0, 499.0]));

    // Opening the key again: the dragged size beats width/height, as the
    // dropped spot beats `at`.
    view.update_in(cx, |this, window, cx| this.open_overlay("w".into(), spec, window, cx));
    redraw(cx);
    assert_eq!(drawn(&view, cx), [364.0, 100.0, 635.0, 499.0]);
}

#[test]
fn windows_without_resizable_move_from_their_edges() {
    in_own_process("fixed_window_edges");
}

#[gpui::test]
fn fixed_window_edges(cx: &mut gpui::TestAppContext) {
    use serde_json::json;
    if !running_alone("fixed_window_edges") {
        return;
    }
    let spec = json!({ "kind": "window", "mod": "probe", "key": "fixed", "width": 300, "height": 200, "at": { "x": 100, "y": 100 } });
    let (view, cx) = open_probe_window(cx, spec);
    // Where a resizable window's grip would be, beside it: nothing happens.
    drag(cx, (401.0, 301.0), (461.0, 341.0));
    assert_eq!(drawn(&view, cx), [100.0, 100.0, 300.0, 200.0]);
    // On its corner: the press moves it, the size stays.
    drag(cx, (398.0, 298.0), (448.0, 348.0));
    let [x, y, w, h] = drawn(&view, cx);
    assert_eq!((w, h), (300.0, 200.0));
    assert!(x >= 150.0 && y >= 150.0, "moved: {x}, {y}");
    assert!(stored().get("sizes").is_none(), "{}", stored());
}

#[test]
fn windows_json_from_before_sizes_still_opens_windows_at_their_spot() {
    in_own_process("windows_json_without_sizes");
}

#[gpui::test]
fn windows_json_without_sizes(cx: &mut gpui::TestAppContext) {
    use serde_json::json;
    if !running_alone("windows_json_without_sizes") {
        return;
    }
    let home = crate::mods::app_dir();
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(home.join("windows.json"), json!({ "places": { "old": [120, 80], "wide": [10, 20] }, "sizes": { "wide": [500, 0] } }).to_string()).unwrap();
    let spec = json!({ "kind": "window", "mod": "probe", "key": "old", "resizable": true, "width": 300, "height": 200 });
    let (view, cx) = open_probe_window(cx, spec);
    assert_eq!(drawn(&view, cx), [120.0, 80.0, 300.0, 200.0]);
    // A side never dragged (0) follows the spec: here, the content's height.
    let spec = json!({ "kind": "window", "mod": "probe", "key": "wide", "resizable": true, "width": 300 });
    view.update_in(cx, |this, window, cx| {
        this.close_overlay("w", window, cx);
        this.open_overlay("w".into(), spec, window, cx);
    });
    redraw(cx);
    let (size, fit) = view.update(cx, |this, _| {
        let o = this.overlays.iter().find(|o| o.id == "w").unwrap();
        (o.size, o.fit)
    });
    assert_eq!((size.width, size.height, fit), (500.0, 0.0, true));
    assert_eq!(drawn(&view, cx)[..3], [10.0, 20.0, 500.0]);
}
