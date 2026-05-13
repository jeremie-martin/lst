use gpui::{ClipboardItem, Context, Window};
use lst_editor::{EditorEffect, EditorTab as ModelEditorTab, FileStamp, TabCloseRequest, TabId};
use rfd::{FileDialog, MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
use std::{
    collections::HashSet,
    env,
    ffi::{OsStr, OsString},
    fs, io,
    io::Write,
    path::{Path, PathBuf},
    process,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, MutexGuard,
    },
    time::{Duration, Instant},
};

use crate::{elapsed_ms, LstGpuiApp, PendingAfterSave};
use lst_editor::UndoBoundary;
use std::ops::Range;

mod scratchpad;

pub(crate) use scratchpad::create_scratchpad_note;
#[cfg(test)]
use scratchpad::create_scratchpad_note_with_timestamp;
use scratchpad::{
    remove_previous_scratchpad_after_save_as, remove_scratchpad_file_if_unreferenced,
};

#[derive(Clone, Debug)]
struct AutosaveJob {
    tab_id: TabId,
    path: PathBuf,
    body: String,
    revision: u64,
    expected_stamp: Option<FileStamp>,
}

#[derive(Clone)]
struct SaveTicket {
    generation: u64,
    current_generation: Arc<Mutex<u64>>,
}

impl SaveTicket {
    fn issue(current_generation: &Arc<Mutex<u64>>) -> Self {
        let generation = {
            let mut current = lock_generation(current_generation);
            *current += 1;
            *current
        };
        Self {
            generation,
            current_generation: current_generation.clone(),
        }
    }

    #[cfg(test)]
    fn current_for_test() -> Self {
        let current_generation = Arc::new(Mutex::new(1));
        Self {
            generation: 1,
            current_generation,
        }
    }

    fn is_current(&self) -> bool {
        *lock_generation(&self.current_generation) == self.generation
    }

    fn current_guard(&self) -> Option<MutexGuard<'_, u64>> {
        let guard = lock_generation(&self.current_generation);
        if *guard == self.generation {
            Some(guard)
        } else {
            None
        }
    }
}

fn lock_generation(generation: &Mutex<u64>) -> MutexGuard<'_, u64> {
    generation
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Debug, PartialEq, Eq)]
struct OpenFileResults {
    opened: Vec<(PathBuf, String, Option<FileStamp>)>,
    failed: Vec<(PathBuf, String)>,
}

#[derive(Debug, PartialEq, Eq)]
enum SaveFileResult {
    Saved {
        tab_id: TabId,
        path: PathBuf,
        revision: u64,
        stamp: FileStamp,
        body: String,
    },
    Failed {
        tab_id: TabId,
        path: PathBuf,
        message: String,
    },
    Conflict {
        tab_id: TabId,
        path: PathBuf,
        revision: u64,
        disk_stamp: FileStamp,
    },
    Stale {
        tab_id: TabId,
        path: PathBuf,
        revision: u64,
    },
}

fn save_file_result_path(result: &SaveFileResult) -> &Path {
    match result {
        SaveFileResult::Saved { path, .. }
        | SaveFileResult::Failed { path, .. }
        | SaveFileResult::Conflict { path, .. }
        | SaveFileResult::Stale { path, .. } => path,
    }
}

#[derive(Debug, PartialEq, Eq)]
enum AutosaveCompletion {
    Finished {
        tab_id: TabId,
        path: PathBuf,
        revision: u64,
        stamp: FileStamp,
        body: String,
    },
    Failed {
        tab_id: TabId,
        path: PathBuf,
        message: String,
    },
    Conflict {
        tab_id: TabId,
        path: PathBuf,
        revision: u64,
        disk_stamp: FileStamp,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FileConflictDecision {
    Reload,
    Overwrite,
    Cancel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ConflictWrite {
    Save { revision: u64 },
    Autosave { revision: u64 },
}

impl LstGpuiApp {
    pub(crate) fn handle_model_effects(
        &mut self,
        effects: Vec<EditorEffect>,
        cx: &mut Context<Self>,
    ) {
        for effect in effects {
            match effect {
                EditorEffect::Focus(target) => self.set_focus(target),
                EditorEffect::Reveal(intent) => self.queue_cursor_reveal(intent),
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
                        self.update_model(cx, true, |model| {
                            model.paste_text(text);
                        });
                        self.record_operation(
                            "paste_clipboard",
                            Some(clipboard_read_ms),
                            elapsed_ms(apply_started),
                        );
                    } else {
                        self.update_model(cx, true, |model| {
                            model.clipboard_unavailable();
                        });
                    }
                }
                EditorEffect::OpenFiles => self.open_files_from_dialog(cx),
                EditorEffect::SaveFile {
                    tab_id,
                    path,
                    body,
                    revision,
                    expected_stamp,
                } => self.spawn_save_job(tab_id, path, body, revision, expected_stamp, None, cx),
                EditorEffect::SaveFileAs {
                    tab_id,
                    suggested_name,
                    body,
                    revision,
                    previous_scratchpad_path,
                } => {
                    let Some(path) = FileDialog::new().set_file_name(&suggested_name).save_file()
                    else {
                        self.save_cancelled(tab_id, cx);
                        continue;
                    };
                    self.spawn_save_job(
                        tab_id,
                        path,
                        body,
                        revision,
                        None,
                        Some(previous_scratchpad_path),
                        cx,
                    );
                }
                EditorEffect::AutosaveFile {
                    tab_id,
                    path,
                    body,
                    revision,
                    expected_stamp,
                } => self.start_autosave_job(tab_id, path, body, revision, expected_stamp, cx),
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
                        view.check_external_file_changes(cx);
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
        tab_id: TabId,
        path: PathBuf,
        body: String,
        revision: u64,
        expected_stamp: Option<FileStamp>,
        cx: &mut Context<Self>,
    ) {
        if !can_start_autosave_job(
            self.model.tabs(),
            &self.autosave_inflight,
            tab_id,
            &path,
            revision,
        ) {
            return;
        }
        match file_conflict_stamp(&path, expected_stamp) {
            Ok(Some(disk_stamp))
                if self
                    .model
                    .tab_by_id(tab_id)
                    .is_some_and(|tab| tab.conflict_suppressed_for(disk_stamp)) =>
            {
                return;
            }
            Ok(Some(disk_stamp)) => {
                self.handle_file_conflict(
                    tab_id,
                    path,
                    disk_stamp,
                    ConflictWrite::Autosave { revision },
                    cx,
                );
                return;
            }
            Ok(None) => {}
            Err(err) => {
                self.apply_autosave_completion(
                    AutosaveCompletion::Failed {
                        tab_id,
                        path,
                        message: err.to_string(),
                    },
                    cx,
                );
                return;
            }
        }

        let job = AutosaveJob {
            tab_id,
            path,
            body,
            revision,
            expected_stamp,
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

    fn spawn_save_job(
        &mut self,
        tab_id: TabId,
        path: PathBuf,
        body: String,
        revision: u64,
        expected_stamp: Option<FileStamp>,
        save_as_previous_scratchpad: Option<Option<PathBuf>>,
        cx: &mut Context<Self>,
    ) {
        let ticket = self.issue_save_ticket(&path);
        self.begin_save_inflight(&path);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    save_file_result(tab_id, path, body, revision, expected_stamp, ticket)
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.apply_save_outcome(result, save_as_previous_scratchpad, cx);
            });
        })
        .detach();
    }

    fn issue_save_ticket(&mut self, path: &Path) -> SaveTicket {
        let current_generation = self
            .save_ticket_generations
            .entry(path.to_path_buf())
            .or_insert_with(|| Arc::new(Mutex::new(0)));
        SaveTicket::issue(current_generation)
    }

    fn begin_save_inflight(&mut self, path: &Path) {
        *self.save_inflight.entry(path.to_path_buf()).or_insert(0) += 1;
    }

    fn finish_save_inflight(&mut self, path: &Path) {
        let Some(count) = self.save_inflight.get_mut(path) else {
            return;
        };
        *count = count.saturating_sub(1);
        if *count == 0 {
            self.save_inflight.remove(path);
        }
    }

    pub(crate) fn check_external_file_changes(&mut self, cx: &mut Context<Self>) {
        let requests = self
            .model
            .tabs()
            .iter()
            .filter_map(|tab| {
                Some((
                    tab.id(),
                    tab.path()?.clone(),
                    tab.file_stamp()?,
                    tab.modified(),
                ))
            })
            .collect::<Vec<_>>();

        for (tab_id, path, expected_stamp, modified) in requests {
            if self.save_inflight.contains_key(&path) {
                continue;
            }
            let Ok(disk_stamp) = file_stamp(&path) else {
                continue;
            };
            if disk_stamp == expected_stamp {
                continue;
            }
            if modified {
                if self
                    .model
                    .tab_by_id(tab_id)
                    .is_some_and(|tab| tab.conflict_suppressed_for(disk_stamp))
                {
                    continue;
                }
                let Some(tab) = self.model.tab_by_id(tab_id) else {
                    continue;
                };
                self.handle_file_conflict(
                    tab_id,
                    path,
                    disk_stamp,
                    ConflictWrite::Autosave {
                        revision: tab.revision(),
                    },
                    cx,
                );
                break;
            }
            self.refresh_or_reload_clean_tab_from_path(tab_id, path, disk_stamp, cx);
        }
    }

    fn handle_file_conflict(
        &mut self,
        tab_id: TabId,
        path: PathBuf,
        disk_stamp: FileStamp,
        write: ConflictWrite,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.model.tab_by_id(tab_id) else {
            return;
        };
        if !tab.modified() {
            self.refresh_or_reload_clean_tab_from_path(tab_id, path, disk_stamp, cx);
            self.finish_pending_after_save(tab_id, true, cx);
            return;
        }
        let revision = match write {
            ConflictWrite::Save { revision } | ConflictWrite::Autosave { revision } => revision,
        };
        if tab.revision() != revision {
            cx.notify();
            return;
        }
        let body = tab.buffer_text();
        let saved_body = saved_body_for_conflict(write, &body);
        if fs::read_to_string(&path).is_ok_and(|text| text == saved_body.as_str()) {
            self.finish_matching_disk_conflict(tab_id, path, disk_stamp, saved_body, write, cx);
            return;
        }

        match prompt_file_conflict_decision(&tab.display_name()) {
            FileConflictDecision::Reload => {
                self.reload_tab_from_path(tab_id, path, cx);
                self.finish_pending_after_save(tab_id, true, cx);
            }
            FileConflictDecision::Overwrite => match write {
                ConflictWrite::Save { revision } => {
                    self.spawn_save_job(tab_id, path, body, revision, None, None, cx);
                }
                ConflictWrite::Autosave { revision } => {
                    self.apply_autosave_completion(
                        write_autosave_body_result(tab_id, path, body, revision, None),
                        cx,
                    );
                }
            },
            FileConflictDecision::Cancel => {
                self.update_model(cx, true, |model| {
                    model.suppress_file_conflict(tab_id, path, disk_stamp);
                });
                self.finish_pending_after_save(tab_id, false, cx);
            }
        }
    }

    fn finish_matching_disk_conflict(
        &mut self,
        tab_id: TabId,
        path: PathBuf,
        disk_stamp: FileStamp,
        saved_body: String,
        write: ConflictWrite,
        cx: &mut Context<Self>,
    ) {
        match write {
            ConflictWrite::Save { revision } => {
                let recent_path = path.clone();
                let mut saved = false;
                let mut record_recent = false;
                self.update_model(cx, true, |model| {
                    saved =
                        model.save_finished_for_tab(tab_id, path, revision, disk_stamp, saved_body);
                    record_recent = saved
                        && model
                            .tab_by_id(tab_id)
                            .is_some_and(|tab| !tab.is_scratchpad());
                });
                if record_recent {
                    self.recent.record(&recent_path);
                }
                self.finish_pending_after_save(tab_id, saved, cx);
            }
            ConflictWrite::Autosave { revision } => {
                self.apply_autosave_completion(
                    AutosaveCompletion::Finished {
                        tab_id,
                        path,
                        revision,
                        stamp: disk_stamp,
                        body: saved_body,
                    },
                    cx,
                );
            }
        }
    }

    fn reload_tab_from_path(&mut self, tab_id: TabId, path: PathBuf, cx: &mut Context<Self>) {
        match read_file_with_stamp(&path) {
            Ok((text, stamp)) => {
                self.update_model(cx, true, |model| {
                    model.reload_tab_from_disk(tab_id, path, text, stamp);
                });
            }
            Err(err) => {
                self.update_model(cx, true, |model| {
                    model.reload_failed(path, err.to_string());
                });
            }
        }
    }

    fn refresh_or_reload_clean_tab_from_path(
        &mut self,
        tab_id: TabId,
        path: PathBuf,
        disk_stamp: FileStamp,
        cx: &mut Context<Self>,
    ) {
        match fs::read_to_string(&path) {
            Ok(text)
                if self
                    .model
                    .tab_by_id(tab_id)
                    .is_some_and(|tab| tab.buffer_text() == text) =>
            {
                self.update_model(cx, true, |model| {
                    model.refresh_file_stamp_for_tab(tab_id, path, disk_stamp);
                });
            }
            Ok(text) => {
                self.update_model(cx, true, |model| {
                    model.reload_tab_from_disk(tab_id, path, text, disk_stamp);
                });
            }
            Err(err) => {
                self.update_model(cx, true, |model| {
                    model.reload_failed(path, err.to_string());
                });
            }
        }
    }

    pub(crate) fn request_close_active_tab(&mut self, cx: &mut Context<Self>) {
        let index = self.model.active_index();
        self.request_close_tab_at(index, cx);
    }

    pub(crate) fn request_close_tab_at(&mut self, index: usize, cx: &mut Context<Self>) {
        self.hovered_tab = None;
        if self.tab_is_empty_scratchpad(index) {
            self.cleanup_scratchpad_tab_file(index);
            if self.model.tab_count() == 1 && index == self.model.active_index() {
                self.request_quit(cx);
                return;
            }
            if let Some(tab_id) = self.model.tab_id_at(index) {
                self.update_model(cx, true, |model| {
                    model.discard_close_tab(tab_id);
                });
            }
            return;
        }
        if self.model.tab_count() == 1 && index == self.model.active_index() {
            self.request_quit(cx);
            return;
        }
        let Some(tab_id) = self.model.tab_id_at(index) else {
            return;
        };
        match self.model.close_request_for_tab(tab_id) {
            Some(TabCloseRequest::Close { tab_id }) => {
                self.record_closed_tab(tab_id);
                self.update_model(cx, true, |model| {
                    model.close_clean_tab(tab_id);
                });
            }
            Some(TabCloseRequest::SaveAndClose { tab_id }) => {
                self.start_save_for_pending(tab_id, PendingAfterSave::CloseTab(tab_id), cx);
            }
            None => {}
        }
    }

    pub(crate) fn request_quit(&mut self, cx: &mut Context<Self>) {
        self.continue_quit_sequence(cx);
    }

    fn continue_quit_sequence(&mut self, cx: &mut Context<Self>) {
        let Some(index) = self.first_dirty_tab_index_for_quit() else {
            self.finish_quit(cx);
            return;
        };
        let Some(tab_id) = self.model.tab_id_at(index) else {
            self.finish_quit(cx);
            return;
        };
        let Some(TabCloseRequest::SaveAndClose { tab_id }) =
            self.model.close_request_for_tab(tab_id)
        else {
            self.finish_quit(cx);
            return;
        };
        self.start_save_for_pending(tab_id, PendingAfterSave::Quit, cx);
    }

    fn start_save_for_pending(
        &mut self,
        tab_id: TabId,
        pending: PendingAfterSave,
        cx: &mut Context<Self>,
    ) {
        self.pending_after_save = Some(pending);
        self.update_model(cx, true, |model| {
            model.request_save_tab(tab_id);
        });
    }

    fn first_dirty_tab_index_for_quit(&self) -> Option<usize> {
        self.model
            .tabs()
            .iter()
            .position(|tab| tab.modified() && !(tab.is_scratchpad() && tab.is_blank()))
    }

    fn finish_quit(&mut self, cx: &mut Context<Self>) {
        self.cleanup_empty_scratchpad_files();
        // X11 WM_DELETE_WINDOW already holds GPUI's X11 client RefCell, so defer
        // exit until the current frame releases it. Production shutdown calls
        // `process::exit`; tests route through GPUI's `quit` so the harness can
        // observe shutdown.
        #[cfg(test)]
        cx.defer(|app| app.quit());
        #[cfg(not(test))]
        cx.defer(|_| process::exit(0));
    }

    fn save_cancelled(&mut self, tab_id: TabId, cx: &mut Context<Self>) {
        self.finish_pending_after_save(tab_id, false, cx);
    }

    fn finish_pending_after_save(&mut self, tab_id: TabId, success: bool, cx: &mut Context<Self>) {
        let Some(pending) = self.pending_after_save else {
            return;
        };
        match pending {
            PendingAfterSave::CloseTab(pending_tab_id) if pending_tab_id == tab_id => {
                self.pending_after_save = None;
                if success {
                    self.record_closed_tab(tab_id);
                    self.update_model(cx, true, |model| {
                        model.close_clean_tab(tab_id);
                    });
                }
            }
            PendingAfterSave::Quit => {
                self.pending_after_save = None;
                if success {
                    self.continue_quit_sequence(cx);
                }
            }
            _ => {}
        }
    }

    fn apply_open_file_results(&mut self, results: OpenFileResults, cx: &mut Context<Self>) {
        for (path, message) in results.failed {
            self.update_model(cx, true, |model| {
                model.open_file_failed(path, message);
            });
        }
        if !results.opened.is_empty() {
            let opened_paths = results
                .opened
                .iter()
                .map(|(path, _, _)| path.clone())
                .collect::<Vec<_>>();
            self.update_model(cx, true, |model| {
                model.open_files_with_stamps(results.opened);
            });
            for path in opened_paths {
                self.recent.record(&path);
            }
        }
    }

    fn apply_save_outcome(
        &mut self,
        result: SaveFileResult,
        save_as_previous_scratchpad: Option<Option<PathBuf>>,
        cx: &mut Context<Self>,
    ) {
        let inflight_path = save_file_result_path(&result).to_path_buf();
        self.finish_save_inflight(&inflight_path);
        let is_save_as = save_as_previous_scratchpad.is_some();
        match result {
            SaveFileResult::Saved {
                tab_id,
                path,
                revision,
                stamp,
                body,
            } => {
                let recent_path = path.clone();
                let saved_path = path.clone();
                let mut saved = false;
                let mut record_recent = false;
                self.update_model(cx, true, |model| {
                    saved = if is_save_as {
                        model.save_as_finished_for_tab(tab_id, path, revision, stamp, body)
                    } else {
                        model.save_finished_for_tab(tab_id, path, revision, stamp, body)
                    };
                    record_recent = saved
                        && (is_save_as
                            || model
                                .tab_by_id(tab_id)
                                .is_some_and(|tab| !tab.is_scratchpad()));
                });
                if record_recent {
                    self.recent.record(&recent_path);
                }
                if let Some(previous_scratchpad_path) = save_as_previous_scratchpad {
                    if saved {
                        remove_previous_scratchpad_after_save_as(
                            previous_scratchpad_path,
                            &saved_path,
                            self.model.tabs(),
                        );
                    }
                }
                self.finish_pending_after_save(tab_id, saved, cx);
            }
            SaveFileResult::Failed {
                tab_id,
                path,
                message,
            } => {
                self.update_model(cx, true, |model| {
                    model.save_failed(path, message);
                });
                self.finish_pending_after_save(tab_id, false, cx);
            }
            SaveFileResult::Conflict {
                tab_id,
                path,
                revision,
                disk_stamp,
            } => {
                self.handle_file_conflict(
                    tab_id,
                    path,
                    disk_stamp,
                    ConflictWrite::Save { revision },
                    cx,
                );
            }
            SaveFileResult::Stale { .. } => {
                cx.notify();
            }
        }
    }

    fn apply_autosave_completion(
        &mut self,
        completion: AutosaveCompletion,
        cx: &mut Context<Self>,
    ) {
        match completion {
            AutosaveCompletion::Finished {
                tab_id,
                path,
                revision,
                stamp,
                body,
            } => {
                let recent_path = path.clone();
                let mut record_recent = false;
                self.update_model(cx, true, |model| {
                    record_recent = model
                        .autosave_finished_for_tab(tab_id, path, revision, stamp, body)
                        && model
                            .tab_by_id(tab_id)
                            .is_some_and(|tab| !tab.is_scratchpad());
                });
                if record_recent {
                    self.recent.record(&recent_path);
                }
            }
            AutosaveCompletion::Failed {
                tab_id: _,
                path,
                message,
            } => {
                self.update_model(cx, true, |model| {
                    model.autosave_failed(path, message);
                });
            }
            AutosaveCompletion::Conflict {
                tab_id,
                path,
                revision,
                disk_stamp,
            } => self.handle_file_conflict(
                tab_id,
                path,
                disk_stamp,
                ConflictWrite::Autosave { revision },
                cx,
            ),
        }
    }

    pub(crate) fn request_new_tab(&mut self, cx: &mut Context<Self>) {
        match create_scratchpad_note(self.scratchpad_dir_override()) {
            Ok((path, file_stamp)) => {
                self.update_model(cx, true, |model| {
                    model.new_scratchpad_tab(path, file_stamp);
                });
            }
            Err(err) => {
                self.update_model(cx, true, |model| {
                    model.new_tab();
                    model.save_failed(PathBuf::from("scratchpad"), err.to_string());
                });
            }
        }
    }

    fn scratchpad_dir_override(&self) -> Option<&Path> {
        self.scratchpad_dir.as_deref().or_else(|| {
            self.model
                .tabs()
                .iter()
                .find_map(ModelEditorTab::scratchpad_path)
                .and_then(|path| path.parent())
        })
    }

    fn tab_is_empty_scratchpad(&self, index: usize) -> bool {
        self.model
            .tab(index)
            .is_some_and(|tab| tab.is_scratchpad() && tab.is_blank())
    }

    fn cleanup_scratchpad_tab_file(&self, index: usize) {
        if let Some(tab) = self
            .model
            .tab(index)
            .filter(|tab| tab.is_scratchpad() && tab.is_blank())
        {
            if let Some(path) = tab.path() {
                remove_scratchpad_file_if_unreferenced(self.model.tabs(), tab.id(), path);
            }
        }
    }

    fn cleanup_empty_scratchpad_files(&self) {
        for tab in self.model.tabs() {
            if tab.is_scratchpad() && tab.is_blank() {
                if let Some(path) = tab.path() {
                    remove_scratchpad_file_if_unreferenced(self.model.tabs(), tab.id(), path);
                }
            }
        }
    }

    /// Snapshot a closing tab so `Ctrl+Shift+T` can reopen it. Scratchpads
    /// and untitled tabs are excluded — there is no path to reopen.
    fn record_closed_tab(&mut self, tab_id: TabId) {
        const MAX_CLOSED_HISTORY: usize = 32;
        let Some(tab) = self.model.tab_by_id(tab_id) else {
            return;
        };
        if tab.is_scratchpad() {
            return;
        }
        let Some(path) = tab.path().cloned() else {
            return;
        };
        self.closed_tabs_history.push(crate::ClosedTabRecord {
            path,
            position: tab.cursor_position(),
        });
        if self.closed_tabs_history.len() > MAX_CLOSED_HISTORY {
            let overflow = self.closed_tabs_history.len() - MAX_CLOSED_HISTORY;
            self.closed_tabs_history.drain(..overflow);
        }
    }

    /// Pop the most recently closed tab and reopen it, or focus the
    /// existing tab when the path is already open.
    pub(crate) fn reopen_recently_closed_tab(&mut self, cx: &mut Context<Self>) {
        let Some(record) = self.closed_tabs_history.pop() else {
            return;
        };
        let line = record.position.line;
        let column = record.position.column;
        let existing = self
            .model
            .tabs()
            .iter()
            .find(|tab| tab.path() == Some(&record.path))
            .map(ModelEditorTab::id);
        if let Some(tab_id) = existing {
            self.update_model(cx, true, |model| {
                model.set_active_tab(tab_id);
                model.set_active_cursor_position(line, column);
            });
            return;
        }
        match read_file_with_stamp(&record.path) {
            Ok((text, stamp)) => {
                let opened_path = record.path.clone();
                self.update_model(cx, true, |model| {
                    model.open_files_with_stamps(vec![(opened_path, text, Some(stamp))]);
                    model.set_active_cursor_position(line, column);
                });
                self.recent.record(&record.path);
            }
            Err(err) => {
                self.update_model(cx, true, |model| {
                    model.open_file_failed(record.path, err.to_string());
                });
            }
        }
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AtomicWriteOutcome {
    Written,
    Stale,
    Conflict(FileStamp),
}

fn write_file_with_guards(
    path: &Path,
    bytes: &[u8],
    expected_stamp: Option<FileStamp>,
    ticket: Option<&SaveTicket>,
) -> io::Result<AtomicWriteOutcome> {
    if ticket.is_some_and(|ticket| !ticket.is_current()) {
        return Ok(AtomicWriteOutcome::Stale);
    }
    ensure_existing_target_is_writable(path)?;

    if let Some(disk_stamp) = file_conflict_stamp(path, expected_stamp)? {
        return Ok(AtomicWriteOutcome::Conflict(disk_stamp));
    }

    let write_target = write_target_for_path(path)?;
    let permissions = fs::metadata(path)
        .ok()
        .map(|metadata| metadata.permissions());
    let temp_path = write_temp_replacement(&write_target, bytes, permissions)?;

    let _current = match ticket {
        Some(ticket) => match ticket.current_guard() {
            Some(guard) => Some(guard),
            None => {
                remove_temp_file(&temp_path);
                return Ok(AtomicWriteOutcome::Stale);
            }
        },
        None => None,
    };

    if let Some(disk_stamp) = file_conflict_stamp(path, expected_stamp)? {
        remove_temp_file(&temp_path);
        return Ok(AtomicWriteOutcome::Conflict(disk_stamp));
    }

    if let Err(err) = fs::rename(&temp_path, &write_target) {
        remove_temp_file(&temp_path);
        return Err(err);
    }
    Ok(AtomicWriteOutcome::Written)
}

fn ensure_existing_target_is_writable(path: &Path) -> io::Result<()> {
    match fs::metadata(path) {
        Ok(metadata) => {
            if metadata.is_dir() {
                return Err(io::Error::new(
                    io::ErrorKind::IsADirectory,
                    "cannot save over a directory",
                ));
            }
            if !metadata.is_file() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "cannot save over a non-file target",
                ));
            }
            if metadata.permissions().readonly() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "refusing to replace a read-only file",
                ));
            }
            Ok(())
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

fn write_target_for_path(path: &Path) -> io::Result<PathBuf> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => match fs::canonicalize(path) {
            Ok(target) => Ok(target),
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                fs::read_link(path).map(|target| resolve_link_target(path, target))
            }
            Err(err) => Err(err),
        },
        Ok(_) => Ok(path.to_path_buf()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(path.to_path_buf()),
        Err(err) => Err(err),
    }
}

fn resolve_link_target(link_path: &Path, target: PathBuf) -> PathBuf {
    if target.is_absolute() {
        return target;
    }
    link_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(target)
}

fn write_temp_replacement(
    target: &Path,
    bytes: &[u8],
    permissions: Option<fs::Permissions>,
) -> io::Result<PathBuf> {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    for _ in 0..128 {
        let temp_path = replacement_temp_path(parent, target.file_name());
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
        {
            Ok(mut file) => {
                let result = (|| {
                    file.write_all(bytes)?;
                    if let Some(permissions) = permissions.clone() {
                        file.set_permissions(permissions)?;
                    }
                    file.sync_all()
                })();
                drop(file);
                match result {
                    Ok(()) => return Ok(temp_path),
                    Err(err) => {
                        remove_temp_file(&temp_path);
                        return Err(err);
                    }
                }
            }
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
            Err(err) => return Err(err),
        }
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not create a unique temporary save file",
    ))
}

fn replacement_temp_path(parent: &Path, file_name: Option<&OsStr>) -> PathBuf {
    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    let mut name = OsString::from(".");
    name.push(file_name.unwrap_or_else(|| OsStr::new("buffer")));
    name.push(format!(
        ".lst-gpui-save-{}-{}.tmp",
        process::id(),
        TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    parent.join(name)
}

fn remove_temp_file(path: &Path) {
    let _ = fs::remove_file(path);
}

fn stale_save_result(tab_id: TabId, path: PathBuf, revision: u64) -> SaveFileResult {
    SaveFileResult::Stale {
        tab_id,
        path,
        revision,
    }
}

fn saved_body_for_conflict(write: ConflictWrite, body: &str) -> String {
    match write {
        ConflictWrite::Save { .. } => apply_save_options(body.to_string()),
        ConflictWrite::Autosave { .. } => body.to_string(),
    }
}

fn open_file_results(paths: impl IntoIterator<Item = PathBuf>) -> OpenFileResults {
    let mut opened = Vec::new();
    let mut failed = Vec::new();
    for path in paths {
        match read_file_with_stamp(&path) {
            Ok((text, stamp)) => opened.push((path, text, Some(stamp))),
            Err(err) => failed.push((path, err.to_string())),
        }
    }
    OpenFileResults { opened, failed }
}

fn save_file_result(
    tab_id: TabId,
    path: PathBuf,
    body: String,
    revision: u64,
    expected_stamp: Option<FileStamp>,
    ticket: SaveTicket,
) -> SaveFileResult {
    if !ticket.is_current() {
        return stale_save_result(tab_id, path, revision);
    }

    match file_conflict_stamp(&path, expected_stamp) {
        Ok(Some(disk_stamp)) => {
            return SaveFileResult::Conflict {
                tab_id,
                path,
                revision,
                disk_stamp,
            };
        }
        Ok(None) => {}
        Err(err) => {
            return SaveFileResult::Failed {
                tab_id,
                path,
                message: err.to_string(),
            };
        }
    }
    if !ticket.is_current() {
        return stale_save_result(tab_id, path, revision);
    }
    write_file_result(tab_id, path, body, revision, expected_stamp, ticket)
}

fn write_file_result(
    tab_id: TabId,
    path: PathBuf,
    body: String,
    revision: u64,
    expected_stamp: Option<FileStamp>,
    ticket: SaveTicket,
) -> SaveFileResult {
    let body = apply_save_options(body);
    match write_file_with_guards(&path, body.as_bytes(), expected_stamp, Some(&ticket)) {
        Ok(AtomicWriteOutcome::Written) => match file_stamp(&path) {
            Ok(stamp) => SaveFileResult::Saved {
                tab_id,
                path,
                revision,
                stamp,
                body,
            },
            Err(err) => SaveFileResult::Failed {
                tab_id,
                path,
                message: err.to_string(),
            },
        },
        Ok(AtomicWriteOutcome::Stale) => stale_save_result(tab_id, path, revision),
        Ok(AtomicWriteOutcome::Conflict(disk_stamp)) => SaveFileResult::Conflict {
            tab_id,
            path,
            revision,
            disk_stamp,
        },
        Err(err) => SaveFileResult::Failed {
            tab_id,
            path,
            message: err.to_string(),
        },
    }
}

fn write_autosave_body_result(
    tab_id: TabId,
    path: PathBuf,
    body: String,
    revision: u64,
    expected_stamp: Option<FileStamp>,
) -> AutosaveCompletion {
    match write_file_with_guards(&path, body.as_bytes(), expected_stamp, None) {
        Ok(AtomicWriteOutcome::Written) => match file_stamp(&path) {
            Ok(stamp) => AutosaveCompletion::Finished {
                tab_id,
                path,
                revision,
                stamp,
                body,
            },
            Err(err) => AutosaveCompletion::Failed {
                tab_id,
                path,
                message: err.to_string(),
            },
        },
        Ok(AtomicWriteOutcome::Stale) => unreachable!("autosave writes are never ticket-gated"),
        Ok(AtomicWriteOutcome::Conflict(disk_stamp)) => AutosaveCompletion::Conflict {
            tab_id,
            path,
            revision,
            disk_stamp,
        },
        Err(err) => AutosaveCompletion::Failed {
            tab_id,
            path,
            message: err.to_string(),
        },
    }
}

pub(crate) fn read_file_with_stamp(path: &Path) -> std::io::Result<(String, FileStamp)> {
    let before = file_stamp(path)?;
    let text = fs::read_to_string(path)?;
    let after = file_stamp(path)?;
    if before == after {
        return Ok((text, after));
    }

    let text = fs::read_to_string(path)?;
    let stamp = file_stamp(path)?;
    Ok((text, stamp))
}

fn can_start_autosave_job(
    tabs: &[ModelEditorTab],
    inflight: &HashSet<PathBuf>,
    tab_id: TabId,
    path: &Path,
    revision: u64,
) -> bool {
    !inflight.contains(path) && autosave_revision_is_current(tabs, tab_id, path, revision)
}

fn write_autosave_temp_file(job: &AutosaveJob) -> std::io::Result<PathBuf> {
    let temp_path = autosave_temp_path(&job.path, job.revision);
    fs::write(&temp_path, job.body.as_bytes()).map(|_| temp_path)
}

/// Opt-in save-time text policies driven by env flags
/// (`LST_SAVE_TRIM_TRAILING_WS`, `LST_SAVE_ENSURE_FINAL_NEWLINE`). They live
/// as env vars in the spirit of `LST_LLM_FAKE_RESPONSE` until a real
/// settings surface lands.
fn apply_save_options(body: String) -> String {
    let trim = env_flag("LST_SAVE_TRIM_TRAILING_WS");
    let ensure_newline = env_flag("LST_SAVE_ENSURE_FINAL_NEWLINE");
    if !trim && !ensure_newline {
        return body;
    }
    let mut body = if trim { trim_trailing_ws(&body) } else { body };
    if ensure_newline && !body.is_empty() && !body.ends_with('\n') {
        body.push('\n');
    }
    body
}

fn env_flag(name: &str) -> bool {
    env::var_os(name).is_some_and(|value| value == "1")
}

fn trim_trailing_ws(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut segments = body.split('\n');
    if let Some(first) = segments.next() {
        out.push_str(first.trim_end_matches([' ', '\t']));
    }
    for segment in segments {
        out.push('\n');
        out.push_str(segment.trim_end_matches([' ', '\t']));
    }
    out
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
                tab_id: job.tab_id,
                path: job.path,
                message: err.to_string(),
            });
        }
    };

    if !autosave_revision_is_current(tabs, job.tab_id, &job.path, job.revision) {
        let _ = fs::remove_file(&temp_path);
        return None;
    }

    let _ = fs::remove_file(&temp_path);
    Some(write_autosave_body_result(
        job.tab_id,
        job.path,
        job.body,
        job.revision,
        job.expected_stamp,
    ))
}

pub(crate) fn autosave_revision_is_current(
    tabs: &[ModelEditorTab],
    tab_id: TabId,
    path: &Path,
    revision: u64,
) -> bool {
    let open_tabs_for_path = tabs
        .iter()
        .filter(|tab| tab.path().map(PathBuf::as_path) == Some(path))
        .take(2)
        .count();
    if open_tabs_for_path != 1 {
        return false;
    }
    for tab in tabs {
        if tab.id() == tab_id {
            return tab.path().map(PathBuf::as_path) == Some(path) && tab.revision() == revision;
        }
    }
    false
}

fn file_stamp(path: &Path) -> std::io::Result<FileStamp> {
    fs::metadata(path).map(|metadata| FileStamp::from_metadata(&metadata))
}

fn file_conflict_stamp(
    path: &Path,
    expected_stamp: Option<FileStamp>,
) -> std::io::Result<Option<FileStamp>> {
    let Some(expected_stamp) = expected_stamp else {
        return Ok(None);
    };
    let disk_stamp = match file_stamp(path) {
        Ok(stamp) => stamp,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            // Deleted backing files have no current stamp; reuse the last known
            // stamp as a stable conflict key so Save can offer to recreate them.
            return Ok(Some(expected_stamp));
        }
        Err(err) => return Err(err),
    };
    Ok((disk_stamp != expected_stamp).then_some(disk_stamp))
}

fn prompt_file_conflict_decision(title: &str) -> FileConflictDecision {
    match MessageDialog::new()
        .set_level(MessageLevel::Warning)
        .set_title("File changed on disk")
        .set_description(format!(
            "{title} changed outside lst. Reload from disk, overwrite it, or keep editing?"
        ))
        .set_buttons(MessageButtons::YesNoCancelCustom(
            "Reload".to_string(),
            "Overwrite".to_string(),
            "Cancel".to_string(),
        ))
        .show()
    {
        MessageDialogResult::Custom(label) if label == "Reload" => FileConflictDecision::Reload,
        MessageDialogResult::Custom(label) if label == "Overwrite" => {
            FileConflictDecision::Overwrite
        }
        MessageDialogResult::Yes => FileConflictDecision::Reload,
        MessageDialogResult::No => FileConflictDecision::Overwrite,
        _ => FileConflictDecision::Cancel,
    }
}

impl LstGpuiApp {
    pub(crate) fn start_cleanup(&mut self, cx: &mut Context<Self>) {
        if self.cleanup_in_flight {
            return;
        }

        let client = match build_llm_client() {
            Ok(client) => client,
            Err(message) => {
                self.cleanup_message = Some(message);
                cx.notify();
                return;
            }
        };

        let tab = self.active_tab();
        let tab_id = tab.id();
        let revision = tab.revision();
        let (range, source_text) = if tab.has_selection() {
            let range = tab.selected_range();
            match tab.selected_text() {
                Some(text) if !text.is_empty() => (range, text),
                _ => {
                    self.cleanup_message = Some("Nothing to clean up.".to_string());
                    cx.notify();
                    return;
                }
            }
        } else {
            let text = tab.buffer_text();
            if text.is_empty() {
                self.cleanup_message = Some("Nothing to clean up.".to_string());
                cx.notify();
                return;
            }
            (0..tab.buffer().len_chars(), text)
        };

        self.cleanup_in_flight = true;
        self.cleanup_message = Some("\u{27F3} Cleaning\u{2026}".to_string());
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { client.cleanup(&source_text) })
                .await;
            let _ = this.update(cx, |app, cx| {
                app.cleanup_in_flight = false;
                match result {
                    Ok(cleaned) => app.apply_cleanup_result(tab_id, revision, range, cleaned, cx),
                    Err(err) => app.finish_cleanup_with_error(err, cx),
                }
            });
        })
        .detach();
    }

    fn apply_cleanup_result(
        &mut self,
        tab_id: TabId,
        revision: u64,
        range: Range<usize>,
        cleaned: String,
        cx: &mut Context<Self>,
    ) {
        let stale = match self.model.tab_by_id(tab_id) {
            Some(tab) => self.model.active_tab_id() != tab_id || tab.revision() != revision,
            None => true,
        };
        if stale {
            self.cleanup_message =
                Some("Buffer changed during cleanup; result discarded.".to_string());
            cx.notify();
            return;
        }

        self.update_model(cx, true, |model| {
            model.replace_text(Some(range), cleaned, UndoBoundary::Break);
        });
    }

    fn finish_cleanup_with_error(&mut self, err: crate::llm::LlmError, cx: &mut Context<Self>) {
        self.cleanup_message = Some(format!("Cleanup failed: {err}"));
        cx.notify();
    }
}

fn build_llm_client() -> Result<Box<dyn crate::llm::LlmClient>, String> {
    if let Some(canned) = std::env::var("LST_LLM_FAKE_RESPONSE")
        .ok()
        .filter(|s| !s.is_empty())
    {
        let delay_ms = std::env::var("LST_LLM_FAKE_DELAY_MS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        return Ok(Box::new(crate::llm::FakeLlmClient::new(
            canned,
            Duration::from_millis(delay_ms),
        )));
    }

    let api_key = match std::env::var("DEEPSEEK_API_KEY") {
        Ok(value) if !value.is_empty() => value,
        _ => return Err("DEEPSEEK_API_KEY not set".to_string()),
    };
    let model_name = std::env::var("DEEPSEEK_MODEL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| crate::llm::DEFAULT_DEEPSEEK_MODEL.to_string());
    Ok(Box::new(crate::llm::DeepSeekClient::new(
        api_key, model_name,
    )))
}

#[cfg(test)]
mod tests;
