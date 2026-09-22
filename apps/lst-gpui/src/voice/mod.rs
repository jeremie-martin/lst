//! Native batch dictation. The editor receives only finalized segment deltas;
//! capture, network work, and recoverable audio belong to this app boundary.
mod audio;
mod provider;
mod session;

use crate::LstGpuiApp;
use gpui::{div, prelude::*, rgb, Context, IntoElement, Task};
use std::{path::PathBuf, time::Duration};

#[derive(Default)]
pub(crate) struct Voice {
    session: Option<session::Session>,
    task: Option<Task<()>>,
    message: Option<String>,
}

#[derive(Clone, Copy)]
pub(crate) enum VoiceAction {
    Toggle,
    Finish,
    Cancel,
    Retry,
    ShowNote,
}

impl Voice {
    pub fn active(&self) -> bool {
        self.session.is_some()
    }
    pub fn status(&self) -> Option<String> {
        self.session
            .as_ref()
            .map(|s| s.status())
            .or_else(|| self.message.clone())
    }
}

impl LstGpuiApp {
    pub(crate) fn start_voice_note(&mut self, cx: &mut Context<Self>) {
        if self.voice.active() {
            self.voice_action(VoiceAction::ShowNote, cx);
            return;
        }
        let result = (|| {
            let provider = provider::Provider::new(&self.settings.settings.voice.language)?;
            let directory = self
                .settings
                .settings
                .voice
                .directory
                .clone()
                .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join("audio-notes")))
                .ok_or("Cannot locate audio-notes: HOME is not set")?;
            let (path, stamp) = crate::runtime::create_scratchpad_note(Some(&directory))
                .map_err(|e| format!("Could not create voice note: {e}"))?;
            self.update_model(cx, true, |model| model.new_scratchpad_tab(path, stamp));
            session::Session::new(self.model.active_tab_id(), provider)
        })();
        match result {
            Ok(session) => {
                self.voice.session = Some(session);
                self.voice.message = None;
                self.voice.task = Some(cx.spawn(async move |this, cx| loop {
                    cx.background_executor().timer(Duration::from_millis(100)).await;
                    if !this.update(cx, |view, cx| view.poll_voice(cx)).unwrap_or(false) {
                        break;
                    }
                }));
            }
            Err(error) => self.voice.message = Some(error),
        }
        self.force_editor_focus = true;
        cx.notify();
    }

    fn poll_voice(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(session) = self.voice.session.as_mut() else {
            return false;
        };
        let before = session.status();
        let recording = session.recording();
        session.poll();
        let tab = session.tab;
        let text = self
            .model
            .tab_by_id(tab)
            .filter(|tab| tab.marked_range().is_none())
            .and_then(|_| session.transcript())
            .map(str::to_owned);
        if let Some(text) = text {
            let mut accepted = false;
            self.update_model(cx, true, |model| {
                accepted = model.append_text_to_tab(tab, &text);
                if accepted {
                    model.request_save_tab(tab);
                }
            });
            if accepted {
                self.voice.session.as_mut().expect("active session").accept_transcript();
            }
        }
        let session = self.voice.session.as_ref().expect("active session");
        let done = session.done();
        let changed = before != session.status();
        if done {
            let notice = session.notice.clone();
            self.voice.session = None;
            self.voice.message = Some(notice.map_or_else(
                || "Dictation finished.".into(),
                |notice| format!("Dictation finished. {notice}"),
            ));
        }
        if done || changed || recording {
            cx.notify();
        }
        !done
    }

    pub(crate) fn voice_action(&mut self, action: VoiceAction, cx: &mut Context<Self>) {
        if matches!(action, VoiceAction::Cancel) {
            self.voice.session = None;
            self.voice.task = None;
            self.voice.message = Some("Pending dictation cancelled. Text already inserted is kept.".into());
        } else if let Some(session) = self.voice.session.as_mut() {
            match action {
                VoiceAction::Toggle => session.toggle(),
                VoiceAction::Finish => session.finish(),
                VoiceAction::Retry => session.retry(),
                VoiceAction::ShowNote => {
                    let tab = session.tab;
                    self.update_model(cx, true, |model| model.set_active_tab(tab));
                }
                VoiceAction::Cancel => unreachable!(),
            }
        }
        self.force_editor_focus = true;
        cx.notify();
    }

    pub(crate) fn voice_blocks_close(&mut self, tab: Option<lst_editor::TabId>, cx: &mut Context<Self>) -> bool {
        let blocked = self
            .voice
            .session
            .as_ref()
            .is_some_and(|s| tab.is_none_or(|id| id == s.tab));
        if blocked {
            if let Some(session) = &mut self.voice.session {
                session.notice = Some("Finish or cancel pending dictation before closing.".into());
            }
            cx.notify();
        }
        blocked
    }

    pub(crate) fn render_voice_bar(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = self.theme(cx);
        let mut buttons = Vec::new();
        if let Some(session) = &self.voice.session {
            buttons.push(("voice-show", "Show Note", VoiceAction::ShowNote));
            if session.can_toggle() {
                buttons.push((
                    "voice-toggle",
                    if session.recording() { "Pause" } else { "Resume" },
                    VoiceAction::Toggle,
                ));
            }
            buttons.push(("voice-finish", "Finish", VoiceAction::Finish));
            if session.failed() {
                buttons.push(("voice-retry", "Retry", VoiceAction::Retry));
            }
            buttons.push(("voice-cancel", "Cancel Pending", VoiceAction::Cancel));
        }
        let level = self.voice.session.as_ref().map_or(0, |s| s.level());
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .py_1()
            .bg(rgb(theme.role.panel_bg))
            .border_t_1()
            .border_color(rgb(theme.role.border))
            .text_size(self.ui_px(12.0))
            .text_color(rgb(theme.role.text))
            .when(self.voice.active(), |bar| {
                bar.child(
                    div()
                        .w(self.ui_px(36.0))
                        .h(self.ui_px(6.0))
                        .bg(rgb(theme.role.control_bg))
                        .child(
                            div()
                                .w(self.ui_px(36.0 * level as f32 / 100.0))
                                .h_full()
                                .bg(rgb(theme.role.text_subtle)),
                        ),
                )
            })
            .child(div().flex_1().min_w_0().child(self.voice.status().unwrap_or_default()))
            .children(buttons.into_iter().map(|(id, label, action)| {
                div()
                    .id(id)
                    .flex_none()
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .bg(rgb(theme.role.control_bg))
                    .cursor(gpui::CursorStyle::PointingHand)
                    .hover(move |style| style.bg(rgb(theme.role.control_bg_hover)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.voice_action(action, cx);
                        cx.stop_propagation();
                    }))
                    .child(label)
            }))
            .when(!self.voice.active(), |bar| {
                bar.child(
                    div()
                        .id("voice-dismiss")
                        .px_2()
                        .cursor(gpui::CursorStyle::PointingHand)
                        .child("Dismiss")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.voice.message = None;
                            cx.notify();
                        })),
                )
            })
    }
}
