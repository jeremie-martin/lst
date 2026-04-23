use lst_editor::{EditorTab as ModelEditorTab, FileStamp, TabId};
use std::{
    fs::{self, OpenOptions},
    io,
    path::{Path, PathBuf},
};
use time::OffsetDateTime;

pub(crate) fn create_scratchpad_note(
    scratchpad_dir_override: Option<&Path>,
) -> io::Result<(PathBuf, FileStamp)> {
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
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join(".local/share/lst"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME environment variable not set"))
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

pub(super) fn remove_previous_scratchpad_after_save_as(
    previous_scratchpad_path: Option<PathBuf>,
    path: &Path,
    open_tabs: &[ModelEditorTab],
) {
    if let Some(old) = previous_scratchpad_path.filter(|old| {
        !paths_refer_to_same_file(old, path) && !path_is_open_in_another_tab(open_tabs, old, None)
    }) {
        remove_file_best_effort(&old);
    }
}

pub(super) fn remove_scratchpad_file_if_unreferenced(
    open_tabs: &[ModelEditorTab],
    tab_id: TabId,
    path: &Path,
) {
    if !path_is_open_in_another_tab(open_tabs, path, Some(tab_id)) {
        remove_file_best_effort(path);
    }
}

fn path_is_open_in_another_tab(
    open_tabs: &[ModelEditorTab],
    path: &Path,
    ignored_tab_id: Option<TabId>,
) -> bool {
    open_tabs.iter().any(|tab| {
        ignored_tab_id != Some(tab.id())
            && tab
                .path()
                .is_some_and(|tab_path| paths_refer_to_same_file(tab_path, path))
    })
}

fn paths_refer_to_same_file(left: &Path, right: &Path) -> bool {
    if left == right || files_have_same_identity(left, right) {
        return true;
    }
    matches!(
        (fs::canonicalize(left), fs::canonicalize(right)),
        (Ok(left), Ok(right)) if left == right
    )
}

#[cfg(unix)]
fn files_have_same_identity(left: &Path, right: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;

    match (fs::metadata(left), fs::metadata(right)) {
        (Ok(left), Ok(right)) => left.dev() == right.dev() && left.ino() == right.ino(),
        _ => false,
    }
}

#[cfg(not(unix))]
fn files_have_same_identity(_left: &Path, _right: &Path) -> bool {
    false
}
