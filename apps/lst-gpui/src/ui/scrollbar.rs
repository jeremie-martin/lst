use gpui::{fill, point, px, rgb, size, Bounds, Pixels, Point, Window};

use crate::ui::theme::{metrics, Theme};

#[derive(Clone, Copy, Debug)]
pub(crate) enum ScrollbarAxis {
    Vertical,
    Horizontal,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ScrollbarLayout {
    pub(crate) track_bounds: Bounds<Pixels>,
    pub(crate) thumb_bounds: Bounds<Pixels>,
    max_scroll: Pixels,
    thumb_min: Pixels,
    thumb_travel: Pixels,
    axis: ScrollbarAxis,
}

impl ScrollbarAxis {
    pub(crate) fn pointer_offset(self, point: Point<Pixels>) -> Pixels {
        match self {
            Self::Vertical => point.y,
            Self::Horizontal => point.x,
        }
    }

    fn track_extent(self, bounds: Bounds<Pixels>) -> Pixels {
        match self {
            Self::Vertical => bounds.size.height,
            Self::Horizontal => bounds.size.width,
        }
    }

    fn thumb_bounds(
        self,
        track_bounds: Bounds<Pixels>,
        edge_pad: Pixels,
        thickness: Pixels,
        thumb_start: Pixels,
        thumb_extent: Pixels,
    ) -> Bounds<Pixels> {
        match self {
            Self::Vertical => Bounds::new(
                point(track_bounds.right() - edge_pad - thickness, thumb_start),
                size(thickness, thumb_extent),
            ),
            Self::Horizontal => Bounds::new(
                point(thumb_start, track_bounds.bottom() - edge_pad - thickness),
                size(thumb_extent, thickness),
            ),
        }
    }
}

pub(crate) fn scrollbar_layout(
    axis: ScrollbarAxis,
    track_bounds: Bounds<Pixels>,
    scroll_offset: Pixels,
    max_scroll: Pixels,
    scale: f32,
) -> Option<ScrollbarLayout> {
    let max_scroll = max_scroll.max(px(0.0));
    let track_extent = axis.track_extent(track_bounds);
    if max_scroll <= px(0.0) || track_extent <= px(0.0) {
        return None;
    }

    let edge_pad = metrics::px_for_scale(metrics::SCROLLBAR_EDGE_PAD, scale);
    let thickness = metrics::px_for_scale(metrics::SCROLLBAR_THUMB_WIDTH, scale);
    let min_thumb_extent = metrics::px_for_scale(metrics::SCROLLBAR_MIN_THUMB_HEIGHT, scale);
    let available = (track_extent - edge_pad * 2.0).max(px(0.0));
    if available <= px(0.0) {
        return None;
    }

    let content_extent = track_extent + max_scroll;
    let proportional_extent = available * (track_extent / content_extent);
    let thumb_extent = proportional_extent
        .max(min_thumb_extent.min(available))
        .min(available);
    let thumb_travel = (available - thumb_extent).max(px(0.0));
    let scroll_ratio = if max_scroll > px(0.0) {
        (scroll_offset.max(px(0.0)).min(max_scroll) / max_scroll).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let thumb_min = match axis {
        ScrollbarAxis::Vertical => track_bounds.top() + edge_pad,
        ScrollbarAxis::Horizontal => track_bounds.left() + edge_pad,
    };
    let thumb_start = thumb_min + thumb_travel * scroll_ratio;

    Some(ScrollbarLayout {
        track_bounds,
        thumb_bounds: axis.thumb_bounds(
            track_bounds,
            edge_pad,
            thickness,
            thumb_start,
            thumb_extent,
        ),
        max_scroll,
        thumb_min,
        thumb_travel,
        axis,
    })
}

pub(crate) fn scroll_for_thumb_drag(
    layout: &ScrollbarLayout,
    pointer_offset: Pixels,
    grab_offset: Pixels,
) -> Pixels {
    if layout.max_scroll <= px(0.0) || layout.thumb_travel <= px(0.0) {
        return px(0.0);
    }

    let raw_thumb_start = pointer_offset - grab_offset;
    let ratio = ((raw_thumb_start - layout.thumb_min) / layout.thumb_travel).clamp(0.0, 1.0);
    layout.max_scroll * ratio
}

pub(crate) fn scroll_for_track_click(
    layout: &ScrollbarLayout,
    pointer_offset: Pixels,
    current_scroll: Pixels,
) -> Pixels {
    let page = layout.axis.track_extent(layout.track_bounds);
    let thumb_start = layout.axis.pointer_offset(layout.thumb_bounds.origin);
    let thumb_end = thumb_start + layout.axis.track_extent(layout.thumb_bounds);
    let target = if pointer_offset < thumb_start {
        current_scroll - page
    } else if pointer_offset > thumb_end {
        current_scroll + page
    } else {
        current_scroll
    };
    target.max(px(0.0)).min(layout.max_scroll)
}

pub(crate) fn paint_scrollbar(
    layout: &ScrollbarLayout,
    active: bool,
    hovered: bool,
    scale: f32,
    theme: Theme,
    window: &mut Window,
) {
    let color = if active || hovered {
        theme.role.scrollbar_thumb_active
    } else {
        theme.role.scrollbar_thumb
    };
    let radius = metrics::px_for_scale(metrics::SCROLLBAR_THUMB_WIDTH / 2.0, scale);
    window.paint_quad(fill(layout.thumb_bounds, rgb(color)).corner_radii(radius));
}
