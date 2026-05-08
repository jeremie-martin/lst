use gpui::{
    div, prelude::FluentBuilder, rgb, App, CursorStyle, InteractiveElement, IntoElement,
    ParentElement, RenderOnce, Stateful, StatefulInteractiveElement, Styled,
};

use crate::ui::theme::{metrics, Theme};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconKind {
    Close,
    Plus,
    Recent,
    Sparkle,
    Theme,
}

impl IconKind {
    fn label(self) -> &'static str {
        match self {
            Self::Close => "×",
            Self::Plus => "+",
            Self::Recent => "↺",
            Self::Sparkle => "✦",
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
    disabled: bool,
}

impl IconButton {
    pub fn new(id: impl Into<gpui::ElementId>, icon: IconKind, theme: Theme) -> Self {
        Self {
            div: div().id(id.into()),
            icon,
            theme,
            emphasized: false,
            disabled: false,
        }
    }

    pub fn emphasized(mut self, emphasized: bool) -> Self {
        self.emphasized = emphasized;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl InteractiveElement for IconButton {
    fn interactivity(&mut self) -> &mut gpui::Interactivity {
        self.div.interactivity()
    }
}

impl StatefulInteractiveElement for IconButton {}

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
        let foreground = if self.disabled {
            rgb(self.theme.role.text_muted)
        } else {
            rgb(self.theme.role.text_subtle)
        };
        let interactive = !self.disabled;

        self.div
            .flex()
            .w(metrics::px_for_rem(metrics::ICON_BUTTON_SIZE, rem_size))
            .h(metrics::px_for_rem(metrics::ICON_BUTTON_SIZE, rem_size))
            .rounded_sm()
            .bg(background)
            .when(interactive, |s| {
                s.hover(move |style| style.bg(hover))
                    .active(move |style| style.bg(rgb(active_bg)))
                    .cursor(CursorStyle::PointingHand)
            })
            .items_center()
            .justify_center()
            .text_size(metrics::px_for_rem(metrics::TAB_TEXT_SIZE, rem_size))
            .text_color(foreground)
            .child(self.icon.label())
    }
}
