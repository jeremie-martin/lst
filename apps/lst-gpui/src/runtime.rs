use gpui::{ClipboardItem, Context, Window};
use lst_editor::{EditorEffect, EditorTab as ModelEditorTab, FileStamp, TabCloseRequest, TabId};
use rfd::{FileDialog, MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
use std::{
    collections::HashSet,
    env, fs, io,
    path::{Path, PathBuf},
    process,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, MutexGuard,
    },
    time::{Duration, Instant},
};

use crate::{elapsed_ms, LstGpuiApp, PendingAfterSave};

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
        body: String,
        revision: u64,
        disk_stamp: FileStamp,
    },
    Stale {
        tab_id: TabId,
        path: PathBuf,
        revision: u64,
    },
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
        body: String,
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
                } => self.start_save_file_job(tab_id, path, body, revision, expected_stamp, cx),
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
                    self.start_save_as_file_job(
                        tab_id,
                        path,
                        body,
                        revision,
                        previous_scratchpad_path,
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
                    body,
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

    fn start_save_file_job(
        &mut self,
        tab_id: TabId,
        path: PathBuf,
        body: String,
        revision: u64,
        expected_stamp: Option<FileStamp>,
        cx: &mut Context<Self>,
    ) {
        let ticket = self.issue_save_ticket(&path);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    save_file_result(tab_id, path, body, revision, expected_stamp, ticket)
                })
                .await;
            let _ = this.update(cx, |view, cx| view.apply_save_file_result(result, cx));
        })
        .detach();
    }

    fn start_save_as_file_job(
        &mut self,
        tab_id: TabId,
        path: PathBuf,
        body: String,
        revision: u64,
        previous_scratchpad_path: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        let ticket = self.issue_save_ticket(&path);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { save_file_result(tab_id, path, body, revision, None, ticket) })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.apply_save_as_file_result(result, previous_scratchpad_path, cx);
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
                    tab.buffer_text(),
                    disk_stamp,
                    ConflictWrite::Autosave {
                        revision: tab.revision(),
                    },
                    cx,
                );
                break;
            }
            self.reload_tab_from_path(tab_id, path, cx);
        }
    }

    fn handle_file_conflict(
        &mut self,
        tab_id: TabId,
        path: PathBuf,
        body: String,
        disk_stamp: FileStamp,
        write: ConflictWrite,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.model.tab_by_id(tab_id) else {
            return;
        };
        if !tab.modified() {
            self.reload_tab_from_path(tab_id, path, cx);
            self.finish_pending_after_save(tab_id, true, cx);
            return;
        }

        match prompt_file_conflict_decision(&tab.display_name()) {
            FileConflictDecision::Reload => {
                self.reload_tab_from_path(tab_id, path, cx);
                self.finish_pending_after_save(tab_id, true, cx);
            }
            FileConflictDecision::Overwrite => match write {
                ConflictWrite::Save { revision } => {
                    self.start_save_file_job(tab_id, path, body, revision, None, cx);
                }
                ConflictWrite::Autosave { revision } => {
                    self.apply_autosave_completion(
                        write_autosave_body_result(tab_id, path, body, revision),
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

    fn apply_save_file_result(&mut self, result: SaveFileResult, cx: &mut Context<Self>) {
        match result {
            SaveFileResult::Saved {
                tab_id,
                path,
                revision,
                stamp,
                body,
            } => {
                let recent_path = path.clone();
                let mut saved = false;
                self.update_model(cx, true, |model| {
                    saved = model.save_finished_for_tab(tab_id, path, revision, stamp, body);
                });
                if saved {
                    self.recent.record(&recent_path);
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
                body,
                revision,
                disk_stamp,
            } => {
                self.handle_file_conflict(
                    tab_id,
                    path,
                    body,
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

    fn apply_save_as_file_result(
        &mut self,
        result: SaveFileResult,
        previous_scratchpad_path: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        match result {
            SaveFileResult::Saved {
                tab_id,
                path,
                revision,
                stamp,
                body,
            } => {
                let recent_path = path.clone();
                let mut saved = false;
                self.update_model(cx, true, |model| {
                    saved =
                        model.save_as_finished_for_tab(tab_id, path.clone(), revision, stamp, body);
                });
                if saved {
                    self.recent.record(&recent_path);
                    remove_previous_scratchpad_after_save_as(
                        previous_scratchpad_path,
                        &path,
                        self.model.tabs(),
                    );
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
                body,
                revision,
                disk_stamp,
            } => {
                self.handle_file_conflict(
                    tab_id,
                    path,
                    body,
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
                self.update_model(cx, true, |model| {
                    model.autosave_finished_for_tab(tab_id, path, revision, stamp, body);
                });
                self.recent.record(&recent_path);
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
                body,
                revision,
                disk_stamp,
            } => self.handle_file_conflict(
                tab_id,
                path,
                body,
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

fn save_temp_path(path: &Path) -> PathBuf {
    static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);
    let temp_id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("buffer");
    path.with_file_name(format!(
        ".{file_name}.lst-gpui-save-{}-{temp_id}.tmp",
        process::id()
    ))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AtomicWriteOutcome {
    Written,
    Stale,
}

fn write_file_atomically(
    path: &Path,
    bytes: &[u8],
    ticket: Option<&SaveTicket>,
) -> io::Result<AtomicWriteOutcome> {
    let temp_path = save_temp_path(path);
    let result = (|| {
        if ticket.is_some_and(|ticket| !ticket.is_current()) {
            return Ok(AtomicWriteOutcome::Stale);
        }

        fs::write(&temp_path, bytes)?;
        copy_target_permissions_to_temp(path, &temp_path)?;

        match ticket {
            Some(ticket) => {
                let Some(_current) = ticket.current_guard() else {
                    return Ok(AtomicWriteOutcome::Stale);
                };
                fs::rename(&temp_path, path)?;
            }
            None => fs::rename(&temp_path, path)?,
        }
        Ok(AtomicWriteOutcome::Written)
    })();
    match result {
        Ok(AtomicWriteOutcome::Written) => Ok(AtomicWriteOutcome::Written),
        Ok(AtomicWriteOutcome::Stale) => {
            let _ = fs::remove_file(&temp_path);
            Ok(AtomicWriteOutcome::Stale)
        }
        Err(err) => {
            let _ = fs::remove_file(&temp_path);
            Err(err)
        }
    }
}

fn copy_target_permissions_to_temp(target: &Path, temp_path: &Path) -> io::Result<()> {
    match fs::metadata(target) {
        Ok(metadata) => fs::set_permissions(temp_path, metadata.permissions()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

fn stale_save_result(tab_id: TabId, path: PathBuf, revision: u64) -> SaveFileResult {
    SaveFileResult::Stale {
        tab_id,
        path,
        revision,
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
                body,
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
    write_file_result(tab_id, path, body, revision, ticket)
}

fn write_file_result(
    tab_id: TabId,
    path: PathBuf,
    body: String,
    revision: u64,
    ticket: SaveTicket,
) -> SaveFileResult {
    let body = apply_save_options(body);
    match write_file_atomically(&path, body.as_bytes(), Some(&ticket)) {
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
) -> AutosaveCompletion {
    match write_file_atomically(&path, body.as_bytes(), None) {
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

    match file_conflict_stamp(&job.path, job.expected_stamp) {
        Ok(Some(disk_stamp)) => {
            let _ = fs::remove_file(&temp_path);
            return Some(AutosaveCompletion::Conflict {
                tab_id: job.tab_id,
                path: job.path,
                body: job.body,
                revision: job.revision,
                disk_stamp,
            });
        }
        Ok(None) => {}
        Err(err) => {
            let _ = fs::remove_file(&temp_path);
            return Some(AutosaveCompletion::Failed {
                tab_id: job.tab_id,
                path: job.path,
                message: err.to_string(),
            });
        }
    }

    if let Err(err) = copy_target_permissions_to_temp(&job.path, &temp_path) {
        let _ = fs::remove_file(&temp_path);
        return Some(AutosaveCompletion::Failed {
            tab_id: job.tab_id,
            path: job.path,
            message: err.to_string(),
        });
    }

    match fs::rename(&temp_path, &job.path) {
        Ok(()) => match file_stamp(&job.path) {
            Ok(stamp) => Some(AutosaveCompletion::Finished {
                tab_id: job.tab_id,
                path: job.path,
                revision: job.revision,
                stamp,
                body: job.body,
            }),
            Err(err) => Some(AutosaveCompletion::Failed {
                tab_id: job.tab_id,
                path: job.path,
                message: err.to_string(),
            }),
        },
        Err(err) => {
            let _ = fs::remove_file(&temp_path);
            Some(AutosaveCompletion::Failed {
                tab_id: job.tab_id,
                path: job.path,
                message: err.to_string(),
            })
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use lst_editor::{EditorModel, TabId, UndoBoundary};
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
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
        ModelEditorTab::from_path_with_stamp(
            TabId::from_raw(1),
            path,
            text,
            Some(FileStamp::from_raw(0, Some(0))),
        )
    }

    fn tab_for_path_with_id(id: u64, path: PathBuf, text: &str) -> ModelEditorTab {
        ModelEditorTab::from_path_with_stamp(
            TabId::from_raw(id),
            path,
            text,
            Some(FileStamp::from_raw(0, Some(0))),
        )
    }

    fn scratchpad_for_path_with_id(id: u64, path: PathBuf) -> ModelEditorTab {
        ModelEditorTab::scratchpad_with_stamp(
            TabId::from_raw(id),
            path,
            FileStamp::from_raw(0, Some(0)),
        )
    }

    #[test]
    fn scratchpad_note_creation_uses_timestamped_names_and_collision_suffixes() {
        let dir = temp_dir("scratchpad");
        let timestamp = "2026-04-11_12-13-14".to_string();

        let (first, first_stamp) =
            create_scratchpad_note_with_timestamp(Some(&dir), timestamp.clone())
                .expect("create first scratchpad");
        let (second, second_stamp) = create_scratchpad_note_with_timestamp(Some(&dir), timestamp)
            .expect("create second scratchpad");

        assert_eq!(
            first.file_name().and_then(|name| name.to_str()),
            Some("2026-04-11_12-13-14.md")
        );
        assert_eq!(
            second.file_name().and_then(|name| name.to_str()),
            Some("2026-04-11_12-13-14_1.md")
        );
        assert_eq!(fs::read_to_string(&first).expect("read first"), "");
        assert_eq!(first_stamp, file_stamp(&first).expect("first stamp"));
        assert_eq!(second_stamp, file_stamp(&second).expect("second stamp"));

        fs::remove_dir_all(dir).expect("remove test temp dir");
    }

    #[test]
    fn successful_save_as_removes_previous_scratchpad_file_only_when_path_changes() {
        let dir = temp_dir("scratchpad-save-as");
        let old = dir.join("2026-04-11_12-13-14.md");
        let same = old.clone();
        let new = dir.join("saved.md");
        fs::write(&old, "").expect("write old scratchpad");

        remove_previous_scratchpad_after_save_as(Some(old.clone()), &same, &[]);
        assert!(old.exists());

        remove_previous_scratchpad_after_save_as(Some(old.clone()), &new, &[]);
        assert!(!old.exists());

        fs::remove_dir_all(dir).expect("remove test temp dir");
    }

    #[test]
    fn successful_save_as_keeps_target_when_path_spelling_changes() {
        let dir = temp_dir("scratchpad-save-as-same-file");
        let nested = dir.join("nested");
        fs::create_dir(&nested).expect("create nested test dir");
        let saved = dir.join("saved.md");
        let same_file = nested.join("..").join("saved.md");
        fs::write(&saved, "saved body").expect("write saved file");

        assert_ne!(same_file, saved);
        remove_previous_scratchpad_after_save_as(Some(same_file), &saved, &[]);

        assert_eq!(
            fs::read_to_string(&saved).expect("read saved file"),
            "saved body"
        );

        fs::remove_dir_all(dir).expect("remove test temp dir");
    }

    #[test]
    fn successful_save_as_keeps_source_when_another_tab_still_uses_it() {
        let dir = temp_dir("scratchpad-save-as-source-open");
        let old = dir.join("2026-04-11_12-13-14.md");
        let new = dir.join("saved.md");
        fs::write(&old, "shared scratchpad").expect("write old scratchpad");
        fs::write(&new, "saved body").expect("write saved file");
        let open_tabs = vec![tab_for_path_with_id(2, old.clone(), "other tab")];

        remove_previous_scratchpad_after_save_as(Some(old.clone()), &new, &open_tabs);

        assert_eq!(
            fs::read_to_string(&old).expect("read old scratchpad"),
            "shared scratchpad"
        );

        fs::remove_dir_all(dir).expect("remove test temp dir");
    }

    #[test]
    fn empty_scratchpad_cleanup_keeps_files_open_in_another_tab() {
        let dir = temp_dir("scratchpad-cleanup-shared");
        let path = dir.join("2026-04-11_12-13-14.md");
        fs::write(&path, "other tab content").expect("write scratchpad");
        let open_tabs = vec![
            scratchpad_for_path_with_id(1, path.clone()),
            tab_for_path_with_id(2, path.clone(), "other tab content"),
        ];

        remove_scratchpad_file_if_unreferenced(&open_tabs, TabId::from_raw(1), &path);

        assert_eq!(
            fs::read_to_string(&path).expect("read shared scratchpad"),
            "other tab content"
        );

        fs::remove_dir_all(dir).expect("remove test temp dir");
    }

    #[test]
    fn open_file_results_read_existing_files_and_report_failures() {
        let dir = temp_dir("open");
        let ok = dir.join("ok.txt");
        let missing = dir.join("missing.txt");
        fs::write(&ok, "hello").expect("write open fixture");

        let results = open_file_results([ok.clone(), missing.clone()]);

        assert_eq!(results.opened.len(), 1);
        assert_eq!(results.opened[0].0, ok);
        assert_eq!(results.opened[0].1, "hello");
        assert!(results.opened[0].2.is_some());
        assert_eq!(results.failed.len(), 1);
        assert_eq!(results.failed[0].0, missing);
        assert!(!results.failed[0].1.is_empty());

        fs::remove_dir_all(dir).expect("remove test temp dir");
    }

    #[test]
    fn save_file_result_writes_body_and_reports_result() {
        let dir = temp_dir("save");
        let path = dir.join("saved.txt");

        let tab_id = TabId::from_raw(1);
        let result = save_file_result(
            tab_id,
            path.clone(),
            "saved body".to_string(),
            7,
            None,
            SaveTicket::current_for_test(),
        );

        match result {
            SaveFileResult::Saved {
                tab_id: saved_tab,
                path: saved_path,
                revision,
                stamp,
                body,
            } => {
                assert_eq!(saved_tab, tab_id);
                assert_eq!(saved_path, path.clone());
                assert_eq!(revision, 7);
                assert_eq!(stamp, file_stamp(&path).expect("saved stamp"));
                assert_eq!(body, "saved body");
            }
            other => panic!("expected save success result, got {other:?}"),
        }
        assert_eq!(
            fs::read_to_string(&path).expect("read saved file"),
            "saved body"
        );

        fs::remove_dir_all(dir).expect("remove test temp dir");
    }

    #[test]
    fn superseded_save_ticket_does_not_write_stale_body() {
        let dir = temp_dir("save-stale-ticket");
        let path = dir.join("saved.txt");
        fs::write(&path, "newer body").expect("write newer fixture");
        let tab_id = TabId::from_raw(1);
        let current_generation = Arc::new(Mutex::new(0));
        let old_ticket = SaveTicket::issue(&current_generation);
        let _new_ticket = SaveTicket::issue(&current_generation);

        let result = save_file_result(
            tab_id,
            path.clone(),
            "older body".to_string(),
            1,
            None,
            old_ticket,
        );

        assert_eq!(
            result,
            SaveFileResult::Stale {
                tab_id,
                path: path.clone(),
                revision: 1,
            }
        );
        assert_eq!(
            fs::read_to_string(&path).expect("read saved file"),
            "newer body"
        );

        fs::remove_dir_all(dir).expect("remove test temp dir");
    }

    #[cfg(unix)]
    #[test]
    fn atomic_save_preserves_existing_file_permissions() {
        let dir = temp_dir("save-mode");
        let path = dir.join("script.sh");
        fs::write(&path, "#!/bin/sh\n").expect("write script fixture");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("set executable mode");
        let tab_id = TabId::from_raw(1);

        let result = save_file_result(
            tab_id,
            path.clone(),
            "#!/bin/sh\necho saved\n".to_string(),
            1,
            None,
            SaveTicket::current_for_test(),
        );

        assert!(matches!(result, SaveFileResult::Saved { .. }), "{result:?}");
        let mode = fs::metadata(&path)
            .expect("saved metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o755);

        fs::remove_dir_all(dir).expect("remove test temp dir");
    }

    #[test]
    fn save_file_result_reports_write_failures() {
        let dir = temp_dir("save-failure");

        let tab_id = TabId::from_raw(1);
        let result = save_file_result(
            tab_id,
            dir.clone(),
            "cannot replace directory".to_string(),
            7,
            None,
            SaveTicket::current_for_test(),
        );

        match result {
            SaveFileResult::Failed {
                tab_id: failed_tab,
                path,
                message,
            } => {
                assert_eq!(failed_tab, tab_id);
                assert_eq!(path, dir.clone());
                assert!(!message.is_empty());
            }
            other => panic!("expected save failure result, got {other:?}"),
        }

        fs::remove_dir_all(dir).expect("remove test temp dir");
    }

    #[test]
    fn save_file_result_reports_external_conflicts_without_writing() {
        let dir = temp_dir("save-conflict");
        let path = dir.join("note.txt");
        fs::write(&path, "old").expect("write old fixture");
        let expected_stamp = file_stamp(&path).expect("old stamp");
        fs::write(&path, "external").expect("write external fixture");
        let disk_stamp = file_stamp(&path).expect("external stamp");
        let tab_id = TabId::from_raw(1);

        let result = save_file_result(
            tab_id,
            path.clone(),
            "local".to_string(),
            7,
            Some(expected_stamp),
            SaveTicket::current_for_test(),
        );

        assert_eq!(
            result,
            SaveFileResult::Conflict {
                tab_id,
                path: path.clone(),
                body: "local".to_string(),
                revision: 7,
                disk_stamp,
            }
        );
        assert_eq!(
            fs::read_to_string(&path).expect("read conflicted destination"),
            "external"
        );

        fs::remove_dir_all(dir).expect("remove test temp dir");
    }

    #[test]
    fn save_file_result_reports_deleted_backing_file_as_conflict() {
        let dir = temp_dir("save-deleted");
        let path = dir.join("note.txt");
        fs::write(&path, "old").expect("write old fixture");
        let expected_stamp = file_stamp(&path).expect("old stamp");
        fs::remove_file(&path).expect("delete backing file");
        let tab_id = TabId::from_raw(1);

        let result = save_file_result(
            tab_id,
            path.clone(),
            "local".to_string(),
            7,
            Some(expected_stamp),
            SaveTicket::current_for_test(),
        );

        assert_eq!(
            result,
            SaveFileResult::Conflict {
                tab_id,
                path: path.clone(),
                body: "local".to_string(),
                revision: 7,
                disk_stamp: expected_stamp,
            }
        );
        assert!(!path.exists());

        fs::remove_dir_all(dir).expect("remove test temp dir");
    }

    #[test]
    fn can_start_autosave_job_requires_current_revision_and_no_inflight_write() {
        let dir = temp_dir("autosave-start");
        let path = dir.join("note.txt");
        let tab = tab_for_path(path.clone(), "old");
        let mut inflight = HashSet::new();

        assert!(can_start_autosave_job(
            std::slice::from_ref(&tab),
            &inflight,
            tab.id(),
            &path,
            0
        ));

        inflight.insert(path.clone());
        assert!(!can_start_autosave_job(
            std::slice::from_ref(&tab),
            &inflight,
            tab.id(),
            &path,
            0
        ));

        let mut stale_model = EditorModel::from_tab(tab, "Ready.".to_string());
        stale_model.replace_text(Some(0..0), "new ".into(), UndoBoundary::Break);
        assert!(!can_start_autosave_job(
            stale_model.tabs(),
            &HashSet::new(),
            TabId::from_raw(1),
            &path,
            0
        ));

        fs::remove_dir_all(dir).expect("remove test temp dir");
    }

    #[test]
    fn autosave_completion_commits_current_revision() {
        let dir = temp_dir("autosave-commit");
        let path = dir.join("note.txt");
        fs::write(&path, "old").expect("write autosave destination");
        let expected_stamp = file_stamp(&path).expect("initial stamp");
        let tab = ModelEditorTab::from_path_with_stamp(
            TabId::from_raw(1),
            path.clone(),
            "old",
            Some(expected_stamp),
        );
        let job = AutosaveJob {
            tab_id: tab.id(),
            path: path.clone(),
            body: "new".to_string(),
            revision: 0,
            expected_stamp: Some(expected_stamp),
        };

        let temp_path = write_autosave_temp_file(&job).expect("write autosave temp file");
        let completion = autosave_completion(&[tab], job, Ok(temp_path));

        match completion {
            Some(AutosaveCompletion::Finished {
                tab_id,
                path: saved_path,
                revision,
                stamp,
                body,
            }) => {
                assert_eq!(tab_id, TabId::from_raw(1));
                assert_eq!(saved_path, path.clone());
                assert_eq!(revision, 0);
                assert_eq!(stamp, file_stamp(&path).expect("autosaved stamp"));
                assert_eq!(body, "new");
            }
            other => panic!("expected autosave completion, got {other:?}"),
        }
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
        let tab = tab_for_path(path.clone(), "old");
        let mut model = EditorModel::from_tab(tab, "Ready.".to_string());
        let expected_stamp = model.active_tab().file_stamp();
        model.replace_text(Some(0..0), "current ".into(), UndoBoundary::Break);
        let job = AutosaveJob {
            tab_id: model.active_tab_id(),
            path: path.clone(),
            body: "stale".to_string(),
            revision: 0,
            expected_stamp,
        };

        let temp_path = write_autosave_temp_file(&job).expect("write autosave temp file");
        let completion = autosave_completion(model.tabs(), job, Ok(temp_path.clone()));

        assert_eq!(completion, None);
        assert!(!temp_path.exists());
        assert_eq!(
            fs::read_to_string(&path).expect("read destination file"),
            "old"
        );

        fs::remove_dir_all(dir).expect("remove test temp dir");
    }

    #[test]
    fn autosave_completion_reports_conflict_without_renaming_temp() {
        let dir = temp_dir("autosave-conflict");
        let path = dir.join("note.txt");
        fs::write(&path, "old").expect("write autosave destination");
        let expected_stamp = file_stamp(&path).expect("old stamp");
        let tab = ModelEditorTab::from_path_with_stamp(
            TabId::from_raw(1),
            path.clone(),
            "old",
            Some(expected_stamp),
        );
        let job = AutosaveJob {
            tab_id: tab.id(),
            path: path.clone(),
            body: "local".to_string(),
            revision: 0,
            expected_stamp: Some(expected_stamp),
        };
        let temp_path = write_autosave_temp_file(&job).expect("write autosave temp file");
        fs::write(&path, "external").expect("write external fixture");
        let disk_stamp = file_stamp(&path).expect("external stamp");

        let completion = autosave_completion(&[tab], job, Ok(temp_path.clone()));

        assert_eq!(
            completion,
            Some(AutosaveCompletion::Conflict {
                tab_id: TabId::from_raw(1),
                path: path.clone(),
                body: "local".to_string(),
                revision: 0,
                disk_stamp,
            })
        );
        assert!(!temp_path.exists());
        assert_eq!(
            fs::read_to_string(&path).expect("read destination file"),
            "external"
        );

        fs::remove_dir_all(dir).expect("remove test temp dir");
    }

    #[test]
    fn autosave_completion_reports_deleted_backing_file_as_conflict() {
        let dir = temp_dir("autosave-deleted");
        let path = dir.join("note.txt");
        fs::write(&path, "old").expect("write autosave destination");
        let expected_stamp = file_stamp(&path).expect("old stamp");
        let tab = ModelEditorTab::from_path_with_stamp(
            TabId::from_raw(1),
            path.clone(),
            "old",
            Some(expected_stamp),
        );
        let job = AutosaveJob {
            tab_id: tab.id(),
            path: path.clone(),
            body: "local".to_string(),
            revision: 0,
            expected_stamp: Some(expected_stamp),
        };
        let temp_path = write_autosave_temp_file(&job).expect("write autosave temp file");
        fs::remove_file(&path).expect("delete backing file");

        let completion = autosave_completion(&[tab], job, Ok(temp_path.clone()));

        assert_eq!(
            completion,
            Some(AutosaveCompletion::Conflict {
                tab_id: TabId::from_raw(1),
                path: path.clone(),
                body: "local".to_string(),
                revision: 0,
                disk_stamp: expected_stamp,
            })
        );
        assert!(!temp_path.exists());
        assert!(!path.exists());

        fs::remove_dir_all(dir).expect("remove test temp dir");
    }
}
