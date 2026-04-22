use gpui::{fill, point, px, rgb, size, Bounds, Pixels, Window};

use crate::ui::theme::{metrics, role};

#[derive(Clone, Copy, Debug)]
pub(crate) struct VerticalScrollbarLayout {
    pub(crate) track_bounds: Bounds<Pixels>,
    pub(crate) thumb_bounds: Bounds<Pixels>,
    pub(crate) max_scroll_top: Pixels,
    thumb_min_y: Pixels,
    thumb_travel: Pixels,
}

pub(crate) fn vertical_scrollbar_layout(
    track_bounds: Bounds<Pixels>,
    scroll_top: Pixels,
    max_scroll_top: Pixels,
    scale: f32,
) -> Option<VerticalScrollbarLayout> {
    let max_scroll_top = max_scroll_top.max(px(0.0));
    if max_scroll_top <= px(0.0) || track_bounds.size.height <= px(0.0) {
        return None;
    }

    let edge_pad = metrics::px_for_scale(metrics::SCROLLBAR_EDGE_PAD, scale);
    let thumb_width = metrics::px_for_scale(metrics::SCROLLBAR_THUMB_WIDTH, scale);
    let min_thumb_height = metrics::px_for_scale(metrics::SCROLLBAR_MIN_THUMB_HEIGHT, scale);
    let available_height = (track_bounds.size.height - edge_pad * 2.0).max(px(0.0));
    if available_height <= px(0.0) {
        return None;
    }

    let content_height = track_bounds.size.height + max_scroll_top;
    let proportional_height = available_height * (track_bounds.size.height / content_height);
    let thumb_height = proportional_height
        .max(min_thumb_height.min(available_height))
        .min(available_height);
    let thumb_travel = (available_height - thumb_height).max(px(0.0));
    let scroll_ratio = if max_scroll_top > px(0.0) {
        (scroll_top.max(px(0.0)).min(max_scroll_top) / max_scroll_top).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let thumb_min_y = track_bounds.top() + edge_pad;
    let thumb_top = thumb_min_y + thumb_travel * scroll_ratio;
    let thumb_left = track_bounds.right() - edge_pad - thumb_width;

    Some(VerticalScrollbarLayout {
        track_bounds,
        thumb_bounds: Bounds::new(
            point(thumb_left, thumb_top),
            size(thumb_width, thumb_height),
        ),
        max_scroll_top,
        thumb_min_y,
        thumb_travel,
    })
}

pub(crate) fn scroll_top_for_thumb_drag(
    layout: &VerticalScrollbarLayout,
    pointer_y: Pixels,
    grab_offset_y: Pixels,
) -> Pixels {
    if layout.max_scroll_top <= px(0.0) || layout.thumb_travel <= px(0.0) {
        return px(0.0);
    }

    let raw_thumb_top = pointer_y - grab_offset_y;
    let ratio = ((raw_thumb_top - layout.thumb_min_y) / layout.thumb_travel).clamp(0.0, 1.0);
    layout.max_scroll_top * ratio
}

pub(crate) fn scroll_top_for_track_click(
    layout: &VerticalScrollbarLayout,
    pointer_y: Pixels,
    current_scroll_top: Pixels,
) -> Pixels {
    let page_height = layout.track_bounds.size.height;
    let target = if pointer_y < layout.thumb_bounds.top() {
        current_scroll_top - page_height
    } else if pointer_y > layout.thumb_bounds.bottom() {
        current_scroll_top + page_height
    } else {
        current_scroll_top
    };
    target.max(px(0.0)).min(layout.max_scroll_top)
}

pub(crate) fn paint_vertical_scrollbar(
    layout: &VerticalScrollbarLayout,
    active: bool,
    hovered: bool,
    scale: f32,
    window: &mut Window,
) {
    let color = if active || hovered {
        role::SCROLLBAR_THUMB_ACTIVE
    } else {
        role::SCROLLBAR_THUMB
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

    #[test]
    fn layout_is_absent_without_overflow() {
        assert!(vertical_scrollbar_layout(test_bounds(), px(0.0), px(0.0), 1.0).is_none());
    }

    #[test]
    fn layout_uses_min_thumb_height_and_reaches_bottom() {
        let layout = vertical_scrollbar_layout(test_bounds(), px(300.0), px(300.0), 1.0)
            .expect("overflow should create a scrollbar layout");

        assert_px_close(layout.thumb_bounds.size.height, px(24.0));
        assert_px_close(layout.thumb_bounds.bottom(), px(97.0));
    }

    #[test]
    fn thumb_drag_maps_to_scroll_range_and_clamps() {
        let layout = vertical_scrollbar_layout(test_bounds(), px(0.0), px(300.0), 1.0)
            .expect("overflow should create a scrollbar layout");

        assert_px_close(
            scroll_top_for_thumb_drag(&layout, layout.track_bounds.top() - px(100.0), px(0.0)),
            px(0.0),
        );
        assert_px_close(
            scroll_top_for_thumb_drag(&layout, layout.track_bounds.bottom() + px(100.0), px(0.0)),
            px(300.0),
        );
    }

    #[test]
    fn track_click_pages_toward_pointer_and_clamps() {
        let layout = vertical_scrollbar_layout(test_bounds(), px(150.0), px(300.0), 1.0)
            .expect("overflow should create a scrollbar layout");

        assert_px_close(
            scroll_top_for_track_click(&layout, layout.thumb_bounds.top() - px(1.0), px(50.0)),
            px(0.0),
        );
        assert_px_close(
            scroll_top_for_track_click(&layout, layout.thumb_bounds.bottom() + px(1.0), px(250.0)),
            px(300.0),
        );
    }
}
