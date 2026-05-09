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

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_px_close(actual: Pixels, expected: Pixels) {
        let delta = (actual - expected) / px(1.0);
        assert!(
            delta.abs() < 0.01,
            "expected {expected:?}, got {actual:?}, delta {delta}"
        );
    }

    fn test_bounds() -> Bounds<Pixels> {
        Bounds::new(point(px(90.0), px(0.0)), size(px(10.0), px(100.0)))
    }

    fn vertical_layout(scroll: Pixels, max: Pixels) -> Option<ScrollbarLayout> {
        scrollbar_layout(ScrollbarAxis::Vertical, test_bounds(), scroll, max, 1.0)
    }

    #[test]
    fn layout_is_absent_without_overflow() {
        assert!(vertical_layout(px(0.0), px(0.0)).is_none());
    }

    #[test]
    fn layout_uses_min_thumb_height_and_reaches_bottom() {
        let layout = vertical_layout(px(300.0), px(300.0))
            .expect("overflow should create a scrollbar layout");

        assert_px_close(layout.thumb_bounds.size.height, px(24.0));
        assert_px_close(layout.thumb_bounds.bottom(), px(97.0));
    }

    #[test]
    fn thumb_drag_maps_to_scroll_range_and_clamps() {
        let layout =
            vertical_layout(px(0.0), px(300.0)).expect("overflow should create a scrollbar layout");

        assert_px_close(
            scroll_for_thumb_drag(&layout, layout.track_bounds.top() - px(100.0), px(0.0)),
            px(0.0),
        );
        assert_px_close(
            scroll_for_thumb_drag(&layout, layout.track_bounds.bottom() + px(100.0), px(0.0)),
            px(300.0),
        );
    }

    #[test]
    fn track_click_pages_toward_pointer_and_clamps() {
        let layout = vertical_layout(px(150.0), px(300.0))
            .expect("overflow should create a scrollbar layout");

        assert_px_close(
            scroll_for_track_click(&layout, layout.thumb_bounds.top() - px(1.0), px(50.0)),
            px(0.0),
        );
        assert_px_close(
            scroll_for_track_click(&layout, layout.thumb_bounds.bottom() + px(1.0), px(250.0)),
            px(300.0),
        );
    }

    fn horizontal_test_bounds() -> Bounds<Pixels> {
        Bounds::new(point(px(0.0), px(90.0)), size(px(100.0), px(10.0)))
    }

    fn horizontal_layout(scroll: Pixels, max: Pixels) -> Option<ScrollbarLayout> {
        scrollbar_layout(
            ScrollbarAxis::Horizontal,
            horizontal_test_bounds(),
            scroll,
            max,
            1.0,
        )
    }

    #[test]
    fn horizontal_layout_is_absent_without_overflow() {
        assert!(horizontal_layout(px(0.0), px(0.0)).is_none());
    }

    #[test]
    fn horizontal_layout_uses_min_thumb_width_and_reaches_right_edge() {
        let layout = horizontal_layout(px(300.0), px(300.0))
            .expect("overflow should create a scrollbar layout");

        assert_px_close(layout.thumb_bounds.size.width, px(24.0));
        assert_px_close(layout.thumb_bounds.right(), px(97.0));
    }

    #[test]
    fn horizontal_thumb_drag_maps_to_scroll_range_and_clamps() {
        let layout = horizontal_layout(px(0.0), px(300.0))
            .expect("overflow should create a scrollbar layout");

        assert_px_close(
            scroll_for_thumb_drag(&layout, layout.track_bounds.left() - px(100.0), px(0.0)),
            px(0.0),
        );
        assert_px_close(
            scroll_for_thumb_drag(&layout, layout.track_bounds.right() + px(100.0), px(0.0)),
            px(300.0),
        );
    }

    #[test]
    fn horizontal_track_click_pages_toward_pointer_and_clamps() {
        let layout = horizontal_layout(px(150.0), px(300.0))
            .expect("overflow should create a scrollbar layout");

        assert_px_close(
            scroll_for_track_click(&layout, layout.thumb_bounds.left() - px(1.0), px(50.0)),
            px(0.0),
        );
        assert_px_close(
            scroll_for_track_click(&layout, layout.thumb_bounds.right() + px(1.0), px(250.0)),
            px(300.0),
        );
    }
}
