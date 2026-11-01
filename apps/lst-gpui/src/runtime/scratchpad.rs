use super::FileIdentity;
use lst_editor::{EditorTab as ModelEditorTab, FileStamp, TabId};
use std::{
    fs::{self, OpenOptions},
    io,
    path::{Path, PathBuf},
};
use time::OffsetDateTime;

pub(crate) fn create_scratchpad_note(scratchpad_dir_override: Option<&Path>) -> io::Result<(PathBuf, FileStamp)> {
    create_scratchpad_note_with_timestamp(scratchpad_dir_override, scratchpad_timestamp())
}

pub(super) fn create_scratchpad_note_with_timestamp(
    scratchpad_dir_override: Option<&Path>,
    timestamp: String,
) -> io::Result<(PathBuf, FileStamp)> {
    let dir = scratchpad_dir(scratchpad_dir_override)?;
    fs::create_dir_all(&dir)?;

    for suffix in 0usize.. {
        let file_name = if suffix == 0 {
            format!("{timestamp}.md")
        } else {
            format!("{timestamp}_{suffix}.md")
        };
        let path = dir.join(file_name);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(_) => return super::file_stamp(&path).map(|stamp| (path, stamp)),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err),
        }
    }

    unreachable!("unbounded scratchpad suffix loop should return")
}

pub(crate) fn scratchpad_dir(scratchpad_dir_override: Option<&Path>) -> io::Result<PathBuf> {
    if let Some(dir) = scratchpad_dir_override {
        return Ok(dir.to_path_buf());
    }
    crate::user_dirs::home_dir()
        .map(|home| home.join(".local/share/lst"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not an absolute directory"))
}

fn scratchpad_timestamp() -> String {
    let now = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
    format!(
        "{:04}-{:02}-{:02}_{:02}-{:02}-{:02}",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    )
}

fn remove_file_best_effort(path: &Path) {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(_) => {}
    }
}

/// Deletes the scratchpad a Save As moved away from, unless the new path is
/// that same file or another tab holds it. Two paths that were just written
/// are compared by `FileIdentity`.
///
/// Returns whether the scratchpad moved to a different file.
pub(super) fn remove_previous_scratchpad_after_save_as(old: &Path, path: &Path, open_tabs: &[ModelEditorTab]) -> bool {
    let moved = FileIdentity::of(old) != FileIdentity::of(path);
    if moved && !held_by_another_tab(open_tabs, old, None) {
        remove_file_best_effort(old);
    }
    moved
}

pub(super) fn remove_scratchpad_file_if_unreferenced(open_tabs: &[ModelEditorTab], tab_id: TabId, path: &Path) {
    if !held_by_another_tab(open_tabs, path, Some(tab_id)) {
        remove_file_best_effort(path);
    }
}

/// Opening and Save As admit at most one tab per file (`FileIdentity`), so a
/// tab holding the same file holds the same path: comparing paths keeps
/// filesystem access off the UI thread.
fn held_by_another_tab(open_tabs: &[ModelEditorTab], path: &Path, ignored_tab_id: Option<TabId>) -> bool {
    open_tabs
        .iter()
        .any(|tab| ignored_tab_id != Some(tab.id()) && tab.path().is_some_and(|tab_path| tab_path == path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_tab(id: u64, path: &Path) -> ModelEditorTab {
        ModelEditorTab::from_path_with_stamp(TabId::from_raw(id), path.to_path_buf(), "", None)
    }

    #[test]
    fn scratchpads_created_in_the_same_second_get_distinct_files() {
        let dir = tempfile::tempdir().unwrap();
        let timestamp = "2026-09-24_12-00-00".to_string();
        let (first, _) = create_scratchpad_note_with_timestamp(Some(dir.path()), timestamp.clone()).unwrap();
        fs::write(&first, "first note").unwrap();

        let (second, _) = create_scratchpad_note_with_timestamp(Some(dir.path()), timestamp).unwrap();

        assert_ne!(first, second);
        assert_eq!(fs::read_to_string(&first).unwrap(), "first note");
        assert_eq!(fs::read_to_string(&second).unwrap(), "");
    }

    #[test]
    fn save_as_removes_the_old_scratchpad_only_when_nothing_else_uses_it() {
        let dir = tempfile::tempdir().unwrap();
        let scratchpad = |name: &str| {
            let path = dir.path().join(name);
            fs::write(&path, "note").unwrap();
            path
        };

        let moved = scratchpad("moved.md");
        assert!(remove_previous_scratchpad_after_save_as(
            &moved,
            &dir.path().join("saved.md"),
            &[]
        ));
        assert!(!moved.exists(), "the scratchpad was saved elsewhere");

        let linked = scratchpad("linked.md");
        let link = dir.path().join("link.md");
        std::os::unix::fs::symlink(&linked, &link).unwrap();
        assert!(!remove_previous_scratchpad_after_save_as(&linked, &link, &[]));
        assert!(linked.exists(), "the save target is the scratchpad itself");

        let shared = scratchpad("shared.md");
        remove_previous_scratchpad_after_save_as(&shared, &dir.path().join("saved-copy.md"), &[open_tab(2, &shared)]);
        assert!(shared.exists(), "another tab still edits the scratchpad");
    }
}
