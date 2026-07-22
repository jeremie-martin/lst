use gpui::{
    div, font, prelude::FluentBuilder, rgb, App, AppContext, Context, CursorStyle, InteractiveElement, IntoElement,
    ParentElement, Render, RenderOnce, Stateful, StatefulInteractiveElement, Styled, Window,
};
use lucide_icons::Icon;

use crate::ui::theme::{metrics, Theme};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconKind {
    Close,
    Plus,
    Minus,
    Recent,
    Menu,
    ChevronUp,
    ChevronDown,
    ChevronRight,
    Replace,
    ReplaceAll,
}

impl IconKind {
    fn icon(self) -> Icon {
        match self {
            Self::Close => Icon::X,
            Self::Plus => Icon::Plus,
            Self::Minus => Icon::Minus,
            Self::Recent => Icon::History,
            Self::Menu => Icon::Menu,
            Self::ChevronUp => Icon::ChevronUp,
            Self::ChevronDown => Icon::ChevronDown,
            Self::ChevronRight => Icon::ChevronRight,
            Self::Replace => Icon::Replace,
            Self::ReplaceAll => Icon::ReplaceAll,
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
    tooltip: Option<String>,
}

impl IconButton {
    pub fn new(id: impl Into<gpui::ElementId>, icon: IconKind, theme: Theme) -> Self {
        Self {
            div: div().id(id.into()),
            icon,
            theme,
            emphasized: false,
            disabled: false,
            tooltip: None,
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

    pub fn tooltip(mut self, label: impl Into<String>) -> Self {
        self.tooltip = Some(label.into());
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
        let active_bg = self.theme.role.selection_bg;
        let foreground = if self.disabled {
            rgb(self.theme.role.text_muted)
        } else {
            rgb(self.theme.role.text_subtle)
        };
        let interactive = !self.disabled;

        let icon = char::from(self.icon.icon()).to_string();
        let tooltip_theme = self.theme;
        self.div
            .flex()
            .w(metrics::px_for_rem(metrics::ICON_BUTTON_SIZE, rem_size))
            .h(metrics::px_for_rem(metrics::ICON_BUTTON_SIZE, rem_size))
            .rounded_sm()
            .when(self.emphasized, |button| {
                button.border_1().border_color(rgb(self.theme.role.control_border))
            })
            .bg(background)
            .when(self.disabled, |button| button.opacity(0.55))
            .when(interactive, |s| {
                s.hover(move |style| style.bg(hover))
                    .active(move |style| style.bg(rgb(active_bg)).opacity(0.82))
                    .cursor(CursorStyle::PointingHand)
            })
            .items_center()
            .justify_center()
            .font(font("lucide"))
            .text_size(metrics::px_for_rem(metrics::TAB_TEXT_SIZE, rem_size))
            .text_color(foreground)
            .when_some(self.tooltip, |button, tooltip| {
                button.tooltip(move |_window, cx| {
                    cx.new(|_| Tooltip {
                        text: tooltip.clone(),
                        theme: tooltip_theme,
                    })
                    .into()
                })
            })
            .child(icon)
    }
}

struct Tooltip {
    text: String,
    theme: Theme,
}

impl Render for Tooltip {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded_sm()
            .border_1()
            .border_color(rgb(self.theme.role.border))
            .bg(rgb(self.theme.role.control_bg))
            .text_size(metrics::px_for_rem(metrics::UI_TEXT_SM, window.rem_size()))
            .text_color(rgb(self.theme.role.text))
            .child(self.text.clone())
    }
}
