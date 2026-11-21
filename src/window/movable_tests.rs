/// Reports its root's size to `shared("probe.size")`, as a mod in a window
/// would learn it was resized. Its content is 120 px tall, for a window that
/// fits its content.
const SIZE_PROBE: &str = r#"import { View, div } from "gpui-kit";
import { setShared } from "herdr";
export default class extends View {
  render() {
    return div().id("root").size_full().child(div().h(120))
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

/// The window's spec-facing state: its size as designed, whether it fits its
/// content, and whether it was pinned to a spot.
fn state(view: &gpui::Entity<super::AppWindow>, cx: &mut gpui::VisualTestContext) -> (f32, f32, bool, bool) {
    view.update(cx, |this, _| {
        let o = this.overlays.iter().find(|o| o.id == "w").unwrap();
        (o.size.width, o.size.height, o.fit, o.place.is_some())
    })
}

#[test]
fn a_click_on_a_grip_changes_nothing_and_a_drag_saves_only_its_sides() {
    in_own_process("window_grip_click_and_sides");
}

#[gpui::test]
fn window_grip_click_and_sides(cx: &mut gpui::TestAppContext) {
    use gpui::{Modifiers, MouseButton, point, px};
    use serde_json::json;
    if !running_alone("window_grip_click_and_sides") {
        return;
    }
    // Half the app wide, as tall as its content, centered.
    let spec = json!({ "kind": "window", "mod": "probe", "key": "half", "resizable": true, "width": 0.5 });
    let (view, cx) = open_probe_window(cx, spec);
    let [x, y, w, h] = drawn(&view, cx);
    assert_eq!([w, h], [500.0, 122.0]);
    assert_eq!(state(&view, cx), (0.5, 0.0, true, false));

    // A click with a little jitter on the right grip: still a share, still fitted, not pinned, nothing saved.
    drag(cx, (x + w + 1.0, y + 60.0), (x + w + 4.0, y + 61.0));
    assert_eq!(drawn(&view, cx), [x, y, w, h]);
    assert_eq!(state(&view, cx), (0.5, 0.0, true, false));
    assert!(stored().get("sizes").is_none() && stored().get("places").is_none(), "{}", stored());

    // Dragged sideways: the width becomes px; the height, never dragged, stays the content's.
    drag(cx, (x + w + 1.0, y + 60.0), (x + w + 41.0, y + 60.0));
    assert_eq!(drawn(&view, cx), [x, y, 540.0, h]);
    assert_eq!(state(&view, cx), (540.0, 0.0, true, true));
    assert_eq!(stored()["sizes"]["half"], json!([540.0, 0.0]));

    // Its bottom dragged: it stops fitting its content and follows the drag; the width saved before stays.
    drag(cx, (x + 270.0, y + h + 1.0), (x + 270.0, y + h + 51.0));
    assert_eq!(drawn(&view, cx), [x, y, 540.0, h + 50.0]);
    assert_eq!(state(&view, cx), (540.0, h + 50.0, false, true));
    assert_eq!(stored()["sizes"]["half"], json!([540.0, h + 50.0]));

    // Released outside the app (no mouse up, the next move has no button): saved all the same.
    let (from, to) = (point(px(x + 541.0), px(y + 60.0)), point(px(x + 561.0), px(y + 60.0)));
    cx.simulate_mouse_move(from, None, Modifiers::default());
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::default());
    redraw(cx);
    cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::default());
    redraw(cx);
    cx.simulate_mouse_move(to, None, Modifiers::default());
    redraw(cx);
    assert!(view.update(cx, |this, _| this.win_drag.is_none()));
    assert_eq!(drawn(&view, cx), [x, y, 560.0, h + 50.0]);
    assert_eq!(stored()["sizes"]["half"], json!([560.0, h + 50.0]));
}

#[test]
fn a_zoomed_window_resizes_and_saves_its_size_as_designed() {
    in_own_process("window_grips_zoomed");
}

#[gpui::test]
fn window_grips_zoomed(cx: &mut gpui::TestAppContext) {
    use serde_json::json;
    if !running_alone("window_grips_zoomed") {
        return;
    }
    let (view, cx) = open_window(cx, "probe", SIZE_PROBE, json!({ "id": "root", "type": "block", "content": { "tab": "active" } }));
    cx.simulate_resize(gpui::size(gpui::px(1000.0), gpui::px(600.0)));
    let spec = json!({ "kind": "window", "mod": "probe", "key": "zoomed", "resizable": true, "width": 300, "height": 200, "at": { "x": 100, "y": 100 } });
    view.update_in(cx, |this, window, cx| {
        this.write_person(|p| p.zoom = 1.5, window, cx);
        this.open_overlay("w".into(), spec.clone(), window, cx);
    });
    redraw(cx);
    assert_eq!(drawn(&view, cx), [100.0, 100.0, 450.0, 300.0]);

    drag(cx, (551.0, 401.0), (611.0, 461.0));
    assert_eq!(drawn(&view, cx), [100.0, 100.0, 510.0, 360.0]);
    assert_eq!(stored()["sizes"]["zoomed"], json!([340.0, 240.0]), "as designed, not as drawn");

    // The minimum (96 × 48) is zoomed too.
    drag(cx, (611.0, 461.0), (0.0, 0.0));
    assert_eq!(drawn(&view, cx), [100.0, 100.0, 144.0, 72.0]);
    assert_eq!(stored()["sizes"]["zoomed"], json!([96.0, 48.0]));

    // A reopen caps a bigger saved size at the app's size in designed px:
    // viewport / zoom, which draws back up to the viewport.
    std::fs::write(crate::mods::app_dir().join("windows.json"), json!({
        "places": { "zoomed": [0, 0] }, "sizes": { "zoomed": [3000, 2000] }
    }).to_string()).unwrap();
    view.update_in(cx, |this, window, cx| {
        this.close_overlay("w", window, cx);
        this.open_overlay("w".into(), spec, window, cx);
    });
    redraw(cx);
    let used = view.update(cx, |this, _| this.overlays.iter().find(|o| o.id == "w").map(|o| (o.size.width, o.size.height)).unwrap());
    assert_eq!(used, (1000.0 / 1.5, 600.0 / 1.5));
    assert_eq!(drawn(&view, cx), [0.0, 0.0, 1000.0, 600.0]);
}

#[test]
fn a_window_resized_from_its_edge_holds_its_terminals_rows_still() {
    in_own_process("window_resize_holds_rows");
}

#[gpui::test]
fn window_resize_holds_rows(cx: &mut gpui::TestAppContext) {
    use gpui::{Modifiers, MouseButton, point, px};
    use serde_json::json;
    if !running_alone("window_resize_holds_rows") {
        return;
    }
    let (view, cx) = open_window(cx, "probe", SIZE_PROBE, json!({ "id": "pane", "type": "block", "content": { "terminal": "t0" } }));
    cx.simulate_resize(gpui::size(px(1000.0), px(600.0)));
    let spec = json!({ "kind": "window", "resizable": true, "width": 300, "height": 200, "at": { "x": 100, "y": 100 },
        "layout": { "id": "term", "type": "block", "content": { "terminal": "t1" } } });
    view.update_in(cx, |this, window, cx| this.open_overlay("w".into(), spec.clone(), window, cx));
    redraw(cx);
    let held = |view: &gpui::Entity<super::AppWindow>, cx: &mut gpui::VisualTestContext| {
        view.update(cx, |this, cx| ["ovw/term", "pane"].map(|id| this.terms[id].view.read(cx).holds_rows_at_top()))
    };
    assert_eq!(held(&view, cx), [false, false]);

    let (from, to) = (point(px(401.0), px(301.0)), point(px(401.0), px(371.0)));
    cx.simulate_mouse_move(from, None, Modifiers::default());
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::default());
    redraw(cx);
    assert_eq!(held(&view, cx), [false, false], "a press alone moves no text");
    cx.simulate_mouse_move(point(px(401.0), px(303.0)), MouseButton::Left, Modifiers::default());
    redraw(cx);
    assert_eq!(held(&view, cx), [false, false], "nor does jitter short of a drag");
    assert_eq!(drawn(&view, cx), [100.0, 100.0, 300.0, 200.0]);
    cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::default());
    redraw(cx);
    assert_eq!(drawn(&view, cx), [100.0, 100.0, 300.0, 270.0], "the grip resizes the window");
    assert_eq!(held(&view, cx), [true, false], "only the resized window's terminal holds its rows");
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::default());
    redraw(cx);
    assert_eq!(held(&view, cx), [false, false], "released: rows centre again");

    // A window closed mid-resize leaves no terminal holding.
    let t1 = view.update(cx, |this, _| this.terms["ovw/term"].view.clone());
    cx.simulate_mouse_down(to, MouseButton::Left, Modifiers::default());
    redraw(cx);
    cx.simulate_mouse_move(point(px(401.0), px(401.0)), MouseButton::Left, Modifiers::default());
    redraw(cx);
    assert!(view.update(cx, |_, cx| t1.read(cx).holds_rows_at_top()));
    view.update_in(cx, |this, window, cx| this.close_overlay("w", window, cx));
    assert!(view.update(cx, |this, cx| this.win_drag.is_none() && !t1.read(cx).holds_rows_at_top()));
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
    // 151 while a move grabs inside the border (1 px shift), 150 once it grabs the outer corner.
    assert!((150.0..=151.0).contains(&x) && (150.0..=151.0).contains(&y), "moved: {x}, {y}");
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
    std::fs::write(home.join("windows.json"), json!({
        "places": { "old": [120, 80], "wide": [10, 20], "fixed": [10, 20], "big": [0, 0], "tiny": [0, 0] },
        "sizes": { "wide": [500, 0], "fixed": [500, 400], "big": [3000, 2000], "tiny": [40, 40] }
    }).to_string()).unwrap();
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

    let reopen = |view: &gpui::Entity<super::AppWindow>, cx: &mut gpui::VisualTestContext, spec: serde_json::Value| {
        view.update_in(cx, |this, window, cx| {
            this.close_overlay("w", window, cx);
            this.open_overlay("w".into(), spec, window, cx);
        });
        redraw(cx);
        drawn(view, cx)
    };
    // No longer resizable: the size saved under its key doesn't count.
    let spec = json!({ "kind": "window", "mod": "probe", "key": "fixed", "width": 300, "height": 200 });
    assert_eq!(reopen(&view, cx, spec), [10.0, 20.0, 300.0, 200.0]);
    // Saved on a bigger display: no bigger than the app.
    let spec = json!({ "kind": "window", "mod": "probe", "key": "big", "resizable": true, "width": 300, "height": 200 });
    assert_eq!(reopen(&view, cx, spec), [0.0, 0.0, 1000.0, 600.0]);
    // Saved under a smaller minimum: no smaller than this one.
    let spec = json!({ "kind": "window", "mod": "probe", "key": "tiny", "resizable": true, "minWidth": 200, "width": 300, "height": 200 });
    assert_eq!(reopen(&view, cx, spec), [0.0, 0.0, 200.0, 48.0]);
}

/// Draws a 40 × 20 element named "btn" at `shared("dock.at")` until
/// `shared("dock.hidden")`, as a desk draws the button a window opens from.
const DOCK: &str = r#"import { View, div } from "gpui-kit";
import { shared } from "herdr";
export default class extends View {
  render() {
    const at = shared("dock.at") ?? [20, 30];
    const root = div().id("dock").size_full().relative();
    if (shared("dock.hidden")) return root;
    return root.child(div().id("btn").absolute().left(at[0]).top(at[1]).w(40).h(20).overlay_anchor("btn"));
  }
}
"#;

fn anchored(anchor: &str) -> serde_json::Value {
    serde_json::json!({ "kind": "window", "mod": "probe", "key": "probe", "anchor": anchor, "width": 300, "height": 200, "at": { "x": 100, "y": 100 } })
}

/// A window whose layout is the dock, and no windows yet.
fn open_docked(cx: &mut gpui::TestAppContext) -> (gpui::Entity<super::AppWindow>, &mut gpui::VisualTestContext) {
    let dock = crate::mods::app_dir().join("mods/dock");
    std::fs::create_dir_all(&dock).unwrap();
    std::fs::write(dock.join("mod.json"), r#"{ "place": "none" }"#).unwrap();
    std::fs::write(dock.join("main.js"), DOCK).unwrap();
    let (view, cx) = open_window(cx, "probe", SIZE_PROBE, serde_json::json!({ "id": "root", "type": "block", "content": { "mod": "dock" } }));
    cx.simulate_resize(gpui::size(gpui::px(1000.0), gpui::px(600.0)));
    redraw(cx);
    (view, cx)
}

fn rounded(b: gpui::Bounds<gpui::Pixels>) -> [f32; 4] {
    [b.origin.x, b.origin.y, b.size.width, b.size.height].map(|v| f32::from(v).round())
}

/// Where "btn" is drawn now, as the engine finds it.
fn anchor_rect(cx: &mut gpui::VisualTestContext) -> Option<[f32; 4]> {
    let window = cx.update(|window, _| window.window_handle().window_id());
    gpui_shell::overlay_anchor(window, "btn").map(rounded)
}

/// The window's frame while it moves; None once it stands.
fn moving(cx: &mut gpui::VisualTestContext) -> Option<[f32; 4]> {
    cx.debug_bounds("moving-window").map(rounded)
}

fn wait(cx: &mut gpui::VisualTestContext, ms: u64) {
    cx.executor().advance_clock(std::time::Duration::from_millis(ms));
    redraw(cx);
}

fn open_w(view: &gpui::Entity<super::AppWindow>, cx: &mut gpui::VisualTestContext, spec: serde_json::Value) {
    view.update_in(cx, |this, window, cx| this.open_overlay("w".into(), spec, window, cx));
    redraw(cx);
}

fn close_w(view: &gpui::Entity<super::AppWindow>, cx: &mut gpui::VisualTestContext) {
    view.update_in(cx, |this, window, cx| this.close_overlay("w", window, cx));
    redraw(cx);
}

/// The overlays `herdr-app status` lists.
fn listed(view: &gpui::Entity<super::AppWindow>, cx: &mut gpui::VisualTestContext) -> serde_json::Value {
    view.update_in(cx, |this, window, cx| this.control(serde_json::json!({ "status": true }), window, cx))["overlays"].clone()
}

fn count_w(view: &gpui::Entity<super::AppWindow>, cx: &mut gpui::VisualTestContext) -> usize {
    view.update(cx, |this, _| this.overlays.iter().filter(|o| o.id == "w").count())
}

fn between(v: [f32; 4], a: [f32; 4], b: [f32; 4]) -> bool {
    (0..4).all(|i| v[i] > a[i].min(b[i]) && v[i] < a[i].max(b[i]))
}

fn near(v: [f32; 4], w: [f32; 4]) -> bool {
    (0..4).all(|i| (v[i] - w[i]).abs() <= 1.0)
}

#[test]
fn anchored_windows_grow_out_of_the_anchor_and_return_to_where_it_is_at_close() {
    in_own_process("window_anchor_round_trip");
}

#[gpui::test]
fn window_anchor_round_trip(cx: &mut gpui::TestAppContext) {
    if !running_alone("window_anchor_round_trip") {
        return;
    }
    let (view, cx) = open_docked(cx);
    let btn = anchor_rect(cx).expect("the dock's anchor is drawn");
    assert_eq!(btn[2..], [40.0, 20.0]);
    let target = [100.0, 100.0, 300.0, 200.0];

    open_w(&view, cx, anchored("btn"));
    assert_eq!(moving(cx), Some(btn), "it starts as the anchor");
    wait(cx, 100);
    let mid = moving(cx).unwrap();
    assert!(between(mid, btn, target), "{mid:?} on its way from {btn:?} to {target:?}");
    wait(cx, 100);
    assert_eq!(moving(cx), None, "landed");
    assert_eq!(drawn(&view, cx), target);
    assert!(view.update(cx, |this, _| this.overlays[0].motion.is_none()));

    // The anchor moves (a list re-sorts); an idle dock keeps it findable.
    crate::mods::set_shared("dock.at".into(), serde_json::json!([600, 400]));
    cx.run_until_parked();
    redraw(cx);
    redraw(cx);
    let moved = anchor_rect(cx).expect("still drawn");
    assert_ne!(moved, btn);

    close_w(&view, cx);
    assert_eq!(listed(&view, cx), serde_json::json!([]), "a closing window isn't open");
    assert_eq!(count_w(&view, cx), 1, "it's still drawn while it shrinks");
    assert_eq!(moving(cx), Some(target), "it shrinks from where it stands");
    wait(cx, 199);
    let end = moving(cx).unwrap();
    assert!(near(end, moved), "{end:?} ends at the anchor where it is now, {moved:?}, not {btn:?}");
    wait(cx, 1);
    assert_eq!(count_w(&view, cx), 0, "gone when it's back in the anchor");
    assert!(!view.update(cx, |this, _| this.copies.contains_key("overlay:w")), "its mod is closed");
}

#[test]
fn anchored_windows_fade_where_they_stand_when_the_anchor_is_gone() {
    in_own_process("window_anchor_gone");
}

#[gpui::test]
fn window_anchor_gone(cx: &mut gpui::TestAppContext) {
    if !running_alone("window_anchor_gone") {
        return;
    }
    let (view, cx) = open_docked(cx);
    open_w(&view, cx, anchored("btn"));
    wait(cx, 200);
    let target = drawn(&view, cx);

    // Unpainted, the anchor leaves the map that frame.
    crate::mods::set_shared("dock.hidden".into(), serde_json::json!(true));
    cx.run_until_parked();
    redraw(cx);
    assert_eq!(anchor_rect(cx), None);

    close_w(&view, cx);
    assert_eq!(moving(cx), Some(target));
    wait(cx, 100);
    assert_eq!(moving(cx), Some(target), "it fades in place, no shrinking");
    assert_eq!(count_w(&view, cx), 1);
    wait(cx, 50);
    assert_eq!(count_w(&view, cx), 0);
    assert_eq!(listed(&view, cx), serde_json::json!([]));
}

#[test]
fn windows_without_an_anchor_open_and_close_at_once() {
    in_own_process("window_without_anchor");
}

#[gpui::test]
fn window_without_anchor(cx: &mut gpui::TestAppContext) {
    use serde_json::json;
    if !running_alone("window_without_anchor") {
        return;
    }
    let (view, cx) = open_docked(cx);
    let plain = json!({ "kind": "window", "mod": "probe", "width": 300, "height": 200, "at": { "x": 100, "y": 100 } });
    open_w(&view, cx, plain);
    assert_eq!(moving(cx), None);
    assert_eq!(drawn(&view, cx), [100.0, 100.0, 300.0, 200.0]);
    assert!(view.update(cx, |this, _| this.overlays[0].motion.is_none()));
    view.update_in(cx, |this, window, cx| this.close_overlay("w", window, cx));
    assert_eq!(count_w(&view, cx), 0, "closed in the same call");

    // An anchor that isn't drawn when it opens: it just appears.
    open_w(&view, cx, anchored("nowhere"));
    assert_eq!(moving(cx), None);
    assert_eq!(drawn(&view, cx), [100.0, 100.0, 300.0, 200.0]);
    assert_eq!(listed(&view, cx), json!(["probe"]));
}

#[test]
fn reopening_a_window_while_it_closes_replaces_it_at_once() {
    in_own_process("window_reopen_mid_close");
}

#[gpui::test]
fn window_reopen_mid_close(cx: &mut gpui::TestAppContext) {
    if !running_alone("window_reopen_mid_close") {
        return;
    }
    let (view, cx) = open_docked(cx);
    open_w(&view, cx, anchored("btn"));
    wait(cx, 200);
    close_w(&view, cx);
    wait(cx, 100);
    assert!(moving(cx).is_some());

    // Same id and key, while the old one shrinks.
    open_w(&view, cx, anchored("btn"));
    assert_eq!(count_w(&view, cx), 1);
    assert!(view.update(cx, |this, _| this.overlays[0].closing.is_none()));
    assert_eq!(listed(&view, cx), serde_json::json!(["probe"]));
    // The old one's end would have removed it by id.
    wait(cx, 1000);
    assert_eq!(count_w(&view, cx), 1);
    assert_eq!(drawn(&view, cx), [100.0, 100.0, 300.0, 200.0]);

    // Under a new id, the key still replaces the closing one.
    close_w(&view, cx);
    view.update_in(cx, |this, window, cx| this.open_overlay("w2".into(), anchored("btn"), window, cx));
    redraw(cx);
    assert_eq!(view.update(cx, |this, _| this.overlays.iter().map(|o| o.id.clone()).collect::<Vec<_>>()), ["w2"]);
}

#[test]
fn a_moving_window_takes_no_input() {
    in_own_process("window_motion_input");
}

#[gpui::test]
fn window_motion_input(cx: &mut gpui::TestAppContext) {
    if !running_alone("window_motion_input") {
        return;
    }
    let (view, cx) = open_docked(cx);
    open_w(&view, cx, anchored("btn"));
    wait(cx, 100);
    let [x, y, w, h] = moving(cx).unwrap();
    let (cx0, cy0) = (x + w / 2.0, y + h / 2.0);
    cx.simulate_mouse_move(gpui::point(gpui::px(cx0), gpui::px(cy0)), None, gpui::Modifiers::default());
    cx.simulate_mouse_down(gpui::point(gpui::px(cx0), gpui::px(cy0)), gpui::MouseButton::Left, gpui::Modifiers::default());
    assert!(view.update(cx, |this, _| this.win_drag.is_none()), "a press on a growing window starts no drag");
    cx.simulate_mouse_up(gpui::point(gpui::px(cx0), gpui::px(cy0)), gpui::MouseButton::Left, gpui::Modifiers::default());
    wait(cx, 100);
    assert_eq!(drawn(&view, cx), [100.0, 100.0, 300.0, 200.0]);

    // Landed, it drags again.
    drag(cx, (250.0, 150.0), (300.0, 200.0));
    let landed = drawn(&view, cx);
    assert!(landed[0] >= 150.0 && landed[1] >= 150.0, "moved: {landed:?}");

    // A closing one takes none either.
    close_w(&view, cx);
    drag(cx, (300.0, 200.0), (500.0, 300.0));
    assert_eq!(moving(cx), Some(landed));
}

#[test]
fn reduce_motion_fades_anchored_windows_in_place() {
    in_own_process("window_anchor_reduce_motion");
}

#[gpui::test]
fn window_anchor_reduce_motion(cx: &mut gpui::TestAppContext) {
    if !running_alone("window_anchor_reduce_motion") {
        return;
    }
    let (view, cx) = open_docked(cx);
    cx.update(|_, cx| cx.set_reduce_motion(true));
    let target = [100.0, 100.0, 300.0, 200.0];
    open_w(&view, cx, anchored("btn"));
    assert_eq!(moving(cx), Some(target), "no growing, only a fade");
    wait(cx, 150);
    assert_eq!(moving(cx), None);
    close_w(&view, cx);
    assert_eq!(moving(cx), Some(target));
    wait(cx, 100);
    assert_eq!(moving(cx), Some(target));
    wait(cx, 50);
    assert_eq!(count_w(&view, cx), 0);
}
