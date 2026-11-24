//! Windows: overlays a mod opens with `kind: "window"`. They stay until
//! closed, move where they're dropped (a press anywhere the mod doesn't keep
//! for itself drags), and the one pressed last is on top. With a `key` a
//! window remembers its spot, and opening that key again replaces it; with
//! `keep` it also reopens on the next start, in the layout that opened it.
//! A `resizable` one also follows its sides and corners as they're dragged,
//! and remembers that size under its key too.
//! A layout switch closes them (they belong to their layout, like its mods).

use super::*;

/// How far the pointer moves before a press becomes a drag.
const DRAG_START: f32 = 4.0;
/// How much of a window stays on screen however far it's dragged, px.
const KEEP_SHOWN: f32 = 48.0;
/// How thick a resize grip is, straddling the window's side, px.
const GRIP: f32 = 8.0;
/// The smallest a resizable window gets without `minWidth`/`minHeight`, as designed.
const MIN_WIDTH: f32 = KEEP_SHOWN * 2.0;
const MIN_HEIGHT: f32 = KEEP_SHOWN;
/// No `minWidth`/`minHeight` goes below this: up to 1 a size means a share of
/// the window, and below it the corner grips would cover each other.
const MIN_FLOOR: f32 = GRIP * 4.0;

pub struct WinDrag {
    pub id: String,
    /// The pointer's offset from the window's top left.
    grab: Point<Pixels>,
    down: Point<Pixels>,
    pointer: Point<Pixels>,
    moving: bool,
    /// Pressed on a grip: the drag resizes the window instead of moving it.
    resize: Option<Resize>,
}

#[derive(Clone, Copy)]
struct Resize {
    edge: ResizeEdge,
    /// The window as it was at the press; every move resizes from there.
    from: Bounds<Pixels>,
    min: Size<Pixels>,
    screen: Size<Pixels>,
}

/// `resizable: true` on a window's spec: the smallest it may be dragged to, as designed.
pub fn resize_limits(spec: &Value) -> Option<Size<f32>> {
    if spec.get("resizable").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    let num = |k: &str, d: f32| spec.get(k).and_then(Value::as_f64).map_or(d, |v| v as f32).max(MIN_FLOOR);
    Some(size(num("minWidth", MIN_WIDTH), num("minHeight", MIN_HEIGHT)))
}

/// Which sides a grip moves: left, top, right, bottom.
fn sides(edge: ResizeEdge) -> (bool, bool, bool, bool) {
    use ResizeEdge::*;
    (
        matches!(edge, Left | TopLeft | BottomLeft),
        matches!(edge, Top | TopLeft | TopRight),
        matches!(edge, Right | TopRight | BottomRight),
        matches!(edge, Bottom | BottomLeft | BottomRight),
    )
}

/// A window's bounds while `edge` is dragged from `down` to `pointer`: the
/// other sides stay, and the pointer counts only inside `screen`, so a dragged
/// side can't leave the app. It's never smaller than `min`, except that `min`
/// goes no bigger than `screen` (or a side would pass the app's edge) nor than
/// the window at the press (one opened smaller would jump off the pointer).
fn resized(from: Bounds<Pixels>, edge: ResizeEdge, down: Point<Pixels>, pointer: Point<Pixels>, min: Size<Pixels>, screen: Size<Pixels>) -> Bounds<Pixels> {
    let (left, top, right, bottom) = sides(edge);
    let min = size(min.width.min(screen.width).min(from.size.width), min.height.min(screen.height).min(from.size.height));
    let d = point(pointer.x.max(px(0.0)).min(screen.width), pointer.y.max(px(0.0)).min(screen.height)) - down;
    let (mut a, mut b) = (from.origin, from.bottom_right());
    if left {
        a.x = (a.x + d.x).min(b.x - min.width);
    }
    if right {
        b.x = (b.x + d.x).max(a.x + min.width);
    }
    if top {
        a.y = (a.y + d.y).min(b.y - min.height);
    }
    if bottom {
        b.y = (b.y + d.y).max(a.y + min.height);
    }
    Bounds::from_corners(a, b)
}

// ---- windows.json: spots by key, and the kept windows of each layout

fn path() -> PathBuf {
    file("windows.json")
}

fn read() -> Value {
    std::fs::read_to_string(path()).ok().and_then(|s| serde_json::from_str(&s).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}))
}

fn edit(f: impl FnOnce(&mut Value)) {
    let mut v = read();
    f(&mut v);
    if let Ok(s) = serde_json::to_string_pretty(&v) {
        let _ = crate::persist::write_atomic(&path(), s + "\n");
    }
}

/// `at` pulled back so KEEP_SHOWN px of a window of `size` stay in the viewport.
fn keep_shown(at: Point<Pixels>, size: Size<Pixels>, viewport: Size<Pixels>) -> Point<Pixels> {
    let keep = px(KEEP_SHOWN);
    point(at.x.min(viewport.width - keep).max(keep - size.width), at.y.min(viewport.height - keep).max(px(0.0)))
}

fn pair_of(map: &str, key: &str) -> Option<(f32, f32)> {
    let v = read();
    let p = v.get(map)?.get(key)?.as_array()?;
    Some((p.first()?.as_f64()? as f32, p.get(1)?.as_f64()? as f32))
}

pub fn place_of(key: &str) -> Option<Point<Pixels>> {
    pair_of("places", key).map(|(x, y)| point(px(x), px(y)))
}

/// A resizable window's size as last dragged, as designed (0: by its content,
/// up to 1: a share of the window). Files from before resizing have none.
pub fn size_of(key: &str) -> Option<Size<f32>> {
    pair_of("sizes", key).map(|(w, h)| size(w, h))
}

/// A dragged size reopened where it may no longer fit (saved on a bigger
/// display, or before `minWidth`/`minHeight` grew): at least `min`, and never
/// bigger than the app. 0 (never dragged) and shares stay as they are.
pub fn restored_size(saved: Size<f32>, min: Size<f32>, viewport: Size<Pixels>) -> Size<f32> {
    let zoom = settings::zoom();
    let fit = |v: f32, lo: f32, full: Pixels| if v > 1.0 { v.max(lo).min(f32::from(full) / zoom) } else { v };
    size(fit(saved.width, min.width, viewport.width), fit(saved.height, min.height, viewport.height))
}

/// `dragged` holds the sides this drag set; a side it didn't set keeps what
/// was saved for it (0 if never dragged), so it goes on following the spec.
fn remember(key: &str, at: Point<Pixels>, dragged: Option<Size<Option<f32>>>) {
    // Thousandths keep shares exact without writing f32 noise (523.4000244140625).
    let r = |v: f32| (f64::from(v) * 1000.0).round() / 1000.0;
    let (w, h) = pair_of("sizes", key).unwrap_or_default();
    edit(|v| {
        v["places"][key] = json!([f32::from(at.x).round(), f32::from(at.y).round()]);
        if let Some(s) = dragged {
            v["sizes"][key] = json!([r(s.width.unwrap_or(w)), r(s.height.unwrap_or(h))]);
        }
    });
}

pub fn keep(layout: &str, key: &str, spec: &Value) {
    edit(|v| v["kept"][layout][key] = spec.clone());
}

pub fn forget(layout: &str, key: &str) {
    if read().get("kept").and_then(|k| k.get(layout)).and_then(|l| l.get(key)).is_none() {
        return;
    }
    edit(|v| {
        if let Some(l) = v["kept"].get_mut(layout).and_then(Value::as_object_mut) {
            l.remove(key);
        }
    });
}

fn kept(layout: &str) -> Vec<Value> {
    read().get("kept").and_then(|k| k.get(layout)).and_then(Value::as_object).map(|l| l.values().cloned().collect()).unwrap_or_default()
}

impl AppWindow {
    /// The windows a layout keeps, reopened (at start, or on switching to it).
    pub(super) fn open_kept_windows(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for spec in kept(&self.layout_name) {
            self.open_overlay(mods::new_overlay_id(), spec, window, cx);
        }
    }

    /// Leaving a layout: its windows close (the kept ones come back with it).
    pub(super) fn close_windows(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The layout's windows go with it; the app's own (What's new) stay.
        let ids: Vec<String> = self.overlays.iter().filter(|o| o.kind == OverlayKind::Window && !o.frame).map(|o| o.id.clone()).collect();
        for id in ids {
            self.remove_overlay(&id, window, cx);
        }
    }

    /// A window at its spot: where it was dropped, else centered once its size
    /// is known (it stays invisible until then), always partly on screen.
    pub(super) fn render_window(&self, o: &Overlay, body: Stateful<Div>, viewport: Size<Pixels>, cx: &mut Context<Self>) -> AnyElement {
        let measured = self.bounds.borrow().get(&o.slot).map(|b| b.size);
        let size = measured.unwrap_or_default();
        let at = match (&self.win_drag, o.place) {
            (Some(d), _) if d.id == o.id && d.moving && d.resize.is_none() => d.pointer - d.grab,
            (_, Some(p)) => p,
            _ => point((viewport.width - size.width) / 2.0, (viewport.height - size.height) / 2.0),
        };
        let Point { x, y } = keep_shown(at, size, viewport);
        let measure = {
            let (bounds, slot, known) = (self.bounds.clone(), o.slot.clone(), measured.is_some());
            canvas(
                move |b, window, _| {
                    if !known {
                        window.request_animation_frame();
                    }
                    bounds.borrow_mut().insert(slot, b);
                },
                |_, _, _, _| {},
            )
            .absolute()
            .size_full()
        };
        let (raise, press) = (o.id.clone(), o.id.clone());
        let body = body
            .relative()
            .child(measure)
            // Pressed anywhere, even on the mod's own controls: to the front.
            .capture_any_mouse_down(cx.listener(move |this, _, _, cx| this.raise_window(&raise, cx)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.window_press(&press, ev.position, cx);
                }),
            );
        let grips = if o.resizable.is_some() { self.window_grips(&o.id, cx) } else { Vec::new() };
        div().absolute().left(x).top(y).when(measured.is_none(), |d| d.opacity(0.0)).child(body).children(grips).into_any_element()
    }

    /// Strips across a resizable window's sides and squares on its corners,
    /// above its content: a press there resizes and never reaches the mod.
    fn window_grips(&self, id: &str, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let (g, out) = (px(GRIP), px(-GRIP / 2.0));
        let grip = |edge: ResizeEdge, cursor: CursorStyle| {
            let id = id.to_owned();
            div()
                .id(SharedString::from(format!("grip-{id}-{edge:?}")))
                .absolute()
                .occlude()
                .cursor(cursor)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        this.raise_window(&id, cx);
                        this.resize_press(&id, edge, ev.position, window.viewport_size(), cx);
                    }),
                )
        };
        let corner = |edge, cursor| grip(edge, cursor).w(g * 2.0).h(g * 2.0);
        // Corners last: on top where they meet the strips.
        vec![
            grip(ResizeEdge::Top, CursorStyle::ResizeUpDown).top(out).left(g).right(g).h(g),
            grip(ResizeEdge::Bottom, CursorStyle::ResizeUpDown).bottom(out).left(g).right(g).h(g),
            grip(ResizeEdge::Left, CursorStyle::ResizeLeftRight).left(out).top(g).bottom(g).w(g),
            grip(ResizeEdge::Right, CursorStyle::ResizeLeftRight).right(out).top(g).bottom(g).w(g),
            corner(ResizeEdge::TopLeft, CursorStyle::ResizeUpLeftDownRight).top(out).left(out),
            corner(ResizeEdge::TopRight, CursorStyle::ResizeUpRightDownLeft).top(out).right(out),
            corner(ResizeEdge::BottomLeft, CursorStyle::ResizeUpRightDownLeft).bottom(out).left(out),
            corner(ResizeEdge::BottomRight, CursorStyle::ResizeUpLeftDownRight).bottom(out).right(out),
        ]
        .into_iter()
        .map(IntoElement::into_any_element)
        .collect()
    }

    fn resize_press(&mut self, id: &str, edge: ResizeEdge, at: Point<Pixels>, screen: Size<Pixels>, cx: &mut Context<Self>) {
        let Some(o) = self.overlays.iter().find(|o| o.id == id) else { return };
        let Some(min) = o.resizable else { return };
        let Some(from) = self.outer_bounds(o) else { return };
        let min = size(settings::z(min.width), settings::z(min.height));
        let resize = Resize { edge, from, min, screen };
        self.set_win_drag(Some(WinDrag { id: id.to_owned(), grab: at - from.origin, down: at, pointer: at, moving: false, resize: Some(resize) }), cx);
        cx.notify();
    }

    /// Every press on a window ends here, so a resize's terminals never keep
    /// holding their rows after it. Returns the press it ends.
    pub(super) fn set_win_drag(&mut self, drag: Option<WinDrag>, cx: &mut Context<Self>) -> Option<WinDrag> {
        let ended = std::mem::replace(&mut self.win_drag, drag);
        if let Some(d) = ended.as_ref().filter(|d| d.resize.is_some() && d.moving) {
            self.hold_terminal_rows(&d.id, false, cx);
        }
        ended
    }

    /// Only this window's terminals: a switch for every terminal would move
    /// the others' text up to half a line when a resize starts and ends.
    fn hold_terminal_rows(&self, id: &str, hold: bool, cx: &mut Context<Self>) {
        let Some(l) = self.overlays.iter().find(|o| o.id == id).and_then(|o| o.layout.as_ref()) else { return };
        for n in l.visible_blocks() {
            if matches!(n.content(), Some(Content::Terminal { .. }))
                && let Some(t) = self.pane_term(&n.id)
            {
                t.view.update(cx, |v, cx| {
                    if v.hold_rows_at_top(hold) {
                        cx.notify();
                    }
                });
            }
        }
    }

    fn resize_window(&mut self, id: &str, edge: ResizeEdge, b: Bounds<Pixels>, cx: &mut Context<Self>) {
        let Some(o) = self.overlays.iter_mut().find(|o| o.id == id) else { return };
        let (left, top, right, bottom) = sides(edge);
        // The 1 px border doesn't zoom, but width/height size the window
        // border included (border-box), so the outer size round-trips through `settings::z`.
        let zoom = settings::zoom();
        o.place = Some(b.origin);
        if left || right {
            o.size.width = f32::from(b.size.width) / zoom;
        }
        if top || bottom {
            o.size.height = f32::from(b.size.height) / zoom;
            // A fitted mod stays as tall as its content, not the dragged height.
            if o.fit {
                o.fit = false;
                if let Some(root) = self.copies.get(&o.slot).and_then(|c| c.root.clone()) {
                    root.update(cx, |r, cx| r.set_fit_content(false, cx));
                }
            }
        }
        cx.notify();
    }

    /// The window border included: its bounds are measured inside the border,
    /// but `size` and `place` include it.
    pub(super) fn outer_bounds(&self, o: &Overlay) -> Option<Bounds<Pixels>> {
        let b = self.bounds.borrow().get(&o.slot).copied()?;
        Some(if o.bare { b } else { b.dilate(px(1.0)) })
    }

    fn raise_window(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(i) = self.overlays.iter().position(|o| o.id == id) else { return };
        if i + 1 < self.overlays.len() {
            let o = self.overlays.remove(i);
            self.overlays.push(o);
            cx.notify();
        }
    }

    fn window_press(&mut self, id: &str, at: Point<Pixels>, cx: &mut Context<Self>) {
        // A terminal in the window keeps its presses (selecting text).
        let on_terminal = self.overlays.iter().find(|o| o.id == id).and_then(|o| o.layout.as_ref()).is_some_and(|l| {
            let bounds = self.bounds.borrow();
            l.visible_blocks().iter().any(|n| matches!(n.content(), Some(Content::Terminal { .. })) && bounds.get(&n.id).is_some_and(|b| b.contains(&at)))
        });
        if on_terminal {
            return;
        }
        // Outer, as `place` is: grabbed inside the border, a drop would shift it 1 px.
        let Some(b) = self.overlays.iter().find(|o| o.id == id).and_then(|o| self.outer_bounds(o)) else { return };
        self.set_win_drag(Some(WinDrag { id: id.to_owned(), grab: at - b.origin, down: at, pointer: at, moving: false, resize: None }), cx);
        cx.notify();
    }

    fn window_move(&mut self, at: Point<Pixels>, button: Option<MouseButton>, viewport: Size<Pixels>, cx: &mut Context<Self>) {
        let Some(d) = &mut self.win_drag else { return };
        match button {
            Some(MouseButton::Left) => {}
            // Another button reported while left is held (macOS sends a right
            // drag then): the left press is still on.
            Some(_) => return,
            // Released outside the app: no mouse up came, so this one ends the drag.
            None => {
                self.win_up_swallow = self.window_release(viewport, cx);
                return;
            }
        }
        d.pointer = at;
        let dist = f32::from((at.x - d.down.x).abs()).max(f32::from((at.y - d.down.y).abs()));
        if !d.moving && dist < DRAG_START {
            return;
        }
        let started = !d.moving;
        d.moving = true;
        let (id, down, resize) = (d.id.clone(), d.down, d.resize);
        let Some(r) = resize else {
            cx.notify();
            return;
        };
        // From the first real move, not the press: a click on a grip must leave the terminal's text where it is.
        if started {
            self.hold_terminal_rows(&id, true, cx);
        }
        self.resize_window(&id, r.edge, resized(r.from, r.edge, down, at, r.min, r.screen), cx);
    }

    /// Ends a press: a drag leaves the window there, at its size (and remembers
    /// them under its key); returns true so the release doesn't also click
    /// what's under it.
    fn window_release(&mut self, viewport: Size<Pixels>, cx: &mut Context<Self>) -> bool {
        let Some(d) = self.set_win_drag(None, cx) else { return false };
        cx.notify();
        if !d.moving {
            return false;
        }
        let at = d.pointer - d.grab;
        let measured = self.overlays.iter().find(|o| o.id == d.id).and_then(|o| self.bounds.borrow().get(&o.slot).map(|b| b.size));
        // A release outside the app reports a pointer far off screen; saving that would lose the window.
        let at = keep_shown(at, measured.unwrap_or_default(), viewport);
        if let Some(o) = self.overlays.iter_mut().find(|o| o.id == d.id) {
            if d.resize.is_none() {
                o.place = Some(at);
            }
            let dragged = d.resize.map(|r| {
                let (left, top, right, bottom) = sides(r.edge);
                size((left || right).then_some(o.size.width), (top || bottom).then_some(o.size.height))
            });
            if let (Some(k), Some(at)) = (&o.key, o.place) {
                remember(k, at, dragged);
            }
        }
        true
    }

    /// While a window is pressed: follows the pointer anywhere in the app.
    pub(super) fn window_tracker(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.win_drag.is_none() && !self.win_up_swallow {
            return None;
        }
        let this = cx.entity().downgrade();
        Some(
            canvas(
                |_, _, _| {},
                move |_, _, window, _| {
                    let t = this.clone();
                    window.on_mouse_event(move |ev: &MouseMoveEvent, phase, window, cx| {
                        if phase == DispatchPhase::Capture {
                            let viewport = window.viewport_size();
                            let _ = t.update(cx, |this, cx| this.window_move(ev.position, ev.pressed_button, viewport, cx));
                        }
                    });
                    let t = this.clone();
                    // A new press: its mouse up is a click of its own.
                    window.on_mouse_event(move |_: &MouseDownEvent, phase, _, cx| {
                        if phase == DispatchPhase::Capture {
                            let _ = t.update(cx, |this, _| this.win_up_swallow = false);
                        }
                    });
                    let t = this.clone();
                    window.on_mouse_event(move |ev: &MouseUpEvent, phase, window, cx| {
                        let viewport = window.viewport_size();
                        if phase != DispatchPhase::Capture || ev.button != MouseButton::Left {
                            return;
                        }
                        let ended = t
                            .update(cx, |this, cx| {
                                let swallowed = std::mem::take(&mut this.win_up_swallow);
                                if swallowed {
                                    cx.notify();
                                }
                                let released = this.window_release(viewport, cx);
                                swallowed || released
                            })
                            .unwrap_or(false);
                        if ended {
                            cx.stop_propagation();
                        }
                    });
                },
            )
            .absolute()
            .size_full()
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: it brings gpui's `test` attribute, which expands to `#[test]` again and recurses.
    use super::{MIN_FLOOR, MIN_HEIGHT, MIN_WIDTH, keep_shown, resize_limits, resized, restored_size};
    use gpui::{Bounds, Pixels, ResizeEdge, Size, point, px, size};
    use serde_json::json;

    fn clamped(at: (f32, f32), size: (f32, f32)) -> (f32, f32) {
        let p = keep_shown(point(px(at.0), px(at.1)), gpui::size(px(size.0), px(size.1)), gpui::size(px(1000.0), px(600.0)));
        (f32::from(p.x), f32::from(p.y))
    }

    #[test]
    fn a_drop_far_past_any_side_keeps_a_strip_of_the_window_shown() {
        // Left: the window's right 48 px stay on screen. Top: the title bar must not leave.
        assert_eq!(clamped((-9999.0, -9999.0), (300.0, 200.0)), (-252.0, 0.0));
        assert_eq!(clamped((9999.0, 9999.0), (300.0, 200.0)), (952.0, 552.0));
        assert_eq!(clamped((9999.0, -9999.0), (300.0, 200.0)), (952.0, 0.0));
        assert_eq!(clamped((-9999.0, 9999.0), (300.0, 200.0)), (-252.0, 552.0));
    }

    #[test]
    fn a_spot_already_shown_is_left_alone() {
        assert_eq!(clamped((100.0, 100.0), (300.0, 200.0)), (100.0, 100.0));
    }

    #[test]
    fn an_unmeasured_zero_size_window_is_still_pulled_inside() {
        assert_eq!(clamped((-500.0, -500.0), (0.0, 0.0)), (48.0, 0.0));
    }

    fn r(x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(w), px(h)))
    }

    const MIN: Size<Pixels> = Size { width: px(96.0), height: px(48.0) };
    const SCREEN: Size<Pixels> = Size { width: px(1000.0), height: px(600.0) };

    #[test]
    fn the_bottom_right_corner_grows_the_window_in_place() {
        let b = resized(r(100.0, 100.0, 300.0, 200.0), ResizeEdge::BottomRight, point(px(400.0), px(300.0)), point(px(460.0), px(340.0)), MIN, SCREEN);
        assert_eq!(b, r(100.0, 100.0, 360.0, 240.0));
    }

    #[test]
    fn the_left_side_moves_the_window_and_keeps_its_right_side() {
        let b = resized(r(100.0, 100.0, 300.0, 200.0), ResizeEdge::Left, point(px(100.0), px(200.0)), point(px(40.0), px(250.0)), MIN, SCREEN);
        assert_eq!(b, r(40.0, 100.0, 360.0, 200.0));
        let b = resized(r(100.0, 100.0, 300.0, 200.0), ResizeEdge::TopLeft, point(px(100.0), px(100.0)), point(px(150.0), px(130.0)), MIN, SCREEN);
        assert_eq!(b, r(150.0, 130.0, 250.0, 170.0));
    }

    #[test]
    fn each_side_and_corner_moves_only_its_own_sides() {
        let from = r(100.0, 100.0, 300.0, 200.0);
        let drag = |edge, down: (f32, f32)| resized(from, edge, point(px(down.0), px(down.1)), point(px(down.0 + 30.0), px(down.1 + 20.0)), MIN, SCREEN);
        assert_eq!(drag(ResizeEdge::Top, (250.0, 100.0)), r(100.0, 120.0, 300.0, 180.0));
        assert_eq!(drag(ResizeEdge::Right, (400.0, 200.0)), r(100.0, 100.0, 330.0, 200.0));
        assert_eq!(drag(ResizeEdge::Bottom, (250.0, 300.0)), r(100.0, 100.0, 300.0, 220.0));
        assert_eq!(drag(ResizeEdge::TopRight, (400.0, 100.0)), r(100.0, 120.0, 330.0, 180.0));
        assert_eq!(drag(ResizeEdge::BottomLeft, (100.0, 300.0)), r(130.0, 100.0, 270.0, 220.0));
    }

    #[test]
    fn a_window_smaller_than_its_minimum_does_not_jump_off_the_pointer() {
        // Opened at 60 × 30 under a 96 × 48 minimum: a 1 px move moves 1 px.
        let from = r(10.0, 10.0, 60.0, 30.0);
        assert_eq!(resized(from, ResizeEdge::Left, point(px(10.0), px(20.0)), point(px(11.0), px(20.0)), MIN, SCREEN), from);
        assert_eq!(resized(from, ResizeEdge::TopLeft, point(px(10.0), px(10.0)), point(px(9.0), px(9.0)), MIN, SCREEN), r(9.0, 9.0, 61.0, 31.0));
        assert_eq!(resized(from, ResizeEdge::BottomRight, point(px(70.0), px(40.0)), point(px(71.0), px(41.0)), MIN, SCREEN), r(10.0, 10.0, 61.0, 31.0));
    }

    #[test]
    fn a_minimum_bigger_than_the_app_stops_at_the_app() {
        // Wider than the app, under a minimum wider still: the left side still moves in.
        let from = r(-100.0, 0.0, 1200.0, 200.0);
        let huge = size(px(5000.0), px(5000.0));
        assert_eq!(resized(from, ResizeEdge::Left, point(px(0.0), px(100.0)), point(px(100.0), px(100.0)), huge, SCREEN), r(0.0, 0.0, 1100.0, 200.0));
        assert_eq!(resized(from, ResizeEdge::Left, point(px(0.0), px(100.0)), point(px(900.0), px(100.0)), huge, SCREEN), r(100.0, 0.0, 1000.0, 200.0));
    }

    #[test]
    fn a_restored_size_fits_the_app_and_the_minimum() {
        let (min, viewport) = (size(200.0, 48.0), size(px(1000.0), px(600.0)));
        assert_eq!(restored_size(size(3000.0, 2000.0), min, viewport), size(1000.0, 600.0));
        assert_eq!(restored_size(size(40.0, 40.0), min, viewport), size(200.0, 48.0));
        assert_eq!(restored_size(size(0.0, 0.5), min, viewport), size(0.0, 0.5), "never dragged, and a share, stay");
    }

    #[test]
    fn a_window_never_gets_smaller_than_its_minimum() {
        let from = r(100.0, 100.0, 300.0, 200.0);
        assert_eq!(resized(from, ResizeEdge::BottomRight, point(px(400.0), px(300.0)), point(px(0.0), px(0.0)), MIN, SCREEN), r(100.0, 100.0, 96.0, 48.0));
        assert_eq!(resized(from, ResizeEdge::TopLeft, point(px(100.0), px(100.0)), point(px(900.0), px(500.0)), MIN, SCREEN), r(304.0, 252.0, 96.0, 48.0));
    }

    #[test]
    fn a_dragged_side_stays_in_the_app() {
        let from = r(100.0, 100.0, 300.0, 200.0);
        assert_eq!(resized(from, ResizeEdge::BottomRight, point(px(400.0), px(300.0)), point(px(5000.0), px(5000.0)), MIN, SCREEN), r(100.0, 100.0, 900.0, 500.0));
        assert_eq!(resized(from, ResizeEdge::TopLeft, point(px(100.0), px(100.0)), point(px(-500.0), px(-500.0)), MIN, SCREEN), r(0.0, 0.0, 400.0, 300.0));
    }

    #[test]
    fn only_resizable_windows_get_limits_and_those_have_a_floor() {
        assert_eq!(resize_limits(&json!({"kind": "window"})), None);
        assert_eq!(resize_limits(&json!({"resizable": false, "minWidth": 200})), None);
        assert_eq!(resize_limits(&json!({"resizable": true})), Some(size(MIN_WIDTH, MIN_HEIGHT)));
        assert_eq!(resize_limits(&json!({"resizable": true, "minWidth": 240, "minHeight": 0.5})), Some(size(240.0, MIN_FLOOR)));
    }
}
