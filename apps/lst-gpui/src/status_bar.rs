//! Status chrome whose unchanged layout and paint can be reused by GPUI.
use crate::{
    ui::theme::{metrics, Theme},
    LstGpuiApp,
};
use gpui::{
    div, prelude::*, rgb, AnyView, App, Bounds, Context, CursorStyle, Pixels, Render, StyleRefinement, WeakEntity,
    Window,
};

const LANGUAGE_BUTTON_HEIGHT: f32 = 22.0;

#[derive(PartialEq)]
struct StatusBarProps {
    scale: f32,
    theme: Theme,
    segments: Vec<String>,
    message: String,
    language: String,
    polishing: bool,
    polish_enabled: bool,
}

pub(crate) struct StatusBar {
    parent: WeakEntity<LstGpuiApp>,
    props: StatusBarProps,
}

impl LstGpuiApp {
    pub(crate) fn render_status_bar(&mut self, cx: &mut Context<Self>) -> AnyView {
        let scale = self.ui_scale();
        let theme = self.theme(cx);
        self.theme_name_rendered = theme.name.to_string();
        let segments = self.status_detail_segments();
        self.status_details_rendered = segments.join("  ");
        self.theme_button_bounds_px = None;
        let props = StatusBarProps {
            scale,
            theme,
            segments,
            message: self
                .cleanup_message
                .clone()
                .unwrap_or_else(|| self.model.status().to_string()),
            language: self
                .model
                .active_tab()
                .language()
                .map(|language| format!("{language:?}"))
                .unwrap_or_else(|| "Plain Text".to_string()),
            polishing: self.cleanup_in_flight,
            polish_enabled: !self.cleanup_in_flight
                && self.prompt_review.is_none()
                && self.active_tab().buffer().len_chars() > 0,
        };
        let mut changed = false;
        let view = if let Some(view) = &self.status_bar_view {
            if view.read(cx).props != props {
                changed = true;
                view.update(cx, |view, cx| {
                    view.props = props;
                    cx.notify();
                });
            }
            view.clone()
        } else {
            let parent = cx.entity().downgrade();
            let view = cx.new(|_| StatusBar { parent, props });
            self.status_bar_view = Some(view.clone());
            view
        };
        let view = AnyView::from(view);
        if changed {
            // A notification from parent render dirties the next draw.
            // Changed props must already be visible in this one.
            view
        } else {
            // Match the intrinsic height: tallest control, vertical padding,
            // and the unscaled one-pixel top border.
            let content_height = LANGUAGE_BUTTON_HEIGHT.max(metrics::STATUS_TEXT_LINE_HEIGHT);
            view.cached(
                StyleRefinement::default()
                    .flex_none()
                    .w_full()
                    .h(self.ui_px(content_height + 2.0 * metrics::STATUS_VERTICAL_PAD) + gpui::px(1.0)),
            )
        }
    }
}

impl Render for StatusBar {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let scale = self.props.scale;
        let theme = self.props.theme;
        let polishing = self.props.polishing;
        let polish_enabled = self.props.polish_enabled;
        let entity = self.parent.clone();
        let polish_button = div()
            .flex_none()
            .on_children_prepainted(move |bounds: Vec<Bounds<Pixels>>, _window, cx| {
                let _ = entity.update(cx, |this, _| {
                    this.cleanup_button_bounds_px = bounds.first().copied();
                });
            })
            .child(
                div()
                    .id("polish-prompt-button")
                    .flex_none()
                    .px_2()
                    .rounded_sm()
                    .bg(rgb(theme.role.control_bg))
                    .text_color(rgb(if polish_enabled {
                        theme.role.text
                    } else {
                        theme.role.text_muted
                    }))
                    .when(polish_enabled, |button| {
                        button
                            .cursor(CursorStyle::PointingHand)
                            .hover(move |style| style.bg(rgb(theme.role.control_bg_hover)))
                            .on_click(self.listener(|this, _, window, cx| {
                                this.force_editor_focus = true;
                                this.dispatch_workspace_command(
                                    crate::workspace_action::WorkspaceCommand::CleanupText,
                                    window,
                                    cx,
                                );
                                cx.stop_propagation();
                            }))
                    })
                    .child(if polishing {
                        "Improving prompt…"
                    } else {
                        "Improve Prompt"
                    }),
            );
        div()
            .w_full()
            .flex_none()
            .flex()
            .justify_between()
            .items_center()
            .gap_3()
            .px_3()
            .py(metrics::px_for_scale(metrics::STATUS_VERTICAL_PAD, scale))
            .bg(rgb(theme.role.panel_bg))
            .border_t_1()
            .border_color(rgb(theme.role.border))
            .text_size(metrics::px_for_scale(metrics::STATUS_TEXT_SIZE, scale))
            .line_height(metrics::px_for_scale(metrics::STATUS_TEXT_LINE_HEIGHT, scale))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(rgb(theme.role.text_subtle))
                    .child(self.props.message.clone()),
            )
            .child(
                div()
                    .id("new-voice-note")
                    .flex_none()
                    .px_2()
                    .rounded_sm()
                    .bg(rgb(theme.role.control_bg))
                    .text_color(rgb(theme.role.text))
                    .cursor(CursorStyle::PointingHand)
                    .hover(move |style| style.bg(rgb(theme.role.control_bg_hover)))
                    .on_click(self.listener(|this, _, window, cx| {
                        this.dispatch_workspace_command(
                            crate::workspace_action::WorkspaceCommand::NewVoiceNote,
                            window,
                            cx,
                        );
                        cx.stop_propagation();
                    }))
                    .child("Dictate"),
            )
            .child(polish_button)
            .child(
                div()
                    .flex()
                    .min_w_0()
                    .overflow_hidden()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .id("language-mode-button")
                            .flex()
                            .items_center()
                            .h(metrics::px_for_scale(LANGUAGE_BUTTON_HEIGHT, scale))
                            .px_2()
                            .rounded_sm()
                            .text_color(rgb(theme.role.text_subtle))
                            .cursor(CursorStyle::PointingHand)
                            .hover(move |style| style.bg(rgb(theme.role.control_bg_hover)))
                            .on_click(self.listener(|this, _, _, cx| {
                                this.toggle_language_menu(cx);
                                cx.stop_propagation();
                            }))
                            .child(self.props.language.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .min_w_0()
                            .overflow_hidden()
                            .items_center()
                            .gap_2()
                            .text_color(rgb(theme.role.text_subtle))
                            .children(
                                self.props
                                    .segments
                                    .iter()
                                    .cloned()
                                    .map(|segment| div().flex_none().child(segment).into_any_element()),
                            ),
                    ),
            )
    }
}

impl StatusBar {
    fn listener<E: ?Sized>(
        &self,
        f: impl Fn(&mut LstGpuiApp, &E, &mut Window, &mut Context<LstGpuiApp>) + 'static,
    ) -> impl Fn(&E, &mut Window, &mut App) + 'static {
        let parent = self.parent.clone();
        move |event, window, cx| {
            let _ = parent.update(cx, |app, cx| f(app, event, window, cx));
        }
    }
}
