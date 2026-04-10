use gpui::{
    div, px, rgb, AnyElement, App, CursorStyle, InteractiveElement, IntoElement, ParentElement,
    RenderOnce, SharedString, Stateful, StatefulInteractiveElement, Styled,
};
use smallvec::SmallVec;

use crate::theme::{
    COLOR_ACCENT, COLOR_BORDER, COLOR_SUBTEXT, COLOR_SURFACE0, COLOR_SURFACE1, COLOR_TEXT,
    SHELL_GAP, TAB_HEIGHT, TAB_HORIZONTAL_PAD, TAB_MAX_WIDTH, TAB_MIN_WIDTH, TAB_SLOT_WIDTH,
    TAB_TEXT_SIZE,
};

#[derive(IntoElement)]
pub struct Tab {
    div: Stateful<gpui::Div>,
    active: bool,
    group_name: SharedString,
    children: SmallVec<[AnyElement; 2]>,
    start_slot: Option<AnyElement>,
    end_slot: Option<AnyElement>,
}

impl Tab {
    pub fn new(id: impl Into<gpui::ElementId>) -> Self {
        let id = id.into();
        Self {
            div: div().id(id.clone()),
            active: false,
            group_name: format!("tab-{id:?}").into(),
            children: SmallVec::new(),
            start_slot: None,
            end_slot: None,
        }
    }

    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    pub fn group_name(mut self, group_name: impl Into<SharedString>) -> Self {
        self.group_name = group_name.into();
        self
    }

    pub fn start_slot(mut self, element: Option<AnyElement>) -> Self {
        self.start_slot = element;
        self
    }

    pub fn end_slot(mut self, element: Option<AnyElement>) -> Self {
        self.end_slot = element;
        self
    }
}

impl InteractiveElement for Tab {
    fn interactivity(&mut self) -> &mut gpui::Interactivity {
        self.div.interactivity()
    }
}

impl StatefulInteractiveElement for Tab {}

impl ParentElement for Tab {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Tab {
    fn render(self, _window: &mut gpui::Window, _cx: &mut App) -> impl IntoElement {
        let background = if self.active {
            rgb(COLOR_SURFACE1)
        } else {
            rgb(COLOR_SURFACE0)
        };
        let text = if self.active {
            rgb(COLOR_TEXT)
        } else {
            rgb(COLOR_SUBTEXT)
        };

        self.div
            .group(self.group_name)
            .relative()
            .flex()
            .flex_none()
            .h(px(TAB_HEIGHT))
            .min_w(px(TAB_MIN_WIDTH))
            .max_w(px(TAB_MAX_WIDTH))
            .px(px(TAB_HORIZONTAL_PAD))
            .gap(px(SHELL_GAP))
            .items_center()
            .border_r_1()
            .border_color(rgb(COLOR_BORDER))
            .bg(background)
            .cursor(CursorStyle::PointingHand)
            .hover(|style| style.bg(rgb(COLOR_SURFACE1)))
            .child(
                div()
                    .h_full()
                    .w(px(TAB_SLOT_WIDTH))
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .children(self.start_slot),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .h_full()
                    .items_center()
                    .text_size(px(TAB_TEXT_SIZE))
                    .text_color(text)
                    .truncate()
                    .children(self.children),
            )
            .child(
                div()
                    .h_full()
                    .w(px(TAB_SLOT_WIDTH))
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .children(self.end_slot),
            )
            .children(
                self.active.then_some(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .h(px(2.0))
                        .bg(rgb(COLOR_ACCENT))
                        .into_any_element(),
                ),
            )
    }
}
