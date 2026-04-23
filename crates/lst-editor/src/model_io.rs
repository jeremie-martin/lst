use crate::{EditorEffect, EditorModel, EditorTab, FileStamp, RevealIntent, TabId};
use std::path::PathBuf;

impl EditorModel {
    pub fn request_open_files(&mut self) {
        self.queue_effect(EditorEffect::OpenFiles);
    }

    pub fn open_files(&mut self, files: Vec<(PathBuf, String)>) {
        self.open_files_with_stamps(
            files
                .into_iter()
                .map(|(path, text)| (path, text, None))
                .collect(),
        );
    }

    pub fn open_files_with_stamps(&mut self, files: Vec<(PathBuf, String, Option<FileStamp>)>) {
        let start_len = self.tabs.len();
        for (path, text, file_stamp) in files {
            let id = self.alloc_tab_id();
            self.tabs
                .push(EditorTab::from_path_with_stamp(id, path, &text, file_stamp));
        }
        if self.tabs.len() > start_len {
            self.activate_tab(self.tabs.len() - 1);
            self.status = format!("Opened {} tab(s).", self.tabs.len() - start_len);
            self.queue_reveal(RevealIntent::NearestEdge);
        }
    }

    pub fn open_file_failed(&mut self, path: PathBuf, message: String) {
        self.status = format!("Failed to open {}: {message}", path.display());
    }

    pub fn request_save(&mut self) {
        self.request_save_tab(self.active_tab_id());
    }

    pub fn request_save_tab(&mut self, tab_id: TabId) {
        let Some(tab) = self.tab_by_id(tab_id) else {
            return;
        };
        let body = tab.buffer_text();
        if let Some(path) = tab.path().cloned() {
            self.queue_effect(EditorEffect::SaveFile {
                tab_id,
                path,
                body,
                expected_stamp: tab.file_stamp(),
            });
        } else {
            let previous_scratchpad_path = if tab.is_scratchpad() {
                tab.path().cloned()
            } else {
                None
            };
            self.queue_effect(EditorEffect::SaveFileAs {
                tab_id,
                suggested_name: tab.display_name(),
                body,
                previous_scratchpad_path,
            });
        }
    }

    pub fn request_save_as(&mut self) {
        self.request_save_as_tab(self.active_tab_id());
    }

    pub fn request_save_as_tab(&mut self, tab_id: TabId) {
        let Some(tab) = self.tab_by_id(tab_id) else {
            return;
        };
        let previous_scratchpad_path = if tab.is_scratchpad() {
            tab.path().cloned()
        } else {
            None
        };
        self.queue_effect(EditorEffect::SaveFileAs {
            tab_id,
            suggested_name: tab.display_name(),
            body: tab.buffer_text(),
            previous_scratchpad_path,
        });
    }

    pub fn save_finished(&mut self, path: PathBuf) {
        let tab_id = self.active_tab_id();
        self.save_finished_for_tab(tab_id, path, None);
    }

    pub fn save_finished_for_tab(
        &mut self,
        tab_id: TabId,
        path: PathBuf,
        file_stamp: Option<FileStamp>,
    ) {
        if let Some(tab) = self.tab_mut_by_id(tab_id) {
            if let Some(file_stamp) = file_stamp {
                tab.mark_saved(path.clone(), file_stamp);
            } else {
                tab.mark_clean_at_path(path.clone());
            }
            self.status = format!("Saved {}.", path.display());
        }
    }

    pub fn save_as_finished_for_tab(
        &mut self,
        tab_id: TabId,
        path: PathBuf,
        file_stamp: Option<FileStamp>,
    ) {
        if let Some(tab) = self.tab_mut_by_id(tab_id) {
            if let Some(file_stamp) = file_stamp {
                tab.mark_saved_as(path.clone(), file_stamp);
            } else {
                tab.mark_clean_file_at_path(path.clone());
            }
            self.status = format!("Saved {}.", path.display());
        }
    }

    pub fn save_failed(&mut self, path: PathBuf, message: String) {
        self.status = format!("Failed to save {}: {message}", path.display());
    }

    pub fn save_failed_for_tab(&mut self, tab_id: TabId, path: PathBuf, message: String) {
        if self.tab_by_id(tab_id).is_some() {
            self.status = format!("Failed to save {}: {message}", path.display());
        }
    }

    pub fn autosave_tick(&mut self) {
        let jobs = self
            .tabs
            .iter()
            .filter(|tab| tab.modified())
            .filter_map(|tab| {
                let path = tab.path().cloned()?;
                let open_tabs_for_path = self
                    .tabs
                    .iter()
                    .filter(|candidate| candidate.path() == Some(&path))
                    .take(2)
                    .count();
                if open_tabs_for_path != 1 {
                    return None;
                }
                Some((
                    tab.id(),
                    path,
                    tab.buffer_text(),
                    tab.revision(),
                    tab.file_stamp(),
                ))
            })
            .collect::<Vec<_>>();
        for (tab_id, path, body, revision, expected_stamp) in jobs {
            self.queue_effect(EditorEffect::AutosaveFile {
                tab_id,
                path,
                body,
                revision,
                expected_stamp,
            });
        }
    }

    pub fn autosave_finished(&mut self, path: PathBuf, revision: u64) {
        let Some(tab_id) = self
            .tabs
            .iter()
            .find(|tab| tab.path() == Some(&path) && tab.revision() == revision)
            .map(EditorTab::id)
        else {
            return;
        };
        self.autosave_finished_for_tab(tab_id, path, revision, None);
    }

    pub fn autosave_finished_for_tab(
        &mut self,
        tab_id: TabId,
        path: PathBuf,
        revision: u64,
        file_stamp: Option<FileStamp>,
    ) {
        for tab in &mut self.tabs {
            if tab.id() == tab_id && tab.path() == Some(&path) && tab.revision() == revision {
                tab.mark_autosaved(file_stamp);
            }
        }
        if self.active_tab().id() == tab_id
            && self.active_tab().path() == Some(&path)
            && self.active_tab().revision() == revision
        {
            self.status = format!("Autosaved {}.", path.display());
        }
    }

    pub fn autosave_failed(&mut self, path: PathBuf, message: String) {
        if self.active_tab().path() == Some(&path) {
            self.status = format!("Autosave failed for {}: {message}", path.display());
        }
    }

    pub fn autosave_failed_for_tab(&mut self, tab_id: TabId, path: PathBuf, message: String) {
        if self.active_tab().id() == tab_id {
            self.status = format!("Autosave failed for {}: {message}", path.display());
        }
    }

    pub fn reload_tab_from_disk(
        &mut self,
        tab_id: TabId,
        path: PathBuf,
        text: String,
        file_stamp: FileStamp,
    ) -> bool {
        let Some(tab) = self.tab_mut_by_id(tab_id) else {
            return false;
        };
        tab.reset_from_disk_at_path(path.clone(), &text, file_stamp);
        self.sync_find_with_active_document();
        self.status = format!("Reloaded {}.", path.display());
        true
    }

    pub fn reload_failed(&mut self, tab_id: TabId, path: PathBuf, message: String) {
        if self.tab_by_id(tab_id).is_some() {
            self.status = format!("Failed to reload {}: {message}", path.display());
        }
    }

    pub fn suppress_file_conflict(&mut self, tab_id: TabId, path: PathBuf, stamp: FileStamp) {
        if let Some(tab) = self.tab_mut_by_id(tab_id) {
            tab.suppress_file_conflict(stamp);
            self.status = format!("Kept local changes for {}.", path.display());
        }
    }
}
