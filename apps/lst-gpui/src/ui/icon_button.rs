use gpui::{
    div, rgb, App, CursorStyle, InteractiveElement, IntoElement, ParentElement, RenderOnce,
    Stateful, StatefulInteractiveElement, Styled,
};

use crate::ui::theme::{metrics, Theme};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconKind {
    Close,
    Plus,
    Recent,
    Theme,
}

impl IconKind {
    fn label(self) -> &'static str {
        match self {
            Self::Close => "×",
            Self::Plus => "+",
            Self::Recent => "≡",
            Self::Theme => "◐",
        }
    }
}

#[derive(IntoElement)]
pub struct IconButton {
    div: Stateful<gpui::Div>,
    icon: IconKind,
    theme: Theme,
    emphasized: bool,
}

impl IconButton {
    pub fn new(id: impl Into<gpui::ElementId>, icon: IconKind, theme: Theme) -> Self {
        Self {
            div: div().id(id.into()),
            icon,
            theme,
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
    fn render(self, window: &mut gpui::Window, _cx: &mut App) -> impl IntoElement {
        let rem_size = window.rem_size();
        let background = if self.emphasized {
            rgb(self.theme.role.control_bg)
        } else {
            rgb(self.theme.role.panel_bg)
        };
        let hover = if self.emphasized {
            rgb(self.theme.role.control_bg_hover)
        } else {
            rgb(self.theme.role.control_bg)
        };
        let active_bg = self.theme.role.control_bg_hover;

        self.div
            .flex()
            .w(metrics::px_for_rem(metrics::ICON_BUTTON_SIZE, rem_size))
            .h(metrics::px_for_rem(metrics::ICON_BUTTON_SIZE, rem_size))
            .rounded_sm()
            .bg(background)
            .hover(|style| style.bg(hover))
            .active(move |style| style.bg(rgb(active_bg)))
            .cursor(CursorStyle::PointingHand)
            .items_center()
            .justify_center()
            .text_size(metrics::px_for_rem(metrics::TAB_TEXT_SIZE, rem_size))
            .text_color(rgb(self.theme.role.text_subtle))
            .child(self.icon.label())
    }
}
