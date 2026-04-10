use gpui::{
    div, px, rgb, App, CursorStyle, InteractiveElement, IntoElement, ParentElement, RenderOnce,
    Stateful, StatefulInteractiveElement, Styled,
};

use crate::theme::{
    COLOR_ACCENT, COLOR_BORDER, COLOR_SUBTEXT, COLOR_SURFACE1, COLOR_SURFACE2, ICON_BUTTON_SIZE,
    TAB_TEXT_SIZE,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconKind {
    Close,
}

impl IconKind {
    fn label(self) -> &'static str {
        match self {
            Self::Close => "x",
        }
    }
}

#[derive(IntoElement)]
pub struct IconButton {
    div: Stateful<gpui::Div>,
    icon: IconKind,
    emphasized: bool,
}

impl IconButton {
    pub fn new(id: impl Into<gpui::ElementId>, icon: IconKind) -> Self {
        Self {
            div: div().id(id.into()),
            icon,
            emphasized: false,
        }
    }

    pub fn emphasized(mut self, emphasized: bool) -> Self {
        self.emphasized = emphasized;
        self
    }
}

impl InteractiveElement for IconButton {
    fn interactivity(&mut self) -> &mut gpui::Interactivity {
        self.div.interactivity()
    }
}

impl StatefulInteractiveElement for IconButton {}

impl ParentElement for IconButton {
    fn extend(&mut self, _elements: impl IntoIterator<Item = gpui::AnyElement>) {}
}

impl RenderOnce for IconButton {
    fn render(self, _window: &mut gpui::Window, _cx: &mut App) -> impl IntoElement {
        let background = if self.emphasized {
            rgb(COLOR_SURFACE2)
        } else {
            rgb(COLOR_SURFACE1)
        };
        let hover = if self.emphasized {
            rgb(COLOR_ACCENT)
        } else {
            rgb(COLOR_SURFACE2)
        };

        self.div
            .w(px(ICON_BUTTON_SIZE))
            .h(px(ICON_BUTTON_SIZE))
            .rounded_sm()
            .border_1()
            .border_color(rgb(COLOR_BORDER))
            .bg(background)
            .hover(|style| style.bg(hover))
            .cursor(CursorStyle::PointingHand)
            .items_center()
            .justify_center()
            .text_size(px(TAB_TEXT_SIZE))
            .text_color(rgb(COLOR_SUBTEXT))
            .child(self.icon.label())
    }
}
