use gpui::{ClipboardItem, Context, Window};
use lst_editor::{EditorEffect, EditorTab as ModelEditorTab};
use rfd::FileDialog;
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    process,
    time::Duration,
};

use crate::LstGpuiApp;

#[derive(Clone, Debug)]
struct AutosaveJob {
    path: PathBuf,
    body: String,
    revision: u64,
}

#[derive(Debug, PartialEq, Eq)]
struct OpenFileResults {
    opened: Vec<(PathBuf, String)>,
    failed: Vec<(PathBuf, String)>,
}

#[derive(Debug, PartialEq, Eq)]
enum SaveFileResult {
    Saved(PathBuf),
    Failed { path: PathBuf, message: String },
}

#[derive(Debug, PartialEq, Eq)]
enum AutosaveCompletion {
    Finished { path: PathBuf, revision: u64 },
    Failed { path: PathBuf, message: String },
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
                    if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                        self.update_model(cx, true, |model| {
                            model.paste_text(text);
                        });
                    } else {
                        self.update_model(cx, true, |model| {
                            model.clipboard_unavailable();
                        });
                    }
                }
                EditorEffect::OpenFiles => self.open_files_from_dialog(cx),
                EditorEffect::SaveFile { path, body } => {
                    self.apply_save_file_result(save_file_result(path, body), cx);
                }
                EditorEffect::SaveFileAs {
                    suggested_name,
                    body,
                } => {
                    let Some(path) = FileDialog::new().set_file_name(&suggested_name).save_file()
                    else {
                        continue;
                    };
                    self.apply_save_file_result(save_file_result(path, body), cx);
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
        self.apply_open_file_results(open_file_results(paths), cx);
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
                        view.update_model(cx, false, |model| {
                            model.autosave_tick();
                        });
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
        if !can_start_autosave_job(self.model.tabs(), &self.autosave_inflight, &path, revision) {
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
        if let Some(completion) = autosave_completion(self.model.tabs(), job, result) {
            self.apply_autosave_completion(completion, cx);
        } else {
            cx.notify();
        }
    }

    fn apply_open_file_results(&mut self, results: OpenFileResults, cx: &mut Context<Self>) {
        for (path, message) in results.failed {
            self.update_model(cx, true, |model| {
                model.open_file_failed(path, message);
            });
        }
        if !results.opened.is_empty() {
            self.update_model(cx, true, |model| {
                model.open_files(results.opened);
            });
        }
    }

    fn apply_save_file_result(&mut self, result: SaveFileResult, cx: &mut Context<Self>) {
        self.update_model(cx, true, |model| match result {
            SaveFileResult::Saved(path) => model.save_finished(path),
            SaveFileResult::Failed { path, message } => model.save_failed(path, message),
        });
    }

    fn apply_autosave_completion(
        &mut self,
        completion: AutosaveCompletion,
        cx: &mut Context<Self>,
    ) {
        self.update_model(cx, true, |model| match completion {
            AutosaveCompletion::Finished { path, revision } => {
                model.autosave_finished(path, revision);
            }
            AutosaveCompletion::Failed { path, message } => {
                model.autosave_failed(path, message);
            }
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

fn open_file_results(paths: impl IntoIterator<Item = PathBuf>) -> OpenFileResults {
    let mut opened = Vec::new();
    let mut failed = Vec::new();
    for path in paths {
        match fs::read_to_string(&path) {
            Ok(text) => opened.push((path, text)),
            Err(err) => failed.push((path, err.to_string())),
        }
    }
    OpenFileResults { opened, failed }
}

fn save_file_result(path: PathBuf, body: String) -> SaveFileResult {
    match fs::write(&path, body) {
        Ok(()) => SaveFileResult::Saved(path),
        Err(err) => SaveFileResult::Failed {
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

fn autosave_completion(
    tabs: &[ModelEditorTab],
    job: AutosaveJob,
    result: std::io::Result<PathBuf>,
) -> Option<AutosaveCompletion> {
    let temp_path = match result {
        Ok(temp_path) => temp_path,
        Err(err) => {
            return Some(AutosaveCompletion::Failed {
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
        Ok(()) => Some(AutosaveCompletion::Finished {
            path: job.path,
            revision: job.revision,
        }),
        Err(err) => {
            let _ = fs::remove_file(&temp_path);
            Some(AutosaveCompletion::Failed {
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
        if tab.path().map(PathBuf::as_path) != Some(path) {
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
    fn open_file_results_read_existing_files_and_report_failures() {
        let dir = temp_dir("open");
        let ok = dir.join("ok.txt");
        let missing = dir.join("missing.txt");
        fs::write(&ok, "hello").expect("write open fixture");

        let results = open_file_results([ok.clone(), missing.clone()]);

        assert_eq!(results.opened, vec![(ok, "hello".to_string())]);
        assert_eq!(results.failed.len(), 1);
        assert_eq!(results.failed[0].0, missing);
        assert!(!results.failed[0].1.is_empty());

        fs::remove_dir_all(dir).expect("remove test temp dir");
    }

    #[test]
    fn save_file_result_writes_body_and_reports_result() {
        let dir = temp_dir("save");
        let path = dir.join("saved.txt");

        let result = save_file_result(path.clone(), "saved body".to_string());

        assert_eq!(result, SaveFileResult::Saved(path.clone()));
        assert_eq!(
            fs::read_to_string(&path).expect("read saved file"),
            "saved body"
        );

        fs::remove_dir_all(dir).expect("remove test temp dir");
    }

    #[test]
    fn save_file_result_reports_write_failures() {
        let dir = temp_dir("save-failure");

        let result = save_file_result(dir.clone(), "cannot replace directory".to_string());

        match result {
            SaveFileResult::Failed { path, message } => {
                assert_eq!(path, dir.clone());
                assert!(!message.is_empty());
            }
            other => panic!("expected save failure result, got {other:?}"),
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
        let completion = autosave_completion(&[tab], job, Ok(temp_path));

        assert_eq!(
            completion,
            Some(AutosaveCompletion::Finished {
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
        let completion = autosave_completion(&[tab], job, Ok(temp_path.clone()));

        assert_eq!(completion, None);
        assert!(!temp_path.exists());
        assert_eq!(
            fs::read_to_string(&path).expect("read destination file"),
            "old"
        );

        fs::remove_dir_all(dir).expect("remove test temp dir");
    }
}
