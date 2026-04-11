use gpui::{ClipboardItem, Context, Window};
use lst_editor::{EditorCommand, EditorEffect, EditorTab as ModelEditorTab};
use rfd::FileDialog;
use std::{
    fs,
    path::{Path, PathBuf},
    process,
    time::{Duration, Instant},
};

use crate::launch::{AutoBench, BenchAction};
use crate::{bench_trace, elapsed_ms, LstGpuiApp};

#[derive(Clone, Debug)]
struct AutosaveJob {
    path: PathBuf,
    body: String,
    revision: u64,
}

impl LstGpuiApp {
    pub(crate) fn handle_model_effects(
        &mut self,
        effects: Vec<EditorEffect>,
        cx: &mut Context<Self>,
    ) {
        for effect in effects {
            match effect {
                EditorEffect::Focus(target) => self.queue_focus(target),
                EditorEffect::RevealCursor => self.reveal_active_cursor(),
                EditorEffect::WriteClipboard(text) => {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
                EditorEffect::WritePrimary(text) => {
                    cx.write_to_primary(ClipboardItem::new_string(text));
                }
                EditorEffect::ReadClipboard => {
                    let read_started = Instant::now();
                    if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                        let clipboard_read_ms = elapsed_ms(read_started);
                        let apply_started = Instant::now();
                        self.apply_model_command(EditorCommand::PasteText(text), cx);
                        self.record_operation(
                            "paste_clipboard",
                            Some(clipboard_read_ms),
                            elapsed_ms(apply_started),
                        );
                    } else {
                        self.model.status =
                            "Clipboard does not currently contain plain text.".to_string();
                    }
                }
                EditorEffect::OpenFiles => self.open_files_from_dialog(cx),
                EditorEffect::SaveFile { path, body } => {
                    let command = match fs::write(&path, body) {
                        Ok(()) => EditorCommand::SaveFinished { path },
                        Err(err) => EditorCommand::SaveFailed {
                            path,
                            message: err.to_string(),
                        },
                    };
                    self.apply_model_command(command, cx);
                }
                EditorEffect::SaveFileAs {
                    suggested_name,
                    body,
                } => {
                    let Some(path) = FileDialog::new().set_file_name(&suggested_name).save_file()
                    else {
                        continue;
                    };
                    let command = match fs::write(&path, body) {
                        Ok(()) => EditorCommand::SaveFinished { path },
                        Err(err) => EditorCommand::SaveFailed {
                            path,
                            message: err.to_string(),
                        },
                    };
                    self.apply_model_command(command, cx);
                }
                EditorEffect::AutosaveFile {
                    path,
                    body,
                    revision,
                } => self.start_autosave_job(path, body, revision, cx),
            }
        }
    }

    fn open_files_from_dialog(&mut self, cx: &mut Context<Self>) {
        let Some(paths) = FileDialog::new().pick_files() else {
            return;
        };
        let mut opened = Vec::new();
        for path in paths {
            match fs::read_to_string(&path) {
                Ok(text) => opened.push((path, text)),
                Err(err) => self.apply_model_command(
                    EditorCommand::OpenFileFailed {
                        path,
                        message: err.to_string(),
                    },
                    cx,
                ),
            }
        }
        if !opened.is_empty() {
            self.apply_model_command(EditorCommand::OpenFiles(opened), cx);
        }
    }

    pub(crate) fn start_background_tasks(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.autosave_started {
            return;
        }
        self.autosave_started = true;
        let view = cx.entity();
        window
            .spawn(cx, async move |cx| loop {
                cx.background_executor()
                    .timer(Duration::from_millis(500))
                    .await;
                if view
                    .update(cx, |view, cx| {
                        view.apply_model_command(EditorCommand::AutosaveTick, cx);
                    })
                    .is_err()
                {
                    break;
                }
            })
            .detach();
    }

    fn start_autosave_job(
        &mut self,
        path: PathBuf,
        body: String,
        revision: u64,
        cx: &mut Context<Self>,
    ) {
        if self.autosave_inflight.contains(&path)
            || !autosave_revision_is_current(&self.model.tabs, &path, revision)
        {
            return;
        }

        let job = AutosaveJob {
            path,
            body,
            revision,
        };
        self.autosave_inflight.insert(job.path.clone());
        cx.spawn({
            let job = job.clone();
            async move |this, cx| {
                let temp_path = autosave_temp_path(&job.path, job.revision);
                let body = job.body.clone();
                let result = cx
                    .background_executor()
                    .spawn(async move { fs::write(&temp_path, &body).map(|_| temp_path) })
                    .await;
                let _ = this.update(cx, |view, cx| view.finish_autosave(job, result, cx));
            }
        })
        .detach();
    }

    fn finish_autosave(
        &mut self,
        job: AutosaveJob,
        result: std::io::Result<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        self.autosave_inflight.remove(&job.path);
        match result {
            Ok(temp_path) => {
                if !autosave_revision_is_current(&self.model.tabs, &job.path, job.revision) {
                    let _ = fs::remove_file(&temp_path);
                    cx.notify();
                    return;
                }

                match fs::rename(&temp_path, &job.path) {
                    Ok(()) => {
                        self.apply_model_command(
                            EditorCommand::AutosaveFinished {
                                path: job.path,
                                revision: job.revision,
                            },
                            cx,
                        );
                    }
                    Err(err) => {
                        let _ = fs::remove_file(&temp_path);
                        self.apply_model_command(
                            EditorCommand::AutosaveFailed {
                                path: job.path,
                                message: err.to_string(),
                            },
                            cx,
                        );
                    }
                }
            }
            Err(err) => {
                self.apply_model_command(
                    EditorCommand::AutosaveFailed {
                        path: job.path,
                        message: err.to_string(),
                    },
                    cx,
                );
            }
        }
    }

    pub(crate) fn record_operation(
        &mut self,
        label: &'static str,
        clipboard_read_ms: Option<f64>,
        apply_ms: f64,
    ) {
        let tab = self.active_tab();
        self.last_operation = crate::OperationStats {
            label,
            bytes: tab.buffer.len_bytes(),
            lines: tab.buffer.len_lines(),
            clipboard_read_ms,
            apply_ms,
        };
        bench_trace::record_operation(
            label,
            self.last_operation.bytes,
            self.last_operation.lines,
            clipboard_read_ms,
            apply_ms,
        );
        eprintln!("lst_gpui {}", self.last_operation.summary());
    }

    fn replace_active_text(
        &mut self,
        label: &'static str,
        text: &str,
        clipboard_read_ms: Option<f64>,
        cx: &mut Context<Self>,
    ) {
        let apply_started = Instant::now();
        let old_show_wrap = self.model.show_wrap;
        {
            let tab = self.model.active_tab_mut();
            let id = tab.id();
            let name_hint = tab.display_name();
            *tab = ModelEditorTab::from_text(id, name_hint, None, text);
        }
        if !self.model.find.query.is_empty() {
            self.model.reindex_find_matches_to_nearest();
        }
        self.sync_tab_views(old_show_wrap);
        if let Some(view) = self.tab_views.get_mut(self.model.active) {
            view.invalidate_visual_state();
        }
        self.record_operation(label, clipboard_read_ms, elapsed_ms(apply_started));
        self.model.status = format!("Loaded {} lines.", self.active_tab().line_count());
        self.reveal_active_cursor();
        cx.notify();
    }

    fn append_active_text(
        &mut self,
        label: &'static str,
        text: &str,
        clipboard_read_ms: Option<f64>,
        cx: &mut Context<Self>,
    ) {
        let apply_started = Instant::now();
        let old_show_wrap = self.model.show_wrap;
        {
            let tab = self.model.active_tab_mut();
            let end = tab.len_chars();
            tab.replace_char_range(end..end, text);
            tab.modified = false;
        }
        if !self.model.find.query.is_empty() {
            self.model.reindex_find_matches_to_nearest();
        }
        self.sync_tab_views(old_show_wrap);
        self.record_operation(label, clipboard_read_ms, elapsed_ms(apply_started));
        self.model.status = format!("Appended {} lines.", text.lines().count());
        self.reveal_active_cursor();
        cx.notify();
    }

    pub(crate) fn run_auto_bench(
        &mut self,
        bench: AutoBench,
        window: &mut Window,
        cx: &mut Context<Self>,
        startup_to_action_ms: f64,
        process_started: Instant,
    ) {
        let action_started = Instant::now();

        match bench.action {
            BenchAction::Replace => {
                self.replace_active_text(bench.action.operation_label(), &bench.text, None, cx)
            }
            BenchAction::Append => {
                self.append_active_text(bench.action.operation_label(), &bench.text, None, cx)
            }
        }

        let operation = self.last_operation.clone();
        let action = bench.action;
        let source = bench.source;

        window.on_next_frame(move |_window, cx| {
            eprintln!(
                "lst_gpui bench action={} source={} startup_to_action_ms={startup_to_action_ms:.3} action_to_next_frame_ms={:.3} total_wall_ms={:.3} final_bytes={} final_lines={} apply_ms={:.3}",
                action.action_name(),
                source,
                elapsed_ms(action_started),
                elapsed_ms(process_started),
                operation.bytes,
                operation.lines,
                operation.apply_ms,
            );
            cx.quit();
        });
    }
}

fn autosave_temp_path(path: &Path, revision: u64) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("buffer");
    path.with_file_name(format!(
        ".{file_name}.lst-gpui-autosave-{}-{revision}.tmp",
        process::id()
    ))
}

pub(crate) fn autosave_revision_is_current(
    tabs: &[ModelEditorTab],
    path: &PathBuf,
    revision: u64,
) -> bool {
    let mut matched: Option<u64> = None;
    for tab in tabs {
        if tab.path.as_ref() != Some(path) {
            continue;
        }
        if matched.is_some() {
            return false;
        }
        matched = Some(tab.revision());
    }
    matched == Some(revision)
}
