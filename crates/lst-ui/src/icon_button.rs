use gpui::{
    div, px, rgb, App, CursorStyle, InteractiveElement, IntoElement, ParentElement, RenderOnce,
    Stateful, StatefulInteractiveElement, Styled,
};

use crate::theme::{
    COLOR_SUBTEXT, COLOR_SURFACE0, COLOR_SURFACE1, COLOR_SURFACE2, ICON_BUTTON_SIZE, TAB_TEXT_SIZE,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconKind {
    Close,
    Plus,
}

impl IconKind {
    fn label(self) -> &'static str {
        match self {
            Self::Close => "×",
            Self::Plus => "+",
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
            rgb(COLOR_SURFACE1)
        } else {
            rgb(COLOR_SURFACE0)
        };
        let hover = if self.emphasized {
            rgb(COLOR_SURFACE2)
        } else {
            rgb(COLOR_SURFACE1)
        };

        self.div
            .flex()
            .w(px(ICON_BUTTON_SIZE))
            .h(px(ICON_BUTTON_SIZE))
            .rounded_sm()
            .bg(background)
            .hover(|style| style.bg(hover))
            .active(|style| style.bg(rgb(COLOR_SURFACE2)))
            .cursor(CursorStyle::PointingHand)
            .items_center()
            .justify_center()
            .text_size(px(TAB_TEXT_SIZE))
            .text_color(rgb(COLOR_SUBTEXT))
            .child(self.icon.label())
    }
}
