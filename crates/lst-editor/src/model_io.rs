use crate::{EditorEffect, EditorModel, EditorTab, FileStamp, RevealIntent, TabId};
use std::path::PathBuf;

impl EditorModel {
    pub fn request_open_files(&mut self) {
        self.queue_effect(EditorEffect::OpenFiles);
    }

    pub fn open_files_with_stamps(&mut self, files: Vec<(PathBuf, String, Option<FileStamp>)>) {
        let mut opened = 0;
        let mut last_opened = None;
        for (path, text, file_stamp) in files {
            let id = self.alloc_tab_id();
            last_opened = Some(
                self.tabs
                    .push(EditorTab::from_path_with_stamp(id, path, &text, file_stamp)),
            );
            opened += 1;
        }
        if let Some(index) = last_opened {
            self.activate_tab(index);
            self.status = format!("Opened {opened} tab(s).");
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
            self.queue_effect(EditorEffect::SaveFileAs {
                tab_id,
                suggested_name: tab.display_name(),
                body,
                previous_scratchpad_path: tab.scratchpad_path().cloned(),
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
        self.queue_effect(EditorEffect::SaveFileAs {
            tab_id,
            suggested_name: tab.display_name(),
            body: tab.buffer_text(),
            previous_scratchpad_path: tab.scratchpad_path().cloned(),
        });
    }

    pub fn save_finished_for_tab(&mut self, tab_id: TabId, path: PathBuf, file_stamp: FileStamp) {
        if let Some(tab) = self.tab_mut_by_id(tab_id) {
            tab.mark_saved(path.clone(), file_stamp);
            self.status = format!("Saved {}.", path.display());
        }
    }

    pub fn save_as_finished_for_tab(
        &mut self,
        tab_id: TabId,
        path: PathBuf,
        file_stamp: FileStamp,
    ) {
        if let Some(tab) = self.tab_mut_by_id(tab_id) {
            tab.mark_saved_as(path.clone(), file_stamp);
            self.status = format!("Saved {}.", path.display());
        }
    }

    pub fn save_failed(&mut self, path: PathBuf, message: String) {
        self.status = format!("Failed to save {}: {message}", path.display());
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

    pub fn autosave_finished_for_tab(
        &mut self,
        tab_id: TabId,
        path: PathBuf,
        revision: u64,
        file_stamp: FileStamp,
    ) {
        let active_id = self.active_tab_id();
        let Some(tab) = self.tab_mut_by_id(tab_id) else {
            return;
        };
        if tab.path() != Some(&path) || tab.revision() != revision {
            return;
        }
        tab.mark_autosaved(file_stamp);
        if tab_id == active_id {
            self.status = format!("Autosaved {}.", path.display());
        }
    }

    pub fn autosave_failed(&mut self, path: PathBuf, message: String) {
        self.status = format!("Autosave failed for {}: {message}", path.display());
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

    pub fn reload_failed(&mut self, path: PathBuf, message: String) {
        self.status = format!("Failed to reload {}: {message}", path.display());
    }

    pub fn suppress_file_conflict(&mut self, tab_id: TabId, path: PathBuf, stamp: FileStamp) {
        if let Some(tab) = self.tab_mut_by_id(tab_id) {
            tab.suppress_file_conflict(stamp);
            self.status = format!("Kept local changes for {}.", path.display());
        }
    }
}
