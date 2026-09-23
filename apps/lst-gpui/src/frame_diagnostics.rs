//! Trace a complete root frame without adding a layout node or production work.
use gpui::{
    AnyElement, App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement, LayoutId, Pixels,
    Window,
};

use crate::diagnostics::{self, FrameClock};

pub(crate) fn observe(root: impl IntoElement, clock: Option<FrameClock>) -> AnyElement {
    let root = root.into_any_element();
    match clock {
        Some(clock) => FrameDiagnostics { root, clock }.into_any_element(),
        None => root,
    }
}

struct FrameDiagnostics {
    root: AnyElement,
    clock: FrameClock,
}

impl IntoElement for FrameDiagnostics {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for FrameDiagnostics {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.root.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let _ = self.root.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.root.paint(window, cx);
        diagnostics::record_first_frame();
        diagnostics::record_epoch("frame_end_epoch_us");
        diagnostics::record_frame(self.clock);
    }
}
