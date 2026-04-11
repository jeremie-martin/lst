use gpui::{ClipboardItem, Context, Window};
use lst_editor::{EditorCommand, EditorEffect, EditorTab as ModelEditorTab};
use rfd::FileDialog;
use std::{
    collections::HashSet,
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
                    self.apply_model_command(save_file_command(path, body), cx);
                }
                EditorEffect::SaveFileAs {
                    suggested_name,
                    body,
                } => {
                    let Some(path) = FileDialog::new().set_file_name(&suggested_name).save_file()
                    else {
                        continue;
                    };
                    self.apply_model_command(save_file_command(path, body), cx);
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
        for command in open_file_commands(paths) {
            self.apply_model_command(command, cx);
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
        if !can_start_autosave_job(&self.model.tabs, &self.autosave_inflight, &path, revision) {
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
                let write_job = job.clone();
                let result = cx
                    .background_executor()
                    .spawn(async move { write_autosave_temp_file(&write_job) })
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
        if let Some(command) = autosave_completion_command(&self.model.tabs, job, result) {
            self.apply_model_command(command, cx);
        } else {
            cx.notify();
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

fn open_file_commands(paths: impl IntoIterator<Item = PathBuf>) -> Vec<EditorCommand> {
    let mut commands = Vec::new();
    let mut opened = Vec::new();
    for path in paths {
        match fs::read_to_string(&path) {
            Ok(text) => opened.push((path, text)),
            Err(err) => commands.push(EditorCommand::OpenFileFailed {
                path,
                message: err.to_string(),
            }),
        }
    }
    if !opened.is_empty() {
        commands.push(EditorCommand::OpenFiles(opened));
    }
    commands
}

fn save_file_command(path: PathBuf, body: String) -> EditorCommand {
    match fs::write(&path, body) {
        Ok(()) => EditorCommand::SaveFinished { path },
        Err(err) => EditorCommand::SaveFailed {
            path,
            message: err.to_string(),
        },
    }
}

fn can_start_autosave_job(
    tabs: &[ModelEditorTab],
    inflight: &HashSet<PathBuf>,
    path: &Path,
    revision: u64,
) -> bool {
    !inflight.contains(path) && autosave_revision_is_current(tabs, path, revision)
}

fn write_autosave_temp_file(job: &AutosaveJob) -> std::io::Result<PathBuf> {
    let temp_path = autosave_temp_path(&job.path, job.revision);
    fs::write(&temp_path, &job.body).map(|_| temp_path)
}

fn autosave_completion_command(
    tabs: &[ModelEditorTab],
    job: AutosaveJob,
    result: std::io::Result<PathBuf>,
) -> Option<EditorCommand> {
    let temp_path = match result {
        Ok(temp_path) => temp_path,
        Err(err) => {
            return Some(EditorCommand::AutosaveFailed {
                path: job.path,
                message: err.to_string(),
            });
        }
    };

    if !autosave_revision_is_current(tabs, &job.path, job.revision) {
        let _ = fs::remove_file(&temp_path);
        return None;
    }

    match fs::rename(&temp_path, &job.path) {
        Ok(()) => Some(EditorCommand::AutosaveFinished {
            path: job.path,
            revision: job.revision,
        }),
        Err(err) => {
            let _ = fs::remove_file(&temp_path);
            Some(EditorCommand::AutosaveFailed {
                path: job.path,
                message: err.to_string(),
            })
        }
    }
}

pub(crate) fn autosave_revision_is_current(
    tabs: &[ModelEditorTab],
    path: &Path,
    revision: u64,
) -> bool {
    let mut matched: Option<u64> = None;
    for tab in tabs {
        if tab.path.as_deref() != Some(path) {
            continue;
        }
        if matched.is_some() {
            return false;
        }
        matched = Some(tab.revision());
    }
    matched == Some(revision)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lst_editor::TabId;
    use std::{
        collections::HashSet,
        sync::atomic::{AtomicUsize, Ordering},
    };

    static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);

    fn temp_dir(label: &str) -> PathBuf {
        let id = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("lst-gpui-runtime-{label}-{}-{id}", process::id()));
        fs::create_dir(&dir).expect("create test temp dir");
        dir
    }

    fn tab_for_path(path: PathBuf, text: &str) -> ModelEditorTab {
        ModelEditorTab::from_path(TabId::from_raw(1), path, text)
    }

    #[test]
    fn open_file_commands_read_existing_files_and_report_failures() {
        let dir = temp_dir("open");
        let ok = dir.join("ok.txt");
        let missing = dir.join("missing.txt");
        fs::write(&ok, "hello").expect("write open fixture");

        let commands = open_file_commands([ok.clone(), missing.clone()]);

        assert_eq!(commands.len(), 2);
        match &commands[0] {
            EditorCommand::OpenFileFailed { path, message } => {
                assert_eq!(path, &missing);
                assert!(!message.is_empty());
            }
            other => panic!("expected open failure command, got {other:?}"),
        }
        assert_eq!(
            commands[1],
            EditorCommand::OpenFiles(vec![(ok, "hello".to_string())])
        );

        fs::remove_dir_all(dir).expect("remove test temp dir");
    }

    #[test]
    fn save_file_command_writes_body_and_reports_result() {
        let dir = temp_dir("save");
        let path = dir.join("saved.txt");

        let command = save_file_command(path.clone(), "saved body".to_string());

        assert_eq!(command, EditorCommand::SaveFinished { path: path.clone() });
        assert_eq!(
            fs::read_to_string(&path).expect("read saved file"),
            "saved body"
        );

        fs::remove_dir_all(dir).expect("remove test temp dir");
    }

    #[test]
    fn save_file_command_reports_write_failures() {
        let dir = temp_dir("save-failure");

        let command = save_file_command(dir.clone(), "cannot replace directory".to_string());

        match command {
            EditorCommand::SaveFailed { path, message } => {
                assert_eq!(path, dir.clone());
                assert!(!message.is_empty());
            }
            other => panic!("expected save failure command, got {other:?}"),
        }

        fs::remove_dir_all(dir).expect("remove test temp dir");
    }

    #[test]
    fn can_start_autosave_job_requires_current_revision_and_no_inflight_write() {
        let dir = temp_dir("autosave-start");
        let path = dir.join("note.txt");
        let tab = tab_for_path(path.clone(), "old");
        let mut inflight = HashSet::new();

        assert!(can_start_autosave_job(&[tab.clone()], &inflight, &path, 0));

        inflight.insert(path.clone());
        assert!(!can_start_autosave_job(&[tab.clone()], &inflight, &path, 0));

        let mut stale = tab;
        stale.replace_char_range(0..0, "new ");
        assert!(!can_start_autosave_job(&[stale], &HashSet::new(), &path, 0));

        fs::remove_dir_all(dir).expect("remove test temp dir");
    }

    #[test]
    fn autosave_completion_commits_current_revision() {
        let dir = temp_dir("autosave-commit");
        let path = dir.join("note.txt");
        fs::write(&path, "old").expect("write autosave destination");
        let tab = tab_for_path(path.clone(), "old");
        let job = AutosaveJob {
            path: path.clone(),
            body: "new".to_string(),
            revision: 0,
        };

        let temp_path = write_autosave_temp_file(&job).expect("write autosave temp file");
        let command = autosave_completion_command(&[tab], job, Ok(temp_path));

        assert_eq!(
            command,
            Some(EditorCommand::AutosaveFinished {
                path: path.clone(),
                revision: 0
            })
        );
        assert_eq!(
            fs::read_to_string(&path).expect("read autosaved file"),
            "new"
        );

        fs::remove_dir_all(dir).expect("remove test temp dir");
    }

    #[test]
    fn autosave_completion_discards_stale_temp_without_command() {
        let dir = temp_dir("autosave-stale");
        let path = dir.join("note.txt");
        fs::write(&path, "old").expect("write autosave destination");
        let mut tab = tab_for_path(path.clone(), "old");
        tab.replace_char_range(0..0, "current ");
        let job = AutosaveJob {
            path: path.clone(),
            body: "stale".to_string(),
            revision: 0,
        };

        let temp_path = write_autosave_temp_file(&job).expect("write autosave temp file");
        let command = autosave_completion_command(&[tab], job, Ok(temp_path.clone()));

        assert_eq!(command, None);
        assert!(!temp_path.exists());
        assert_eq!(
            fs::read_to_string(&path).expect("read destination file"),
            "old"
        );

        fs::remove_dir_all(dir).expect("remove test temp dir");
    }
}
