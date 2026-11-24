//! HERDR: `overlay_anchor(name)`: where an element is drawn, by name, so the
//! host can grow a window out of it and shrink the window back into it.
//!
//! An element keeps its entry alive through GPUI element state, which GPUI
//! drops at the end of the first frame that doesn't draw the element. A cached
//! view that GPUI reuses without prepainting carries that state over too, so
//! an idle panel's anchors stay; recording on every prepaint and clearing per
//! frame would lose them.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::{Rc, Weak},
};

use gpui::{AnyElement, App, Bounds, ElementId, IntoElement, Pixels, SharedString, Window, WindowId};

struct Anchor {
    window: WindowId,
    bounds: Cell<Bounds<Pixels>>,
}

/// One element's entry, held as its element state.
struct Drawn {
    name: SharedString,
    anchor: Rc<Anchor>,
}

thread_local! {
    /// Per name, its elements in the order they last prepainted.
    static ANCHORS: RefCell<HashMap<SharedString, Vec<Weak<Anchor>>>> = RefCell::default();
}

/// Where the element named `name` was last drawn in `window`, in window
/// pixels; None when no element by that name is drawn there now. Two drawn
/// at once: the one prepainted last.
pub fn overlay_anchor(window: WindowId, name: &str) -> Option<Bounds<Pixels>> {
    ANCHORS.with_borrow_mut(|all| {
        all.retain(|_, list| {
            list.retain(|a| a.strong_count() > 0);
            !list.is_empty()
        });
        all.get(name)?.iter().rev().filter_map(Weak::upgrade).find(|a| a.window == window).map(|a| a.bounds.get())
    })
}

fn register(name: &SharedString, anchor: &Rc<Anchor>) {
    ANCHORS.with_borrow_mut(|all| {
        let list = all.entry(name.clone()).or_default();
        list.retain(|a| a.strong_count() > 0 && !std::ptr::eq(a.as_ptr(), Rc::as_ptr(anchor)));
        list.push(Rc::downgrade(anchor));
    });
}

/// Wraps `element` so its bounds are found under `name`. `id` is the
/// element's own identity: its entry lives in element state under it.
pub(crate) fn record(element: AnyElement, name: SharedString, id: ElementId) -> AnyElement {
    Recorder { element, name, id }.into_any_element()
}

struct Recorder {
    element: AnyElement,
    name: SharedString,
    id: ElementId,
}

impl IntoElement for Recorder {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl gpui::Element for Recorder {
    type RequestLayoutState = ();
    type PrepaintState = ();

    // None: an id here would nest the wrapped element's own identity under it.
    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(&mut self, _: Option<&gpui::GlobalElementId>, _: Option<&gpui::InspectorElementId>, window: &mut Window, cx: &mut App) -> (gpui::LayoutId, ()) {
        (self.element.request_layout(window, cx), ())
    }

    fn prepaint(&mut self, _: Option<&gpui::GlobalElementId>, _: Option<&gpui::InspectorElementId>, bounds: Bounds<Pixels>, _: &mut (), window: &mut Window, cx: &mut App) {
        self.element.prepaint(window, cx);
        let at = window.window_handle().window_id();
        let name = self.name.clone();
        window.with_global_id(self.id.clone(), |id, window| {
            window.with_element_state::<Drawn, _>(id, |drawn, _| {
                let drawn = drawn.filter(|d| d.name == name && d.anchor.window == at).unwrap_or_else(|| Drawn {
                    name,
                    anchor: Rc::new(Anchor { window: at, bounds: Cell::new(bounds) }),
                });
                drawn.anchor.bounds.set(bounds);
                register(&drawn.name, &drawn.anchor);
                ((), drawn)
            })
        });
    }

    fn paint(&mut self, _: Option<&gpui::GlobalElementId>, _: Option<&gpui::InspectorElementId>, _: Bounds<Pixels>, _: &mut (), _: &mut (), window: &mut Window, cx: &mut App) {
        self.element.paint(window, cx);
    }
}
