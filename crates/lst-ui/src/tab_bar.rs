use gpui::{
    div, px, rgb, AnyElement, App, InteractiveElement, IntoElement, ParentElement, RenderOnce,
    ScrollHandle, StatefulInteractiveElement, Styled,
};
use smallvec::SmallVec;

use crate::theme::{COLOR_BORDER, COLOR_SURFACE0, SHELL_EDGE_PAD, SHELL_GAP, TAB_HEIGHT};

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

    pub fn start_child(mut self, child: impl IntoElement) -> Self {
        self.start_children.push(child.into_any_element());
        self
    }

    pub fn end_child(mut self, child: impl IntoElement) -> Self {
        self.end_children.push(child.into_any_element());
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
        let tabs_row = if let Some(scroll_handle) = self.scroll_handle {
            div()
                .id("tabs-row")
                .flex()
                .gap_2()
                .overflow_x_scroll()
                .track_scroll(&scroll_handle)
                .children(self.children)
                .into_any_element()
        } else {
            div()
                .id("tabs-row")
                .flex()
                .gap_2()
                .overflow_x_scroll()
                .children(self.children)
                .into_any_element()
        };

        div()
            .id(self.id)
            .flex()
            .items_center()
            .w_full()
            .h(px(TAB_HEIGHT + 12.0))
            .px(px(SHELL_EDGE_PAD))
            .gap(px(SHELL_GAP))
            .bg(rgb(COLOR_SURFACE0))
            .border_b_1()
            .border_color(rgb(COLOR_BORDER))
            .children(
                (!self.start_children.is_empty()).then_some(
                    div()
                        .flex_none()
                        .flex()
                        .gap_2()
                        .children(self.start_children)
                        .into_any_element(),
                ),
            )
            .child(div().flex_1().min_w_0().overflow_x_hidden().child(tabs_row))
            .children(
                (!self.end_children.is_empty()).then_some(
                    div()
                        .flex_none()
                        .flex()
                        .gap_2()
                        .children(self.end_children)
                        .into_any_element(),
                ),
            )
    }
}
