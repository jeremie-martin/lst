use gpui::{
    div, px, rgb, AnyElement, App, InteractiveElement, IntoElement, ParentElement, RenderOnce,
    ScrollHandle, StatefulInteractiveElement, Styled,
};
use smallvec::SmallVec;

use crate::ui::theme::{metrics, role};

#[derive(IntoElement)]
pub struct TabBar {
    id: gpui::ElementId,
    start_children: SmallVec<[AnyElement; 2]>,
    children: SmallVec<[AnyElement; 4]>,
    end_children: SmallVec<[AnyElement; 2]>,
    scroll_handle: Option<ScrollHandle>,
}

impl TabBar {
    pub fn new(id: impl Into<gpui::ElementId>) -> Self {
        Self {
            id: id.into(),
            start_children: SmallVec::new(),
            children: SmallVec::new(),
            end_children: SmallVec::new(),
            scroll_handle: None,
        }
    }

    pub fn track_scroll(mut self, scroll_handle: &ScrollHandle) -> Self {
        self.scroll_handle = Some(scroll_handle.clone());
        self
    }
}

impl ParentElement for TabBar {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for TabBar {
    fn render(self, _window: &mut gpui::Window, _cx: &mut App) -> impl IntoElement {
        let tabs_row = div().id("tabs-row").flex().h_full().children(self.children);

        let tabs_scroll = if let Some(scroll_handle) = self.scroll_handle {
            div()
                .id("tabs-scroll")
                .h_full()
                .overflow_x_scroll()
                .overflow_y_hidden()
                .track_scroll(&scroll_handle)
                .child(tabs_row)
                .into_any_element()
        } else {
            div()
                .id("tabs-scroll")
                .h_full()
                .overflow_x_scroll()
                .overflow_y_hidden()
                .child(tabs_row)
                .into_any_element()
        };

        div()
            .id(self.id)
            .flex()
            .w_full()
            .h(px(metrics::TAB_HEIGHT + 1.0))
            .overflow_hidden()
            .bg(rgb(role::PANEL_BG))
            .border_1()
            .border_color(rgb(role::BORDER))
            .children(
                (!self.start_children.is_empty()).then_some(
                    div()
                        .flex_none()
                        .flex()
                        .h_full()
                        .px(px(metrics::SHELL_EDGE_PAD))
                        .gap(px(metrics::SHELL_GAP))
                        .items_center()
                        .border_r_1()
                        .border_color(rgb(role::BORDER))
                        .children(self.start_children)
                        .into_any_element(),
                ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_hidden()
                    .child(tabs_scroll),
            )
            .children(
                (!self.end_children.is_empty()).then_some(
                    div()
                        .flex_none()
                        .flex()
                        .h_full()
                        .px(px(metrics::SHELL_EDGE_PAD))
                        .gap(px(metrics::SHELL_GAP))
                        .items_center()
                        .border_l_1()
                        .border_color(rgb(role::BORDER))
                        .children(self.end_children)
                        .into_any_element(),
                ),
            )
    }
}
