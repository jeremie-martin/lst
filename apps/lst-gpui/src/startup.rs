//! File preparation can overlap platform initialization. Scratchpad creation
//! stays at the window boundary so a failed platform launch creates no notes.

use std::path::Path;

use lst_editor::{EditorModel, EditorTab, TabId, UNTITLED_PREFIX};

use crate::runtime::{self, FileIdentity};

pub(crate) struct LaunchFiles {
    tabs: Vec<EditorTab>,
    status: String,
}

impl LaunchFiles {
    pub(crate) fn load(paths: &[std::path::PathBuf]) -> Self {
        let mut tabs = Vec::new();
        let mut status = "Ready.".to_string();
        let mut opened_files = Vec::new();
        for path in paths {
            // One tab per file, as when opening documents later.
            let identity = FileIdentity::of(path);
            if opened_files.contains(&identity) {
                continue;
            }
            opened_files.push(identity);
            match runtime::read_file_with_stamp(path) {
                Ok((text, file_stamp)) => tabs.push(EditorTab::from_path_with_stamp(
                    TabId::from_raw(tabs.len() as u64 + 1),
                    path.clone(),
                    &text,
                    Some(file_stamp),
                )),
                Err(err) => status = format!("Failed to open {}: {err}", path.display()),
            }
        }
        Self { tabs, status }
    }

    pub(crate) fn into_model(mut self, scratchpad_dir: Option<&Path>) -> EditorModel {
        if self.tabs.is_empty() {
            let id = TabId::from_raw(1);
            let tab = match runtime::create_scratchpad_note(scratchpad_dir) {
                Ok((path, file_stamp)) => EditorTab::scratchpad_with_stamp(id, path, file_stamp),
                Err(err) => {
                    self.status = if self.status == "Ready." {
                        format!("Failed to create scratchpad: {err}")
                    } else {
                        format!("{}; failed to create scratchpad: {err}", self.status)
                    };
                    EditorTab::empty(id, format!("{UNTITLED_PREFIX}-1"))
                }
            };
            self.tabs.push(tab);
        }
        let first = self.tabs.remove(0);
        EditorModel::from_tabs(first, self.tabs, self.status)
    }
}
