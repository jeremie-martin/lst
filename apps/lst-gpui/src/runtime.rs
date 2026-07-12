use gpui::{ClipboardItem, Context, Window};
use lst_editor::{EditorEffect, EditorTab as ModelEditorTab, FileStamp, SaveExpectation, TabCloseRequest, TabId};
use rfd::FileDialog;
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

use crate::{
    diagnostics, elapsed_ms,
    recent::{normalize_recent_path, RecentOrigin},
    ClosePrompt, ClosePromptIntent, ClosePromptStatus, ExitSaveContinuation, FileConflictNotice, LstGpuiApp,
    PendingExitSave, QuitReview, QuitReviewDecision, QuitReviewItemStatus,
};
use lst_editor::UndoBoundary;
use std::ops::Range;

mod clipboard;
mod scratchpad;

use clipboard::persist_clipboards_after_exit;
pub(crate) use scratchpad::create_scratchpad_note;
use scratchpad::{remove_previous_scratchpad_after_save_as, remove_scratchpad_file_if_unreferenced};

#[derive(Debug)]
struct AutosaveJob {
    tab_id: TabId,
    path: PathBuf,
    body: String,
    revision: u64,
    expectation: SaveExpectation,
    ticket: SaveTicket,
}

#[derive(Clone, Debug)]
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
    generation.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

type OpenFileResults = (Vec<(PathBuf, String, Option<FileStamp>)>, Vec<(PathBuf, String)>);

#[derive(Clone, Debug)]
struct ExternalFileRequest {
    tab_id: TabId,
    path: PathBuf,
    expected_stamp: Option<FileStamp>,
}

#[derive(Clone, Debug)]
struct ConflictReloadRequest {
    notice: FileConflictNotice,
    revision: u64,
}

impl ConflictReloadRequest {
    fn is_current(&self, tab: Option<&ModelEditorTab>, notice: Option<&FileConflictNotice>) -> bool {
        notice == Some(&self.notice)
            && tab.is_some_and(|tab| {
                tab.id() == self.notice.tab_id
                    && tab.path() == Some(&self.notice.path)
                    && tab.revision() == self.revision
            })
    }
}

#[cfg(test)]
mod conflict_reload_request_tests {
    use super::*;

    #[test]
    fn editor_edit_invalidates_an_inflight_conflict_reload() {
        let tab_id = TabId::from_raw(1);
        let path = PathBuf::from("/tmp/conflicted.txt");
        let notice = FileConflictNotice {
            tab_id,
            path: path.clone(),
            disk_stamp: FileStamp::from_raw(12, Some(34)),
        };
        let tab = ModelEditorTab::from_path_with_stamp(tab_id, path, "local text", None);
        let request = ConflictReloadRequest {
            notice: notice.clone(),
            revision: tab.revision(),
        };
        let mut model = lst_editor::EditorModel::from_tabs(tab, Vec::new(), "Ready.".to_string());

        assert!(request.is_current(model.tab_by_id(tab_id), Some(&notice)));
        model.replace_text(None, "newer local text".to_string(), UndoBoundary::Break);
        assert!(!request.is_current(model.tab_by_id(tab_id), Some(&notice)));
    }

    #[test]
    fn changed_notice_invalidates_an_inflight_conflict_reload() {
        let tab_id = TabId::from_raw(1);
        let path = PathBuf::from("/tmp/conflicted.txt");
        let notice = FileConflictNotice {
            tab_id,
            path: path.clone(),
            disk_stamp: FileStamp::from_raw(12, Some(34)),
        };
        let tab = ModelEditorTab::from_path_with_stamp(tab_id, path, "local text", None);
        let request = ConflictReloadRequest {
            notice: notice.clone(),
            revision: tab.revision(),
        };
        let newer_notice = FileConflictNotice {
            disk_stamp: FileStamp::from_raw(13, Some(35)),
            ..notice
        };

        assert!(!request.is_current(Some(&tab), Some(&newer_notice)));
    }
}

#[derive(Debug)]
enum ExternalFileObservation {
    Unchanged(ExternalFileRequest),
    Changed {
        request: ExternalFileRequest,
        text: String,
        disk_stamp: FileStamp,
    },
    Missing(ExternalFileRequest),
    Failed {
        request: ExternalFileRequest,
        message: String,
    },
}

/// Outcome of a single file-write attempt (save, save-as, or autosave).
///
/// `Stale` is produced when a newer explicit save or autosave supersedes a
/// ticket before its atomic replacement.
#[derive(Debug, PartialEq, Eq)]
enum FileWriteOutcome {
    Written {
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

impl FileWriteOutcome {
    fn path(&self) -> &Path {
        match self {
            Self::Written { path, .. }
            | Self::Failed { path, .. }
            | Self::Conflict { path, .. }
            | Self::Stale { path, .. } => path,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ConflictWrite {
    Save {
        revision: u64,
        exit_save: Option<PendingExitSave>,
    },
    Autosave {
        revision: u64,
    },
}

#[derive(Clone, Copy, Debug)]
struct SaveTextOptions {
    trim_trailing_whitespace: bool,
    ensure_final_newline: bool,
}

#[derive(Debug)]
enum SaveKind {
    Save {
        guard: SaveGuard,
        exit_save: Option<PendingExitSave>,
    },
    SaveAs {
        previous_scratchpad: Option<PathBuf>,
        exit_save: Option<PendingExitSave>,
    },
}

#[derive(Clone, Copy, Debug)]
enum SaveGuard {
    /// A normal save snapshots the model's observed disk version. If the save
    /// has to wait for an earlier app-owned write, refresh that snapshot after
    /// the earlier completion has updated the tab's file stamp.
    RefreshAfterDeferred(SaveExpectation),
    /// Conflict actions are intentionally tied to the exact disk version the
    /// user reviewed and must never silently retarget a later version.
    Exact(SaveExpectation),
}

#[derive(Debug)]
pub(crate) struct QueuedSaveJob {
    tab_id: TabId,
    path: PathBuf,
    body: String,
    revision: u64,
    kind: SaveKind,
}

impl SaveKind {
    fn exit_save(&self) -> Option<PendingExitSave> {
        match self {
            Self::Save { exit_save, .. } | Self::SaveAs { exit_save, .. } => *exit_save,
        }
    }

    fn set_exit_save(&mut self, pending: PendingExitSave) {
        match self {
            Self::Save { exit_save, .. } | Self::SaveAs { exit_save, .. } => *exit_save = Some(pending),
        }
    }
}

impl LstGpuiApp {
    pub(crate) fn handle_model_effects(&mut self, effects: Vec<EditorEffect>, cx: &mut Context<Self>) {
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
                        self.record_operation("paste_clipboard", Some(clipboard_read_ms), elapsed_ms(apply_started));
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
                    expectation,
                } => {
                    let exit_save = self.pending_exit_save.filter(|pending| pending.tab_id == tab_id);
                    self.spawn_save_job(
                        tab_id,
                        path,
                        body,
                        revision,
                        SaveKind::Save {
                            guard: SaveGuard::RefreshAfterDeferred(expectation),
                            exit_save,
                        },
                        cx,
                    )
                }
                EditorEffect::SaveFileAs {
                    tab_id,
                    suggested_name,
                    body,
                    revision,
                    previous_scratchpad_path,
                } => {
                    let exit_save = self.pending_exit_save.filter(|pending| pending.tab_id == tab_id);
                    let Some(path) = FileDialog::new().set_file_name(&suggested_name).save_file() else {
                        self.finish_exit_save_failure(exit_save, "Save cancelled".to_string(), cx);
                        continue;
                    };
                    let normalized = normalize_recent_path(&path);
                    let existing = self.model.tabs().iter().find_map(|tab| {
                        (tab.id() != tab_id
                            && tab
                                .path()
                                .is_some_and(|open_path| normalize_recent_path(open_path) == normalized))
                        .then_some(tab.id())
                    });
                    if let Some(existing_tab) = existing {
                        self.update_model(cx, true, |model| model.set_active_tab(existing_tab));
                        let message = format!("{} is already open in another tab.", path.display());
                        self.cleanup_message = Some(message.clone());
                        self.finish_exit_save_failure(exit_save, message, cx);
                        continue;
                    }
                    self.spawn_save_job(
                        tab_id,
                        path,
                        body,
                        revision,
                        SaveKind::SaveAs {
                            previous_scratchpad: previous_scratchpad_path,
                            exit_save,
                        },
                        cx,
                    );
                }
                EditorEffect::AutosaveFile {
                    tab_id,
                    path,
                    body,
                    revision,
                    expectation,
                } => self.start_autosave_job(tab_id, path, body, revision, expectation, cx),
            }
        }
    }

    fn open_files_from_dialog(&mut self, cx: &mut Context<Self>) {
        let Some(paths) = FileDialog::new().pick_files() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let results = cx
                .background_executor()
                .spawn(async move { open_file_results(paths) })
                .await;
            let _ = this.update(cx, |view, cx| view.apply_open_file_results(results, cx));
        })
        .detach();
    }

    pub(crate) fn start_background_tasks(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.autosave_started {
            return;
        }
        self.autosave_started = true;
        let view = cx.entity();
        window
            .spawn(cx, async move |cx| loop {
                cx.background_executor().timer(Duration::from_millis(500)).await;
                if view
                    .update(cx, |view, cx| {
                        let cursor_was_visible = view.cursor_visible;
                        let next_cursor_visible = if view.settings.settings.editor.cursor_blink {
                            !view.cursor_visible
                        } else {
                            true
                        };
                        view.cursor_visible = next_cursor_visible;
                        if cursor_was_visible != next_cursor_visible {
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            })
            .detach();

        let view = cx.entity();
        window
            .spawn(cx, async move |cx| loop {
                cx.background_executor().timer(Duration::from_millis(500)).await;
                if view
                    .update(cx, |view, cx| {
                        view.check_settings_reload(cx);
                        view.check_external_file_changes(cx);
                        let eligible_tabs = view.autosave_ready_tabs();
                        if eligible_tabs.is_empty() {
                            return;
                        }
                        let include_ordinary =
                            view.settings.settings.files.autosave == crate::settings::AutosaveMode::All;
                        view.update_model(cx, false, |model| {
                            model.autosave_tick(include_ordinary, &eligible_tabs);
                        });
                    })
                    .is_err()
                {
                    break;
                }
            })
            .detach();
    }

    fn autosave_ready_tabs(&mut self) -> Vec<TabId> {
        const IDLE_BEFORE_AUTOSAVE: Duration = Duration::from_millis(750);

        let now = Instant::now();
        let revisions = self
            .model
            .tabs()
            .iter()
            .map(|tab| (tab.id(), tab.revision(), tab.modified(), tab.has_suppressed_conflict()))
            .collect::<Vec<_>>();
        self.autosave_observed_revisions
            .retain(|tab_id, _| revisions.iter().any(|(current, _, _, _)| current == tab_id));

        let mut ready = Vec::new();
        for (tab_id, revision, modified, conflict_suppressed) in revisions {
            if !modified || conflict_suppressed {
                self.autosave_observed_revisions.remove(&tab_id);
                continue;
            }
            let observed = self
                .autosave_observed_revisions
                .entry(tab_id)
                .or_insert((revision, now));
            if observed.0 != revision {
                *observed = (revision, now);
                continue;
            }
            if now.duration_since(observed.1) >= IDLE_BEFORE_AUTOSAVE {
                ready.push(tab_id);
                observed.1 = now;
            }
        }
        ready
    }

    fn check_settings_reload(&mut self, cx: &mut Context<Self>) {
        if self.settings_reload_inflight {
            return;
        }
        self.settings_reload_inflight = true;
        let settings = self.settings.clone();
        let generation = self.settings_generation;
        cx.spawn(async move |this, cx| {
            let reloaded = cx
                .background_executor()
                .spawn(async move { settings.reloaded_if_changed() })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.settings_reload_inflight = false;
                if view.settings_generation == generation {
                    if let Some(settings) = reloaded {
                        view.apply_reloaded_settings(settings, cx);
                    }
                } else if reloaded.is_some() {
                    // The in-app change won the race. A later probe will
                    // compare the now-current document with disk again.
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn start_autosave_job(
        &mut self,
        tab_id: TabId,
        path: PathBuf,
        body: String,
        revision: u64,
        expectation: SaveExpectation,
        cx: &mut Context<Self>,
    ) {
        if !can_start_autosave_job(self.model.tabs(), &self.autosave_inflight, tab_id, &path, revision) {
            return;
        }
        if self.save_inflight.contains_key(&path) || self.queued_saves.contains_key(&path) {
            return;
        }
        let ticket = self.issue_save_ticket(&path);
        let job = AutosaveJob {
            tab_id,
            path,
            body,
            revision,
            expectation,
            ticket,
        };
        self.autosave_inflight.insert(job.path.clone());
        let inflight_path = job.path.clone();
        let completion_ticket = job.ticket.clone();
        cx.spawn(async move |this, cx| {
            let completion = cx
                .background_executor()
                .spawn(async move {
                    write_autosave_body_result(
                        job.tab_id,
                        job.path,
                        job.body,
                        job.revision,
                        job.expectation,
                        job.ticket,
                    )
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.autosave_inflight.remove(&inflight_path);
                // A newer save can be issued after this job commits but
                // before its background result reaches the UI thread. Do not
                // let that older completion regress the model's saved body or
                // file stamp.
                if completion_ticket.is_current() {
                    view.apply_autosave_completion(completion, cx);
                }
                view.start_queued_save_for_path(&inflight_path, cx);
            });
        })
        .detach();
    }

    fn spawn_save_job(
        &mut self,
        tab_id: TabId,
        path: PathBuf,
        body: String,
        revision: u64,
        kind: SaveKind,
        cx: &mut Context<Self>,
    ) {
        let job = QueuedSaveJob {
            tab_id,
            path,
            body,
            revision,
            kind,
        };
        if self.path_write_inflight(&job.path) {
            self.queue_save_job(job);
            cx.notify();
            return;
        }
        self.start_save_job(job, false, cx);
    }

    fn start_save_job(&mut self, job: QueuedSaveJob, deferred: bool, cx: &mut Context<Self>) {
        if self.path_write_inflight(&job.path) {
            self.queue_save_job(job);
            return;
        }
        let QueuedSaveJob {
            tab_id,
            path,
            body,
            revision,
            kind,
        } = job;
        let expectation = match &kind {
            SaveKind::Save {
                guard: SaveGuard::RefreshAfterDeferred(_),
                ..
            } if deferred => {
                let Some(tab) = self.model.tab_by_id(tab_id).filter(|tab| tab.path() == Some(&path)) else {
                    self.finish_exit_save_failure(
                        kind.exit_save(),
                        "Document changed identity while waiting to save".to_string(),
                        cx,
                    );
                    return;
                };
                tab.save_expectation()
            }
            SaveKind::Save {
                guard: SaveGuard::RefreshAfterDeferred(requested) | SaveGuard::Exact(requested),
                ..
            } => *requested,
            SaveKind::SaveAs { .. } => SaveExpectation::Unguarded,
        };
        let save_options = self.save_text_options();
        let ticket = self.issue_save_ticket(&path);
        self.begin_save_inflight(&path);
        let save_started = Instant::now();
        let completion_ticket = ticket.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { save_file_result(tab_id, path, body, revision, expectation, ticket, save_options) })
                .await;
            let save_complete_ms = elapsed_ms(save_started);
            let _ = this.update(cx, |view, cx| {
                if diagnostics::trace_enabled() {
                    let kind_label = match kind {
                        SaveKind::Save { .. } => "regular",
                        SaveKind::SaveAs { .. } => "save_as",
                    };
                    diagnostics::record_label("save_complete", kind_label);
                    diagnostics::record_ms("save_complete_ms", save_complete_ms);
                }
                view.apply_save_outcome(result, kind, completion_ticket, cx);
            });
        })
        .detach();
    }

    fn path_write_inflight(&self, path: &Path) -> bool {
        self.save_inflight.contains_key(path) || self.autosave_inflight.contains(path)
    }

    fn queue_save_job(&mut self, mut job: QueuedSaveJob) {
        if let Some(previous) = self.queued_saves.remove(&job.path) {
            if let Some(exit_save) = previous.kind.exit_save() {
                if previous.tab_id == job.tab_id && job.kind.exit_save().is_none() {
                    job.kind.set_exit_save(exit_save);
                } else if previous.tab_id != job.tab_id && job.kind.exit_save().is_none() {
                    self.queued_saves.insert(previous.path.clone(), previous);
                    return;
                }
            }
        }
        self.queued_saves.insert(job.path.clone(), job);
    }

    fn start_queued_save_for_path(&mut self, path: &Path, cx: &mut Context<Self>) {
        if self.path_write_inflight(path) {
            return;
        }
        if let Some(job) = self.queued_saves.remove(path) {
            self.start_save_job(job, true, cx);
        }
    }

    fn issue_save_ticket(&mut self, path: &Path) -> SaveTicket {
        let current_generation = self
            .save_ticket_generations
            .entry(path.to_path_buf())
            .or_insert_with(|| Arc::new(Mutex::new(0)));
        SaveTicket::issue(current_generation)
    }

    fn save_generation_for_path(&self, path: &Path) -> u64 {
        self.save_ticket_generations
            .get(path)
            .map(|generation| *lock_generation(generation))
            .unwrap_or(0)
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
        if self.maintenance_inflight {
            return;
        }
        let requests = self
            .model
            .tabs()
            .iter()
            .filter_map(|tab| {
                let path = tab.path()?.clone();
                (!self.path_write_inflight(&path) && !self.queued_saves.contains_key(&path)).then_some(
                    ExternalFileRequest {
                        tab_id: tab.id(),
                        path,
                        expected_stamp: tab.file_stamp(),
                    },
                )
            })
            .collect::<Vec<_>>();
        if requests.is_empty() {
            return;
        }
        self.maintenance_inflight = true;
        cx.spawn(async move |this, cx| {
            let observations = cx
                .background_executor()
                .spawn(async move { observe_external_files(requests) })
                .await;
            let _ = this.update(cx, |view, cx| view.apply_external_file_observations(observations, cx));
        })
        .detach();
    }

    fn apply_external_file_observations(&mut self, observations: Vec<ExternalFileObservation>, cx: &mut Context<Self>) {
        self.maintenance_inflight = false;
        for observation in observations {
            let request = match &observation {
                ExternalFileObservation::Unchanged(request)
                | ExternalFileObservation::Changed { request, .. }
                | ExternalFileObservation::Missing(request)
                | ExternalFileObservation::Failed { request, .. } => request,
            };
            let current = self.model.tab_by_id(request.tab_id);
            if current.is_none_or(|tab| tab.path() != Some(&request.path) || tab.file_stamp() != request.expected_stamp)
                || self.path_write_inflight(&request.path)
                || self.queued_saves.contains_key(&request.path)
            {
                continue;
            }

            match observation {
                ExternalFileObservation::Unchanged(_) => {}
                ExternalFileObservation::Missing(request) => {
                    self.file_conflicts.remove(&request.tab_id);
                    let missing_path = request.path.clone();
                    self.update_model(cx, true, |model| {
                        model.mark_tab_backing_file_missing(request.tab_id);
                    });
                    self.cleanup_message = Some(format!(
                        "{} was deleted. Save to recreate it, use Save As, or discard explicitly.",
                        missing_path.display()
                    ));
                    cx.notify();
                }
                ExternalFileObservation::Failed { request, message } => {
                    self.cleanup_message = Some(format!("Could not check {}: {message}", request.path.display()));
                    cx.notify();
                }
                ExternalFileObservation::Changed {
                    request,
                    text,
                    disk_stamp,
                } => {
                    let Some(tab) = self.model.tab_by_id(request.tab_id) else {
                        continue;
                    };
                    let save_required = tab.modified() || tab.backing_file_missing();
                    if !save_required || tab.buffer_text() == text {
                        self.apply_clean_external_file(request.tab_id, request.path, text, disk_stamp, cx);
                    } else if !tab.conflict_suppressed_for(disk_stamp) {
                        self.handle_file_conflict(
                            request.tab_id,
                            request.path,
                            disk_stamp,
                            ConflictWrite::Autosave {
                                revision: tab.revision(),
                            },
                            cx,
                        );
                    }
                }
            }
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
        if !tab.modified() && !tab.backing_file_missing() {
            let exit_save = match write {
                ConflictWrite::Save { exit_save, .. } => exit_save,
                ConflictWrite::Autosave { .. } => None,
            };
            self.reload_clean_external_file(tab_id, path, exit_save, cx);
            return;
        }
        let explicit_save = matches!(write, ConflictWrite::Save { .. });
        let (revision, exit_save) = match write {
            ConflictWrite::Save { revision, exit_save } => (revision, exit_save),
            ConflictWrite::Autosave { revision } => (revision, None),
        };
        if tab.revision() != revision {
            // An explicit save that hit both a disk change and a concurrent
            // edit must not fail silently: without feedback the status bar
            // keeps the last "Saved" text while nothing was written. A save
            // queued behind this one will re-raise the conflict itself.
            if explicit_save && exit_save.is_none() && !self.queued_saves.contains_key(&path) {
                self.update_model(cx, false, |model| {
                    model.save_failed(path.clone(), "document changed while saving; save again".to_string());
                });
            }
            self.finish_exit_save_failure(exit_save, "Document changed while saving".to_string(), cx);
            cx.notify();
            return;
        }
        // Dismiss acknowledges exactly one disk version. Autosave must not
        // overwrite it in the background, while the next explicit Save is
        // an intentional request and may replace that acknowledged version.
        if tab.conflict_suppressed_for(disk_stamp) {
            match write {
                ConflictWrite::Save { revision, exit_save } => {
                    let body = tab.buffer_text();
                    self.spawn_save_job(
                        tab_id,
                        path,
                        body,
                        revision,
                        SaveKind::Save {
                            guard: SaveGuard::Exact(SaveExpectation::Matching(disk_stamp)),
                            exit_save,
                        },
                        cx,
                    );
                }
                ConflictWrite::Autosave { .. } => {
                    self.finish_exit_save_failure(exit_save, "File changed on disk".to_string(), cx);
                }
            }
            return;
        }

        let notice = FileConflictNotice {
            tab_id,
            path: path.clone(),
            disk_stamp,
        };
        if self.file_conflicts.get(&tab_id) != Some(&notice) {
            self.file_conflicts.insert(tab_id, notice);
        }
        self.finish_exit_save_failure(
            exit_save,
            format!(
                "{} changed on disk; choose an action in the editor banner",
                path.display()
            ),
            cx,
        );
        cx.notify();
    }

    pub(crate) fn active_file_conflict(&self) -> Option<&FileConflictNotice> {
        self.file_conflicts.get(&self.model.active_tab_id())
    }

    pub(crate) fn reload_file_conflict(&mut self, tab_id: TabId, cx: &mut Context<Self>) {
        let Some(notice) = self.file_conflicts.get(&tab_id).cloned() else {
            return;
        };
        let Some(revision) = self
            .model
            .tab_by_id(tab_id)
            .filter(|tab| tab.path() == Some(&notice.path))
            .map(ModelEditorTab::revision)
        else {
            return;
        };
        let request = ConflictReloadRequest { notice, revision };
        let path = request.notice.path.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { read_file_with_stamp(&path) })
                .await;
            let _ = this.update(cx, |view, cx| {
                if !request.is_current(view.model.tab_by_id(tab_id), view.file_conflicts.get(&tab_id)) {
                    if view.file_conflicts.get(&tab_id) == Some(&request.notice) {
                        view.cleanup_message = Some(format!(
                            "Reload cancelled because {} changed in the editor while its disk copy was being read.",
                            request.notice.path.display()
                        ));
                        cx.notify();
                    }
                    return;
                }
                match result {
                    Ok((text, stamp)) => {
                        view.file_conflicts.remove(&tab_id);
                        let reload_path = request.notice.path;
                        view.update_model(cx, true, |model| {
                            model.reload_tab_from_disk(tab_id, reload_path, text, stamp);
                        });
                    }
                    Err(error) => {
                        view.cleanup_message =
                            Some(format!("Could not reload {}: {error}", request.notice.path.display()));
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    pub(crate) fn keep_file_conflict_local(&mut self, tab_id: TabId, cx: &mut Context<Self>) {
        let Some(notice) = self.file_conflicts.remove(&tab_id) else {
            return;
        };
        let Some(tab) = self.model.tab_by_id(tab_id) else {
            return;
        };
        if tab.path() != Some(&notice.path) {
            return;
        }
        self.spawn_save_job(
            tab_id,
            notice.path,
            tab.buffer_text(),
            tab.revision(),
            SaveKind::Save {
                guard: SaveGuard::Exact(SaveExpectation::Matching(notice.disk_stamp)),
                exit_save: None,
            },
            cx,
        );
    }

    pub(crate) fn save_file_conflict_as(&mut self, tab_id: TabId, cx: &mut Context<Self>) {
        if !self.file_conflicts.contains_key(&tab_id) {
            return;
        }
        self.update_model(cx, true, |model| {
            model.set_active_tab(tab_id);
            model.request_save_as_tab(tab_id);
        });
    }

    pub(crate) fn dismiss_file_conflict(&mut self, tab_id: TabId, cx: &mut Context<Self>) {
        let Some(notice) = self.file_conflicts.remove(&tab_id) else {
            return;
        };
        self.update_model(cx, true, |model| {
            model.suppress_file_conflict(tab_id, notice.path, notice.disk_stamp);
        });
    }

    fn save_text_options(&self) -> SaveTextOptions {
        SaveTextOptions {
            trim_trailing_whitespace: self.settings.settings.files.trim_trailing_whitespace
                || env_flag("LST_SAVE_TRIM_TRAILING_WS"),
            ensure_final_newline: self.settings.settings.files.ensure_final_newline
                || env_flag("LST_SAVE_ENSURE_FINAL_NEWLINE"),
        }
    }

    fn apply_clean_external_file(
        &mut self,
        tab_id: TabId,
        path: PathBuf,
        text: String,
        disk_stamp: FileStamp,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.model.tab_by_id(tab_id) else {
            return;
        };
        let same_text = tab.buffer_text() == text;
        let modified = tab.modified();
        let revision = tab.revision();
        self.file_conflicts.remove(&tab_id);
        self.update_model(cx, true, |model| {
            if same_text && modified {
                model.autosave_finished_for_tab(tab_id, path, revision, disk_stamp, text);
            } else if same_text {
                model.refresh_file_stamp_for_tab(tab_id, path, disk_stamp);
            } else {
                model.reload_tab_from_disk(tab_id, path, text, disk_stamp);
            }
        });
    }

    fn reload_clean_external_file(
        &mut self,
        tab_id: TabId,
        path: PathBuf,
        exit_save: Option<PendingExitSave>,
        cx: &mut Context<Self>,
    ) {
        let Some((expected_revision, expected_stamp)) = self
            .model
            .tab_by_id(tab_id)
            .filter(|tab| !tab.modified() && tab.path() == Some(&path))
            .map(|tab| (tab.revision(), tab.file_stamp()))
        else {
            self.finish_exit_save_failure(
                exit_save,
                "Document changed while resolving an external edit".to_string(),
                cx,
            );
            return;
        };
        let expected_save_generation = self.save_generation_for_path(&path);
        cx.spawn(async move |this, cx| {
            let read_path = path.clone();
            let result = cx
                .background_executor()
                .spawn(async move { read_file_with_stamp(&read_path) })
                .await;
            let _ = this.update(cx, |view, cx| match result {
                Ok((text, observed_stamp)) => {
                    let still_clean = view.model.tab_by_id(tab_id).is_some_and(|tab| {
                        !tab.modified()
                            && tab.path() == Some(&path)
                            && tab.revision() == expected_revision
                            && tab.file_stamp() == expected_stamp
                    }) && view.save_generation_for_path(&path) == expected_save_generation
                        && !view.path_write_inflight(&path)
                        && !view.queued_saves.contains_key(&path);
                    if still_clean {
                        view.apply_clean_external_file(tab_id, path, text, observed_stamp, cx);
                        view.finish_exit_save_success(exit_save, cx);
                    } else {
                        view.finish_exit_save_failure(
                            exit_save,
                            "Document changed while resolving an external edit".to_string(),
                            cx,
                        );
                    }
                }
                Err(error) => {
                    view.finish_exit_save_failure(
                        exit_save,
                        format!("Could not reload {}: {error}", path.display()),
                        cx,
                    );
                    view.cleanup_message = Some(format!("Could not reload {}: {error}", path.display()));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(crate) fn request_close_active_tab(&mut self, cx: &mut Context<Self>) {
        let index = self.model.active_index();
        self.request_close_tab_at(index, cx);
    }

    pub(crate) fn request_close_tab_at(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.close_prompt.is_some() || self.quit_review.is_some() || self.pending_exit_save.is_some() {
            return;
        }
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
                self.copy_scratchpad_for_tab_close(tab_id, cx);
                self.record_closed_tab(tab_id);
                self.update_model(cx, true, |model| {
                    model.close_clean_tab(tab_id);
                });
            }
            Some(TabCloseRequest::SaveAndClose { tab_id }) => {
                if self.model.tab_by_id(tab_id).is_some_and(ModelEditorTab::is_scratchpad) {
                    self.start_exit_save(tab_id, ExitSaveContinuation::CloseTab, cx);
                } else {
                    self.close_prompt = Some(ClosePrompt {
                        tab_id,
                        intent: ClosePromptIntent::CloseTab,
                        status: ClosePromptStatus::Reviewing,
                    });
                    cx.notify();
                }
            }
            None => {}
        }
    }

    pub(crate) fn request_quit(&mut self, cx: &mut Context<Self>) {
        if self.close_prompt.is_some() || self.quit_review.is_some() || self.pending_exit_save.is_some() {
            return;
        }
        if self.cleanup_in_flight {
            self.cleanup_message = Some("Wait for text cleanup to finish before quitting.".to_string());
            self.force_editor_focus = true;
            cx.notify();
            return;
        }
        let retrying_same_clipboard_payload = self
            .clipboard_quit_bypass
            .as_ref()
            .is_some_and(|bypass| Some(bypass) == self.active_scratchpad_clipboard_payload().as_ref());
        if !retrying_same_clipboard_payload {
            self.exit_discarded_revisions.clear();
            self.clipboard_quit_bypass = None;
        }
        self.continue_quit_sequence(cx);
    }

    fn continue_quit_sequence(&mut self, cx: &mut Context<Self>) {
        let dirty_regular = self
            .model
            .tabs()
            .iter()
            .filter(|tab| !tab.is_scratchpad() && (tab.modified() || tab.backing_file_missing()))
            .filter(|tab| self.exit_discarded_revisions.get(&tab.id()) != Some(&tab.revision()))
            .map(ModelEditorTab::id)
            .collect::<Vec<_>>();
        if dirty_regular.len() >= 2 {
            self.quit_review =
                Some(QuitReview::new(dirty_regular.into_iter().filter_map(|tab_id| {
                    self.model.tab_by_id(tab_id).map(|tab| (tab_id, tab_identity(tab)))
                })));
            self.quit_review_scroll.scroll_to_item(0);
            cx.notify();
        } else if let Some(tab_id) = dirty_regular.first().copied() {
            self.close_prompt = Some(ClosePrompt {
                tab_id,
                intent: ClosePromptIntent::Quit,
                status: ClosePromptStatus::Reviewing,
            });
            cx.notify();
        } else {
            self.continue_quit_scratchpads(cx);
        }
    }

    pub(crate) fn confirm_close_prompt_save(&mut self, cx: &mut Context<Self>) {
        let Some(prompt) = self.close_prompt.as_mut() else {
            return;
        };
        if matches!(prompt.status, ClosePromptStatus::Saving) {
            return;
        }
        let tab_id = prompt.tab_id;
        let continuation = match prompt.intent {
            ClosePromptIntent::CloseTab => ExitSaveContinuation::CloseTab,
            ClosePromptIntent::Quit => ExitSaveContinuation::QuitSingle,
        };
        prompt.status = ClosePromptStatus::Saving;
        self.start_exit_save(tab_id, continuation, cx);
    }

    pub(crate) fn confirm_close_prompt_discard(&mut self, cx: &mut Context<Self>) {
        if self
            .close_prompt
            .as_ref()
            .is_some_and(|prompt| matches!(prompt.status, ClosePromptStatus::Saving))
        {
            return;
        }
        let Some(prompt) = self.close_prompt.take() else {
            return;
        };
        match prompt.intent {
            ClosePromptIntent::CloseTab => {
                self.record_closed_tab(prompt.tab_id);
                self.update_model(cx, true, |model| {
                    model.discard_close_tab(prompt.tab_id);
                });
            }
            ClosePromptIntent::Quit => {
                self.remember_exit_discard(prompt.tab_id);
                self.continue_quit_scratchpads(cx);
            }
        }
    }

    pub(crate) fn cancel_close_prompt(&mut self, cx: &mut Context<Self>) {
        if self
            .close_prompt
            .as_ref()
            .is_some_and(|prompt| matches!(prompt.status, ClosePromptStatus::Saving))
        {
            return;
        }
        if self.close_prompt.take().is_some() {
            // Cancelling aborts the whole quit attempt. In particular, do not
            // carry a prior clipboard-failure bypass or already-discarded
            // revisions into some later, unrelated quit request.
            self.exit_discarded_revisions.clear();
            self.clipboard_quit_bypass = None;
            self.force_editor_focus = true;
            cx.notify();
        }
    }

    pub(crate) fn move_quit_review_selection(&mut self, down: bool, cx: &mut Context<Self>) {
        let Some(review) = self.quit_review.as_mut().filter(|review| !review.is_running()) else {
            return;
        };
        if review.items.is_empty() {
            return;
        }
        review.selected_index = if down {
            (review.selected_index + 1).min(review.items.len() - 1)
        } else {
            review.selected_index.saturating_sub(1)
        };
        self.quit_review_scroll.scroll_to_item(review.selected_index);
        cx.notify();
    }

    pub(crate) fn toggle_quit_review_item(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(review) = self.quit_review.as_mut().filter(|review| !review.is_running()) else {
            return;
        };
        let Some(item) = review.items.get_mut(index) else {
            return;
        };
        if matches!(item.status, QuitReviewItemStatus::Saved) {
            return;
        }
        review.selected_index = index;
        item.decision = match item.decision {
            QuitReviewDecision::Save => QuitReviewDecision::Discard,
            QuitReviewDecision::Discard => QuitReviewDecision::Save,
        };
        if matches!(item.status, QuitReviewItemStatus::Failed(_)) {
            item.status = QuitReviewItemStatus::Pending;
        }
        review.message = None;
        cx.notify();
    }

    pub(crate) fn toggle_selected_quit_review_item(&mut self, cx: &mut Context<Self>) {
        let Some(index) = self.quit_review.as_ref().map(|review| review.selected_index) else {
            return;
        };
        self.toggle_quit_review_item(index, cx);
    }

    pub(crate) fn confirm_quit_review_save_selected(&mut self, cx: &mut Context<Self>) {
        let Some(review) = self.quit_review.as_mut().filter(|review| !review.is_running()) else {
            return;
        };
        review.message = None;
        for item in &mut review.items {
            if item.decision == QuitReviewDecision::Save && matches!(item.status, QuitReviewItemStatus::Failed(_)) {
                item.status = QuitReviewItemStatus::Pending;
            }
        }
        self.continue_quit_review(cx);
    }

    pub(crate) fn confirm_quit_review_discard_all(&mut self, cx: &mut Context<Self>) {
        let Some(review) = self.quit_review.as_mut().filter(|review| !review.is_running()) else {
            return;
        };
        review.message = None;
        for item in &mut review.items {
            if !matches!(item.status, QuitReviewItemStatus::Saved) {
                item.decision = QuitReviewDecision::Discard;
                item.status = QuitReviewItemStatus::Pending;
            }
        }
        let discarded = review
            .items
            .iter()
            .filter(|item| item.decision == QuitReviewDecision::Discard)
            .map(|item| item.tab_id)
            .collect::<Vec<_>>();
        let failed_scratchpad = review.failed_scratchpad.take();
        for tab_id in discarded.into_iter().chain(failed_scratchpad) {
            self.remember_exit_discard(tab_id);
        }
        self.continue_quit_scratchpads(cx);
    }

    pub(crate) fn cancel_quit_review(&mut self, cx: &mut Context<Self>) {
        if self.quit_review.as_ref().is_some_and(QuitReview::is_running) {
            return;
        }
        if self.quit_review.take().is_some() {
            self.exit_discarded_revisions.clear();
            self.clipboard_quit_bypass = None;
            self.force_editor_focus = true;
            cx.notify();
        }
    }

    fn continue_quit_review(&mut self, cx: &mut Context<Self>) {
        loop {
            let next_index = self.quit_review.as_ref().and_then(|review| {
                review.items.iter().position(|item| {
                    item.decision == QuitReviewDecision::Save && !matches!(item.status, QuitReviewItemStatus::Saved)
                })
            });
            let Some(index) = next_index else {
                let discarded = self
                    .quit_review
                    .as_ref()
                    .into_iter()
                    .flat_map(|review| &review.items)
                    .filter(|item| item.decision == QuitReviewDecision::Discard)
                    .map(|item| item.tab_id)
                    .collect::<Vec<_>>();
                for tab_id in discarded {
                    self.remember_exit_discard(tab_id);
                }
                self.continue_quit_scratchpads(cx);
                return;
            };
            let tab_id = self.quit_review.as_ref().expect("review exists").items[index].tab_id;
            let Some(tab) = self.model.tab_by_id(tab_id) else {
                self.finish_exit_save_failure_for_review(index, "Document is no longer open".to_string(), cx);
                return;
            };
            if !tab.modified() && !tab.backing_file_missing() {
                self.quit_review.as_mut().expect("review exists").items[index].status = QuitReviewItemStatus::Saved;
                continue;
            }
            self.quit_review.as_mut().expect("review exists").items[index].status = QuitReviewItemStatus::Saving;
            self.start_exit_save(tab_id, ExitSaveContinuation::QuitReview, cx);
            return;
        }
    }

    fn continue_quit_scratchpads(&mut self, cx: &mut Context<Self>) {
        if self.pending_exit_save.is_some() {
            return;
        }
        let next = self
            .model
            .tabs()
            .iter()
            .find(|tab| tab.is_scratchpad() && (tab.modified() || tab.backing_file_missing()) && !tab.is_blank())
            .filter(|tab| self.exit_discarded_revisions.get(&tab.id()) != Some(&tab.revision()))
            .map(ModelEditorTab::id);
        let Some(tab_id) = next else {
            self.finish_quit(cx);
            return;
        };
        if let Some(review) = self.quit_review.as_mut() {
            review.saving_scratchpad = true;
            review.failed_scratchpad = None;
            review.message = Some("Saving scratchpads…".to_string());
        }
        self.start_exit_save(tab_id, ExitSaveContinuation::QuitScratchpad, cx);
    }

    fn remember_exit_discard(&mut self, tab_id: TabId) {
        if let Some(revision) = self.model.tab_by_id(tab_id).map(ModelEditorTab::revision) {
            self.exit_discarded_revisions.insert(tab_id, revision);
        }
    }

    fn start_exit_save(&mut self, tab_id: TabId, continuation: ExitSaveContinuation, cx: &mut Context<Self>) {
        if self.pending_exit_save.is_some() {
            return;
        }
        self.pending_exit_save = Some(PendingExitSave { tab_id, continuation });
        self.update_model(cx, true, |model| {
            model.request_save_tab(tab_id);
        });
    }

    fn finish_quit(&mut self, cx: &mut Context<Self>) {
        // Copy-on-close is a scratchpad workflow, never an ordinary-file
        // shutdown side effect. When an application-level quit closes several
        // tabs, the active scratchpad is the only unambiguous clipboard source;
        // every other scratchpad is still archived on disk.
        let scratchpad_payload = self.active_scratchpad_clipboard_payload();
        let bypass_matches = self.clipboard_quit_bypass.as_ref() == scratchpad_payload.as_ref();
        if let Some(payload) = scratchpad_payload.as_ref().filter(|_| !bypass_matches) {
            if let Err(error) = persist_clipboards_after_exit(&payload.text) {
                self.clipboard_quit_bypass = Some(payload.clone());
                self.close_prompt = None;
                self.quit_review = None;
                self.cleanup_message = Some(format!(
                    "Could not keep the scratchpad in the system clipboard: {error}. Press Ctrl+Q again to quit anyway; the scratchpad remains archived on disk."
                ));
                self.force_editor_focus = true;
                cx.notify();
                return;
            }
        }
        self.clipboard_quit_bypass = None;
        self.archive_open_scratchpads();
        self.cleanup_empty_scratchpad_files();
        // X11 WM_DELETE_WINDOW already holds GPUI's X11 client RefCell, so defer
        // exit until the current frame releases it. Real builds rely on the
        // external clipboard owner spawned above instead of in-process writes
        // (which would re-enter that same RefCell). Tests route through GPUI's
        // `quit` so the harness can observe shutdown.
        #[cfg(test)]
        cx.defer(move |app| {
            if let Some(text) = scratchpad_payload.map(|payload| payload.text) {
                app.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                app.write_to_primary(ClipboardItem::new_string(text));
            }
            app.quit();
        });
        #[cfg(not(test))]
        cx.defer(|_| process::exit(0));
    }

    fn finish_exit_save_success(&mut self, exit_save: Option<PendingExitSave>, cx: &mut Context<Self>) {
        let Some(exit_save) = exit_save.filter(|exit_save| self.pending_exit_save == Some(*exit_save)) else {
            return;
        };
        self.pending_exit_save = None;
        match exit_save.continuation {
            ExitSaveContinuation::CloseTab => {
                if self
                    .close_prompt
                    .as_ref()
                    .is_some_and(|prompt| prompt.tab_id == exit_save.tab_id)
                {
                    self.close_prompt = None;
                }
                self.copy_scratchpad_for_tab_close(exit_save.tab_id, cx);
                self.record_closed_tab(exit_save.tab_id);
                self.update_model(cx, true, |model| {
                    model.close_clean_tab(exit_save.tab_id);
                });
            }
            ExitSaveContinuation::QuitSingle => {
                self.close_prompt = None;
                self.continue_quit_scratchpads(cx);
            }
            ExitSaveContinuation::QuitReview => {
                if let Some(item) = self
                    .quit_review
                    .as_mut()
                    .and_then(|review| review.items.iter_mut().find(|item| item.tab_id == exit_save.tab_id))
                {
                    item.status = QuitReviewItemStatus::Saved;
                }
                self.continue_quit_review(cx);
            }
            ExitSaveContinuation::QuitScratchpad => {
                if let Some(review) = self.quit_review.as_mut() {
                    review.saving_scratchpad = false;
                    review.failed_scratchpad = None;
                    review.message = None;
                }
                self.continue_quit_scratchpads(cx);
            }
        }
    }

    fn finish_exit_save_failure(
        &mut self,
        exit_save: Option<PendingExitSave>,
        message: String,
        cx: &mut Context<Self>,
    ) {
        let Some(exit_save) = exit_save.filter(|exit_save| self.pending_exit_save == Some(*exit_save)) else {
            return;
        };
        self.pending_exit_save = None;
        match exit_save.continuation {
            ExitSaveContinuation::CloseTab | ExitSaveContinuation::QuitSingle => {
                let intent = if exit_save.continuation == ExitSaveContinuation::CloseTab {
                    ClosePromptIntent::CloseTab
                } else {
                    ClosePromptIntent::Quit
                };
                if let Some(prompt) = self
                    .close_prompt
                    .as_mut()
                    .filter(|prompt| prompt.tab_id == exit_save.tab_id)
                {
                    prompt.status = ClosePromptStatus::Failed(message);
                } else {
                    self.close_prompt = Some(ClosePrompt {
                        tab_id: exit_save.tab_id,
                        intent,
                        status: ClosePromptStatus::Failed(message),
                    });
                }
            }
            ExitSaveContinuation::QuitReview => {
                let index = self
                    .quit_review
                    .as_ref()
                    .and_then(|review| review.items.iter().position(|item| item.tab_id == exit_save.tab_id));
                if let Some(index) = index {
                    self.finish_exit_save_failure_for_review(index, message, cx);
                    return;
                }
            }
            ExitSaveContinuation::QuitScratchpad => {
                if let Some(review) = self.quit_review.as_mut() {
                    review.saving_scratchpad = false;
                    review.failed_scratchpad = Some(exit_save.tab_id);
                    review.message = Some(format!("Could not save scratchpad: {message}"));
                } else {
                    self.close_prompt = Some(ClosePrompt {
                        tab_id: exit_save.tab_id,
                        intent: ClosePromptIntent::Quit,
                        status: ClosePromptStatus::Failed(message),
                    });
                }
            }
        }
        cx.notify();
    }

    fn finish_exit_save_failure_for_review(&mut self, index: usize, message: String, cx: &mut Context<Self>) {
        if let Some(review) = self.quit_review.as_mut() {
            if let Some(item) = review.items.get_mut(index) {
                item.status = QuitReviewItemStatus::Failed(message);
            }
            review.message = Some("Some selected files could not be saved.".to_string());
        }
        cx.notify();
    }

    fn copy_scratchpad_for_tab_close(&self, tab_id: TabId, cx: &mut Context<Self>) {
        let Some(tab) = self.model.tab_by_id(tab_id).filter(|tab| tab.is_scratchpad()) else {
            return;
        };
        let text = tab.buffer_text();
        if text.trim().is_empty() {
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
        cx.write_to_primary(ClipboardItem::new_string(text));
    }

    fn archive_open_scratchpads(&mut self) {
        let active_id = self.model.active_tab_id();
        let mut paths = self
            .model
            .tabs()
            .iter()
            .filter(|tab| tab.is_scratchpad() && !tab.is_blank())
            .filter_map(|tab| Some((tab.id(), tab.path()?.clone())))
            .collect::<Vec<_>>();
        paths.sort_by_key(|(tab_id, _)| *tab_id == active_id);
        for (_, path) in paths {
            self.recent.record_with_origin(&path, RecentOrigin::Scratchpad);
        }
    }

    fn apply_open_file_results(&mut self, (opened, failed): OpenFileResults, cx: &mut Context<Self>) {
        let failures = failed
            .into_iter()
            .map(|(path, message)| format!("{}: {message}", path.display()))
            .collect::<Vec<_>>();

        for (path, text, stamp) in opened {
            let normalized = normalize_recent_path(&path);
            let existing = self.model.tabs().iter().find_map(|tab| {
                let tab_path = tab.path()?;
                (normalize_recent_path(tab_path) == normalized).then_some(tab.id())
            });
            if let Some(tab_id) = existing {
                self.update_model(cx, true, |model| model.set_active_tab(tab_id));
            } else {
                let opened_path = path.clone();
                self.update_model(cx, true, |model| {
                    model.open_files_with_stamps(vec![(opened_path, text, stamp)]);
                });
            }
            self.recent.record_with_origin(&path, RecentOrigin::Regular);
        }

        if !failures.is_empty() {
            self.cleanup_message = Some(format!("Some files could not be opened: {}", failures.join("; ")));
            cx.notify();
        }
    }

    fn apply_save_outcome(
        &mut self,
        result: FileWriteOutcome,
        kind: SaveKind,
        ticket: SaveTicket,
        cx: &mut Context<Self>,
    ) {
        let inflight_path = result.path().to_path_buf();
        self.finish_save_inflight(&inflight_path);
        if !ticket.is_current() {
            self.finish_exit_save_failure(
                kind.exit_save(),
                "A newer save superseded this save attempt".to_string(),
                cx,
            );
            self.start_queued_save_for_path(&inflight_path, cx);
            return;
        }
        let is_save_as = matches!(kind, SaveKind::SaveAs { .. });
        let exit_save = kind.exit_save();
        match result {
            FileWriteOutcome::Written {
                tab_id,
                path,
                revision,
                stamp,
                body,
            } => {
                let saved_path = path.clone();
                let mut saved = false;
                let mut record_recent = false;
                self.update_model(cx, true, |model| {
                    saved = if is_save_as {
                        model.save_as_finished_for_tab(tab_id, path, revision, stamp, body)
                    } else {
                        model.save_finished_for_tab(tab_id, path, revision, stamp, body)
                    };
                    record_recent =
                        saved && (is_save_as || model.tab_by_id(tab_id).is_some_and(|tab| !tab.is_scratchpad()));
                });
                if record_recent {
                    self.recent.record_with_origin(&saved_path, RecentOrigin::Regular);
                }
                if saved {
                    self.file_conflicts.remove(&tab_id);
                }
                if let SaveKind::SaveAs {
                    previous_scratchpad, ..
                } = kind
                {
                    if saved {
                        if let Some(previous) = previous_scratchpad.as_deref() {
                            if normalize_recent_path(previous) != normalize_recent_path(&saved_path) {
                                self.recent.prune_path(previous);
                            }
                        }
                        remove_previous_scratchpad_after_save_as(previous_scratchpad, &saved_path, self.model.tabs());
                    }
                }
                if saved {
                    self.finish_exit_save_success(exit_save, cx);
                } else {
                    self.finish_exit_save_failure(exit_save, "Document changed while saving".to_string(), cx);
                }
            }
            FileWriteOutcome::Failed { tab_id, path, message } => {
                let failure = message.clone();
                self.update_model(cx, true, |model| {
                    model.save_failed(path, message);
                });
                let _ = tab_id;
                self.finish_exit_save_failure(exit_save, failure, cx);
            }
            FileWriteOutcome::Conflict {
                tab_id,
                path,
                revision,
                disk_stamp,
            } => {
                self.handle_file_conflict(
                    tab_id,
                    path,
                    disk_stamp,
                    ConflictWrite::Save { revision, exit_save },
                    cx,
                );
            }
            FileWriteOutcome::Stale { .. } => {
                self.finish_exit_save_failure(exit_save, "A newer save superseded this save attempt".to_string(), cx);
                cx.notify();
            }
        }
        self.start_queued_save_for_path(&inflight_path, cx);
    }

    fn apply_autosave_completion(&mut self, completion: FileWriteOutcome, cx: &mut Context<Self>) {
        match completion {
            FileWriteOutcome::Written {
                tab_id,
                path,
                revision,
                stamp,
                body,
            } => {
                let recent_path = path.clone();
                let mut saved = false;
                let mut record_recent = false;
                self.update_model(cx, true, |model| {
                    saved = model.autosave_finished_for_tab(tab_id, path, revision, stamp, body);
                    record_recent = saved && model.tab_by_id(tab_id).is_some_and(|tab| !tab.is_scratchpad());
                });
                if saved {
                    self.file_conflicts.remove(&tab_id);
                }
                if record_recent {
                    self.recent.record_with_origin(&recent_path, RecentOrigin::Regular);
                }
            }
            FileWriteOutcome::Failed {
                tab_id: _,
                path,
                message,
            } => {
                self.update_model(cx, true, |model| {
                    model.autosave_failed(path, message);
                });
            }
            FileWriteOutcome::Conflict {
                tab_id,
                path,
                revision,
                disk_stamp,
            } => self.handle_file_conflict(tab_id, path, disk_stamp, ConflictWrite::Autosave { revision }, cx),
            // A newer explicit save may invalidate an older autosave ticket.
            FileWriteOutcome::Stale { .. } => {}
        }
    }

    pub(crate) fn request_new_tab(&mut self, cx: &mut Context<Self>) {
        let directory = self.scratchpad_dir_override().map(Path::to_path_buf);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { create_scratchpad_note(directory.as_deref()) })
                .await;
            let _ = this.update(cx, |view, cx| match result {
                Ok((path, file_stamp)) => {
                    view.update_model(cx, true, |model| {
                        model.new_scratchpad_tab(path, file_stamp);
                    });
                }
                Err(err) => {
                    view.update_model(cx, true, |model| {
                        model.new_tab();
                        model.save_failed(PathBuf::from("scratchpad"), err.to_string());
                    });
                }
            });
        })
        .detach();
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

    /// Snapshot a closing path-backed tab so `Ctrl+Shift+T` can reopen it.
    /// Non-empty scratchpads are also archived into unified recent history.
    fn record_closed_tab(&mut self, tab_id: TabId) {
        const MAX_CLOSED_HISTORY: usize = 32;
        let Some((path, position, origin, blank)) = self.model.tab_by_id(tab_id).and_then(|tab| {
            Some((
                tab.path()?.clone(),
                tab.cursor_position(),
                if tab.is_scratchpad() {
                    RecentOrigin::Scratchpad
                } else {
                    RecentOrigin::Regular
                },
                tab.is_blank(),
            ))
        }) else {
            return;
        };
        if origin == RecentOrigin::Scratchpad && blank {
            return;
        }
        if origin == RecentOrigin::Scratchpad {
            self.recent.record_with_origin(&path, RecentOrigin::Scratchpad);
        }
        self.closed_tabs_history
            .push(crate::ClosedTabRecord { path, position, origin });
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
            .map(|tab| {
                let origin = if tab.is_scratchpad() {
                    RecentOrigin::Scratchpad
                } else {
                    RecentOrigin::Regular
                };
                (tab.id(), origin)
            });
        if let Some((tab_id, origin)) = existing {
            self.update_model(cx, true, |model| {
                model.set_active_tab(tab_id);
                model.set_active_cursor_position(line, column);
            });
            self.recent.record_with_origin(&record.path, origin);
            return;
        }
        cx.spawn(async move |this, cx| {
            let read_path = record.path.clone();
            let result = cx
                .background_executor()
                .spawn(async move { read_file_with_stamp(&read_path) })
                .await;
            let _ = this.update(cx, |view, cx| match result {
                Ok((text, stamp)) => {
                    if let Some((tab_id, origin)) = view
                        .model
                        .tabs()
                        .iter()
                        .find(|tab| tab.path() == Some(&record.path))
                        .map(|tab| {
                            (
                                tab.id(),
                                if tab.is_scratchpad() {
                                    RecentOrigin::Scratchpad
                                } else {
                                    RecentOrigin::Regular
                                },
                            )
                        })
                    {
                        view.update_model(cx, true, |model| {
                            model.set_active_tab(tab_id);
                            model.set_active_cursor_position(line, column);
                        });
                        view.recent.record_with_origin(&record.path, origin);
                        return;
                    }
                    let opened_path = record.path.clone();
                    view.update_model(cx, true, |model| {
                        match record.origin {
                            RecentOrigin::Regular => {
                                model.open_files_with_stamps(vec![(opened_path, text, Some(stamp))]);
                            }
                            RecentOrigin::Scratchpad => {
                                model.new_scratchpad_tab(opened_path.clone(), stamp);
                                let tab_id = model.active_tab_id();
                                model.reload_tab_from_disk(tab_id, opened_path, text, stamp);
                            }
                        }
                        model.set_active_cursor_position(line, column);
                    });
                    view.recent.record_with_origin(&record.path, record.origin);
                }
                Err(err) => {
                    let failed_path = record.path;
                    view.update_model(cx, true, |model| {
                        model.open_file_failed(failed_path, err.to_string());
                    });
                }
            });
        })
        .detach();
    }
}

pub(crate) fn tab_identity(tab: &ModelEditorTab) -> String {
    tab.path()
        .map(|path| normalize_recent_path(path).to_string_lossy().into_owned())
        .unwrap_or_else(|| format!("{} (unsaved)", tab.display_name()))
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
    expectation: SaveExpectation,
    ticket: Option<&SaveTicket>,
) -> io::Result<AtomicWriteOutcome> {
    if ticket.is_some_and(|ticket| !ticket.is_current()) {
        return Ok(AtomicWriteOutcome::Stale);
    }

    if let Some(disk_stamp) = save_conflict_stamp(path, expectation)? {
        return Ok(AtomicWriteOutcome::Conflict(disk_stamp));
    }
    ensure_existing_target_is_writable(path)?;

    let write_target = write_target_for_path(path)?;
    let permissions = fs::metadata(path).ok().map(|metadata| metadata.permissions());
    let temp_path = write_temp_replacement(&write_target, bytes, permissions)?;

    commit_temp_replacement(path, &write_target, &temp_path, expectation, ticket)
}

fn commit_temp_replacement(
    path: &Path,
    write_target: &Path,
    temp_path: &Path,
    expectation: SaveExpectation,
    ticket: Option<&SaveTicket>,
) -> io::Result<AtomicWriteOutcome> {
    let _current = match ticket {
        Some(ticket) => match ticket.current_guard() {
            Some(guard) => Some(guard),
            None => {
                remove_temp_file(temp_path);
                return Ok(AtomicWriteOutcome::Stale);
            }
        },
        None => None,
    };

    match save_conflict_stamp(path, expectation) {
        Ok(Some(disk_stamp)) => {
            remove_temp_file(temp_path);
            return Ok(AtomicWriteOutcome::Conflict(disk_stamp));
        }
        Ok(None) => {}
        Err(error) => {
            remove_temp_file(temp_path);
            return Err(error);
        }
    }

    if let Err(err) = fs::rename(temp_path, write_target) {
        remove_temp_file(temp_path);
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
    link_path.parent().unwrap_or_else(|| Path::new(".")).join(target)
}

fn write_temp_replacement(target: &Path, bytes: &[u8], permissions: Option<fs::Permissions>) -> io::Result<PathBuf> {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    for _ in 0..128 {
        let temp_path = replacement_temp_path(parent, target.file_name());
        match fs::OpenOptions::new().write(true).create_new(true).open(&temp_path) {
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

fn stale_save_result(tab_id: TabId, path: PathBuf, revision: u64) -> FileWriteOutcome {
    FileWriteOutcome::Stale { tab_id, path, revision }
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
    (opened, failed)
}

fn observe_external_files(requests: Vec<ExternalFileRequest>) -> Vec<ExternalFileObservation> {
    requests
        .into_iter()
        .map(|request| match file_stamp(&request.path) {
            Ok(stamp) if Some(stamp) == request.expected_stamp => ExternalFileObservation::Unchanged(request),
            Ok(_) => match read_file_with_stamp(&request.path) {
                Ok((text, disk_stamp)) => ExternalFileObservation::Changed {
                    request,
                    text,
                    disk_stamp,
                },
                Err(error) if error.kind() == io::ErrorKind::NotFound => ExternalFileObservation::Missing(request),
                Err(error) => ExternalFileObservation::Failed {
                    request,
                    message: error.to_string(),
                },
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound && request.expected_stamp.is_none() => {
                ExternalFileObservation::Unchanged(request)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => ExternalFileObservation::Missing(request),
            Err(error) => ExternalFileObservation::Failed {
                request,
                message: error.to_string(),
            },
        })
        .collect()
}

fn save_file_result(
    tab_id: TabId,
    path: PathBuf,
    body: String,
    revision: u64,
    expectation: SaveExpectation,
    ticket: SaveTicket,
    options: SaveTextOptions,
) -> FileWriteOutcome {
    if !ticket.is_current() {
        return stale_save_result(tab_id, path, revision);
    }
    let body = apply_save_options(body, options);

    match save_conflict_stamp(&path, expectation) {
        Ok(Some(disk_stamp)) => {
            if fs::read_to_string(&path).is_ok_and(|disk_body| disk_body == body) {
                return FileWriteOutcome::Written {
                    tab_id,
                    path,
                    revision,
                    stamp: disk_stamp,
                    body,
                };
            }
            return FileWriteOutcome::Conflict {
                tab_id,
                path,
                revision,
                disk_stamp,
            };
        }
        Ok(None) => {}
        Err(err) => {
            return FileWriteOutcome::Failed {
                tab_id,
                path,
                message: err.to_string(),
            };
        }
    }
    if !ticket.is_current() {
        return stale_save_result(tab_id, path, revision);
    }
    write_file_result(tab_id, path, body, revision, expectation, ticket)
}

fn write_file_result(
    tab_id: TabId,
    path: PathBuf,
    body: String,
    revision: u64,
    expectation: SaveExpectation,
    ticket: SaveTicket,
) -> FileWriteOutcome {
    match write_file_with_guards(&path, body.as_bytes(), expectation, Some(&ticket)) {
        Ok(AtomicWriteOutcome::Written) => match file_stamp(&path) {
            Ok(stamp) => FileWriteOutcome::Written {
                tab_id,
                path,
                revision,
                stamp,
                body,
            },
            Err(err) => FileWriteOutcome::Failed {
                tab_id,
                path,
                message: err.to_string(),
            },
        },
        Ok(AtomicWriteOutcome::Stale) => stale_save_result(tab_id, path, revision),
        Ok(AtomicWriteOutcome::Conflict(disk_stamp)) => FileWriteOutcome::Conflict {
            tab_id,
            path,
            revision,
            disk_stamp,
        },
        Err(err) => FileWriteOutcome::Failed {
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
    expectation: SaveExpectation,
    ticket: SaveTicket,
) -> FileWriteOutcome {
    if !ticket.is_current() {
        return stale_save_result(tab_id, path, revision);
    }
    match save_conflict_stamp(&path, expectation) {
        Ok(Some(disk_stamp)) if fs::read_to_string(&path).is_ok_and(|disk_body| disk_body == body) => {
            return FileWriteOutcome::Written {
                tab_id,
                path,
                revision,
                stamp: disk_stamp,
                body,
            };
        }
        Ok(Some(disk_stamp)) => {
            return FileWriteOutcome::Conflict {
                tab_id,
                path,
                revision,
                disk_stamp,
            };
        }
        Ok(None) => {}
        Err(error) => {
            return FileWriteOutcome::Failed {
                tab_id,
                path,
                message: error.to_string(),
            };
        }
    }
    if !ticket.is_current() {
        return stale_save_result(tab_id, path, revision);
    }
    match write_file_with_guards(&path, body.as_bytes(), expectation, Some(&ticket)) {
        Ok(AtomicWriteOutcome::Written) => match file_stamp(&path) {
            Ok(stamp) => FileWriteOutcome::Written {
                tab_id,
                path,
                revision,
                stamp,
                body,
            },
            Err(err) => FileWriteOutcome::Failed {
                tab_id,
                path,
                message: err.to_string(),
            },
        },
        Ok(AtomicWriteOutcome::Stale) => stale_save_result(tab_id, path, revision),
        Ok(AtomicWriteOutcome::Conflict(disk_stamp)) => FileWriteOutcome::Conflict {
            tab_id,
            path,
            revision,
            disk_stamp,
        },
        Err(err) => FileWriteOutcome::Failed {
            tab_id,
            path,
            message: err.to_string(),
        },
    }
}

pub(crate) fn read_file_with_stamp(path: &Path) -> std::io::Result<(String, FileStamp)> {
    for _ in 0..3 {
        let before = file_stamp(path)?;
        let text = fs::read_to_string(path)?;
        let after = file_stamp(path)?;
        if before == after {
            return Ok((text, after));
        }
    }
    Err(io::Error::new(
        io::ErrorKind::WouldBlock,
        "file kept changing while it was being read",
    ))
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

fn apply_save_options(body: String, options: SaveTextOptions) -> String {
    if !options.trim_trailing_whitespace && !options.ensure_final_newline {
        return body;
    }
    let mut body = if options.trim_trailing_whitespace {
        trim_trailing_ws(&body)
    } else {
        body
    };
    if options.ensure_final_newline && !body.is_empty() && !body.ends_with('\n') {
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

pub(crate) fn autosave_revision_is_current(tabs: &[ModelEditorTab], tab_id: TabId, path: &Path, revision: u64) -> bool {
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

fn save_conflict_stamp(path: &Path, expectation: SaveExpectation) -> std::io::Result<Option<FileStamp>> {
    match expectation {
        SaveExpectation::Unguarded => Ok(None),
        SaveExpectation::Absent => match file_stamp(path) {
            Ok(stamp) => Ok(Some(stamp)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        },
        SaveExpectation::Matching(expected_stamp) => {
            let disk_stamp = match file_stamp(path) {
                Ok(stamp) => stamp,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    // A deleted file has no current stamp. Reuse the last known
                    // stamp as a stable key for the conflict UI.
                    return Ok(Some(expected_stamp));
                }
                Err(error) => return Err(error),
            };
            Ok((disk_stamp != expected_stamp).then_some(disk_stamp))
        }
    }
}

impl LstGpuiApp {
    pub(crate) fn start_cleanup(&mut self, cx: &mut Context<Self>) {
        if self.cleanup_in_flight || self.cleanup_confirmation.is_some() {
            return;
        }
        let tab = self.active_tab();
        let tab_id = tab.id();
        let revision = tab.revision();
        if tab.has_selection() {
            let range = tab.selected_range();
            match tab.selected_text() {
                Some(text) if !text.is_empty() => self.begin_cleanup(tab_id, revision, range, text, cx),
                _ => {
                    self.cleanup_message = Some("Nothing to clean up.".to_string());
                    cx.notify();
                }
            }
            return;
        }
        if tab.buffer().len_chars() == 0 {
            self.cleanup_message = Some("Nothing to clean up.".to_string());
            cx.notify();
            return;
        }
        self.cleanup_confirmation = Some(crate::CleanupConfirmation { tab_id, revision });
        cx.notify();
    }

    pub(crate) fn confirm_cleanup_whole_document(&mut self, cx: &mut Context<Self>) {
        let Some(confirmation) = self.cleanup_confirmation.take() else {
            return;
        };
        let Some(tab) = self.model.tab_by_id(confirmation.tab_id) else {
            cx.notify();
            return;
        };
        if self.model.active_tab_id() != confirmation.tab_id || tab.revision() != confirmation.revision {
            self.cleanup_message = Some("Document changed; cleanup was cancelled.".to_string());
            cx.notify();
            return;
        }
        let range = 0..tab.buffer().len_chars();
        let source_text = tab.buffer_text();
        self.begin_cleanup(confirmation.tab_id, confirmation.revision, range, source_text, cx);
    }

    pub(crate) fn cancel_cleanup_confirmation(&mut self, cx: &mut Context<Self>) {
        if self.cleanup_confirmation.take().is_some() {
            cx.notify();
        }
    }

    fn begin_cleanup(
        &mut self,
        tab_id: TabId,
        revision: u64,
        range: Range<usize>,
        source_text: String,
        cx: &mut Context<Self>,
    ) {
        let client = match build_llm_client() {
            Ok(client) => client,
            Err(message) => {
                self.cleanup_message = Some(message);
                cx.notify();
                return;
            }
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
            self.cleanup_message = Some("Buffer changed during cleanup; result discarded.".to_string());
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
    if let Some(canned) = std::env::var("LST_LLM_FAKE_RESPONSE").ok().filter(|s| !s.is_empty()) {
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
    Ok(Box::new(crate::llm::DeepSeekClient::new(api_key, model_name)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deferred_latest_save_uses_the_stamp_from_an_older_committed_save() {
        static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

        let directory = env::temp_dir().join(format!(
            "lst-serialized-save-{}-{}",
            process::id(),
            TEST_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&directory).expect("create test directory");
        let path = directory.join("document.txt");
        fs::write(&path, "initial\n").expect("seed target");
        let initial_stamp = file_stamp(&path).expect("stamp initial target");
        let tab_id = TabId::from_raw(1);
        let tab = ModelEditorTab::from_path_with_stamp(tab_id, path.clone(), "initial\n", Some(initial_stamp));
        let mut model = lst_editor::EditorModel::from_tabs(tab, Vec::new(), "Ready.".to_string());

        model.execute(lst_editor::EditorCommand::SelectAll);
        model.replace_text(None, "older requested body\n".to_string(), UndoBoundary::Break);
        let older_revision = model.active_tab().revision();
        let older_body = model.active_tab().buffer_text();

        model.execute(lst_editor::EditorCommand::SelectAll);
        model.replace_text(None, "latest requested body\n".to_string(), UndoBoundary::Break);
        let latest_revision = model.active_tab().revision();
        let latest_body = model.active_tab().buffer_text();

        let generation = Arc::new(Mutex::new(0));
        let older = save_file_result(
            tab_id,
            path.clone(),
            older_body,
            older_revision,
            SaveExpectation::Matching(initial_stamp),
            SaveTicket::issue(&generation),
            SaveTextOptions {
                trim_trailing_whitespace: false,
                ensure_final_newline: false,
            },
        );
        let FileWriteOutcome::Written {
            path: written_path,
            revision,
            stamp: older_stamp,
            body,
            ..
        } = older
        else {
            panic!("older save should commit");
        };
        assert!(!model.save_finished_for_tab(tab_id, written_path, revision, older_stamp, body));
        assert!(model.active_tab().modified());

        let latest = save_file_result(
            tab_id,
            path.clone(),
            latest_body,
            latest_revision,
            model.active_tab().save_expectation(),
            SaveTicket::issue(&generation),
            SaveTextOptions {
                trim_trailing_whitespace: false,
                ensure_final_newline: false,
            },
        );
        let FileWriteOutcome::Written {
            path: written_path,
            revision,
            stamp,
            body,
            ..
        } = latest
        else {
            panic!("latest save should not conflict with the app's older write");
        };
        assert!(model.save_finished_for_tab(tab_id, written_path, revision, stamp, body));
        assert_eq!(
            fs::read_to_string(&path).expect("read final target"),
            "latest requested body\n"
        );
        assert!(!model.active_tab().modified());

        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[test]
    fn final_atomic_guard_preserves_a_target_recreated_after_expected_absence() {
        static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

        let directory = env::temp_dir().join(format!(
            "lst-expected-absence-{}-{}",
            process::id(),
            TEST_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&directory).expect("create test directory");
        let target = directory.join("document.txt");
        let temp_path = write_temp_replacement(&target, b"editor copy\n", None).expect("stage replacement");

        fs::write(&target, "recreated elsewhere\n").expect("recreate target");
        let recreated_stamp = file_stamp(&target).expect("stamp recreated target");
        let generation = Arc::new(Mutex::new(0));
        let ticket = SaveTicket::issue(&generation);
        let outcome = commit_temp_replacement(&target, &target, &temp_path, SaveExpectation::Absent, Some(&ticket))
            .expect("run final guard");

        assert_eq!(outcome, AtomicWriteOutcome::Conflict(recreated_stamp));
        assert_eq!(
            fs::read_to_string(&target).expect("read preserved target"),
            "recreated elsewhere\n"
        );
        assert!(!temp_path.exists(), "rejected replacement should be removed");
        fs::remove_dir_all(directory).expect("remove test directory");
    }
}
