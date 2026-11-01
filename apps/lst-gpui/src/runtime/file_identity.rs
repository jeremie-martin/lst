//! Which file a path names, and which tab holds it. Paths alias through
//! symlinks, `..`, relative forms and hard links, so the same file can be
//! reached by many paths; at most one tab may hold each file.
//!
//! Resolving identity touches the filesystem, which can block indefinitely
//! (a hung network mount), so it never runs on the UI thread. The UI snapshots
//! the tabs' paths, a background task resolves identities against that
//! snapshot, and the UI applies the answer only while the snapshot still
//! holds. Otherwise it resolves again.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use gpui::Context;
use lst_editor::{FileStamp, Position, TabId};

use super::read_file_with_stamp;
use crate::{
    recent::{normalize_recent_path, RecentOrigin},
    LstGpuiApp,
};

/// A file, independent of the path that reached it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FileIdentity {
    /// An existing file: hard links and every alias share it.
    #[cfg(unix)]
    Inode { device: u64, inode: u64 },
    /// A file that does not exist (yet), by its normalized path.
    Path(PathBuf),
}

impl FileIdentity {
    /// Touches the filesystem: call off the UI thread.
    pub(crate) fn of(path: &Path) -> Self {
        #[cfg(unix)]
        if let Ok(metadata) = fs::metadata(path) {
            use std::os::unix::fs::MetadataExt;
            return Self::Inode {
                device: metadata.dev(),
                inode: metadata.ino(),
            };
        }
        Self::Path(normalize_recent_path(path))
    }
}

/// The paths tabs held when the UI took the snapshot.
pub(crate) type TabPaths = Vec<(TabId, PathBuf)>;

/// The tab in `tabs` whose file `identity` is, resolving each tab's path.
fn tab_holding(identity: &FileIdentity, tabs: &[(TabId, PathBuf)]) -> Option<TabId> {
    tabs.iter()
        .find(|(_, path)| FileIdentity::of(path) == *identity)
        .map(|(tab_id, _)| *tab_id)
}

/// A document to open, or to bring forward when a tab already holds it.
#[derive(Clone, Debug)]
pub(crate) struct OpenRequest {
    pub(crate) path: PathBuf,
    pub(crate) origin: RecentOrigin,
    /// Where to place the cursor, as when reopening a closed tab.
    pub(crate) position: Option<Position>,
}

impl OpenRequest {
    pub(crate) fn new(path: PathBuf, origin: RecentOrigin) -> Self {
        Self {
            path,
            origin,
            position: None,
        }
    }
}

enum Resolved {
    /// A tab already holds the file.
    InTab(TabId),
    /// An earlier request in the same batch opens the file.
    InBatch,
    Read(String, FileStamp),
    Failed(io::Error),
}

/// Off the UI thread: reads each requested file no tab holds yet.
fn resolve_open_requests(requests: &[OpenRequest], tabs: &[(TabId, PathBuf)]) -> Vec<Resolved> {
    let mut batch = Vec::new();
    requests
        .iter()
        .map(|request| {
            let identity = FileIdentity::of(&request.path);
            if let Some(tab_id) = tab_holding(&identity, tabs) {
                return Resolved::InTab(tab_id);
            }
            if batch.contains(&identity) {
                return Resolved::InBatch;
            }
            batch.push(identity);
            match read_file_with_stamp(&request.path) {
                Ok((text, stamp)) => Resolved::Read(text, stamp),
                Err(error) => Resolved::Failed(error),
            }
        })
        .collect()
}

/// Whose file a Save As target is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SaveAsClaim {
    Free,
    /// A tab holds the file.
    Tab(TabId),
    /// A pending write, such as another tab's Save As, targets the file.
    Write,
}

/// Off the UI thread: who already claims the file `target` names.
fn save_as_claim(target: &Path, tabs: &[(TabId, PathBuf)], writes: &[PathBuf]) -> SaveAsClaim {
    let identity = FileIdentity::of(target);
    if let Some(tab_id) = tab_holding(&identity, tabs) {
        SaveAsClaim::Tab(tab_id)
    } else if writes.iter().any(|path| FileIdentity::of(path) == identity) {
        SaveAsClaim::Write
    } else {
        SaveAsClaim::Free
    }
}

impl LstGpuiApp {
    pub(crate) fn tab_paths(&self) -> TabPaths {
        self.model
            .tabs()
            .iter()
            .filter_map(|tab| Some((tab.id(), tab.path()?.clone())))
            .collect()
    }

    /// Opens each requested document in a new tab, or activates the tab that
    /// already holds its file.
    pub(crate) fn open_documents(&mut self, requests: Vec<OpenRequest>, cx: &mut Context<Self>) {
        if requests.is_empty() {
            return;
        }
        let tabs = self.tab_paths();
        cx.spawn(async move |this, cx| {
            let snapshot = tabs.clone();
            let (requests, resolved) = cx
                .background_executor()
                .spawn(async move {
                    let resolved = resolve_open_requests(&requests, &snapshot);
                    (requests, resolved)
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                if view.tab_paths() == tabs {
                    view.apply_opened_documents(requests, resolved, cx);
                } else {
                    view.open_documents(requests, cx);
                }
            });
        })
        .detach();
    }

    fn apply_opened_documents(&mut self, requests: Vec<OpenRequest>, resolved: Vec<Resolved>, cx: &mut Context<Self>) {
        let mut failures = Vec::new();
        let mut pruned = false;
        for (request, resolved) in requests.into_iter().zip(resolved) {
            let origin = match resolved {
                Resolved::InTab(tab_id) => {
                    let scratchpad = self.model.tab_by_id(tab_id).is_some_and(|tab| tab.is_scratchpad());
                    self.update_model(cx, true, |model| {
                        model.set_active_tab(tab_id);
                        if let Some(position) = request.position {
                            model.set_active_cursor_position(position.line, position.column);
                        }
                    });
                    if scratchpad {
                        RecentOrigin::Scratchpad
                    } else {
                        RecentOrigin::Regular
                    }
                }
                Resolved::InBatch => continue,
                Resolved::Read(text, stamp) => {
                    let path = request.path.clone();
                    self.update_model(cx, true, |model| {
                        match request.origin {
                            RecentOrigin::Regular => model.open_files_with_stamps(vec![(path, text, Some(stamp))]),
                            RecentOrigin::Scratchpad => model.open_scratchpad_with_stamp(path, text, stamp),
                        }
                        if let Some(position) = request.position {
                            model.set_active_cursor_position(position.line, position.column);
                        }
                    });
                    request.origin
                }
                Resolved::Failed(error) => {
                    if error.kind() == io::ErrorKind::NotFound {
                        self.recent.prune_path(&request.path);
                        pruned = true;
                    }
                    failures.push((request.path, error.to_string()));
                    continue;
                }
            };
            self.recent.record_with_origin(&request.path, origin);
        }
        if pruned {
            self.spawn_recent_previews(cx);
        }
        match <[_; 1]>::try_from(failures) {
            Ok([(path, message)]) => self.update_model(cx, true, |model| model.open_file_failed(path, message)),
            Err(failures) if !failures.is_empty() => {
                let failures = failures
                    .iter()
                    .map(|(path, message)| format!("{}: {message}", path.display()))
                    .collect::<Vec<_>>();
                self.cleanup_message = Some(format!("Some files could not be opened: {}", failures.join("; ")));
                cx.notify();
            }
            Err(_) => {}
        }
    }

    /// Resolves whose file `target` is, then hands the answer to `apply` on
    /// the UI thread while the tabs and writes still claim the paths it was
    /// resolved against.
    pub(super) fn resolve_save_as_target(
        &mut self,
        target: PathBuf,
        apply: impl FnOnce(&mut Self, PathBuf, SaveAsClaim, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let claims = (self.tab_paths(), self.saves.write_targets());
        cx.spawn(async move |this, cx| {
            let (tabs, writes) = claims.clone();
            let (target, claim) = cx
                .background_executor()
                .spawn(async move {
                    let claim = save_as_claim(&target, &tabs, &writes);
                    (target, claim)
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                if (view.tab_paths(), view.saves.write_targets()) == claims {
                    apply(view, target, claim, cx);
                } else {
                    view.resolve_save_as_target(target, apply, cx);
                }
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn every_alias_of_an_open_file_resolves_to_its_tab() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("note.txt");
        fs::write(&file, "text").unwrap();
        let symlink = dir.path().join("symlink.txt");
        std::os::unix::fs::symlink(&file, &symlink).unwrap();
        let hard_link = dir.path().join("hard-link.txt");
        fs::hard_link(&file, &hard_link).unwrap();
        let dotted = dir.path().join("sub/../note.txt");
        fs::create_dir(dir.path().join("sub")).unwrap();
        let tab = TabId::from_raw(7);
        let tabs = [(tab, file.clone())];

        let requests = [&symlink, &hard_link, &dotted, &file]
            .map(|path| OpenRequest::new(path.to_path_buf(), RecentOrigin::Regular));
        for resolved in resolve_open_requests(&requests, &tabs) {
            assert!(matches!(resolved, Resolved::InTab(id) if id == tab));
        }
        for alias in [&symlink, &hard_link, &dotted] {
            assert_eq!(save_as_claim(alias, &tabs, &[]), SaveAsClaim::Tab(tab));
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_batch_reads_each_file_once_and_reports_missing_ones() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("note.txt");
        fs::write(&file, "text").unwrap();
        let symlink = dir.path().join("symlink.txt");
        std::os::unix::fs::symlink(&file, &symlink).unwrap();
        let requests = [&file, &symlink, &dir.path().join("missing.txt")]
            .map(|path| OpenRequest::new(path.to_path_buf(), RecentOrigin::Regular));

        let resolved = resolve_open_requests(&requests, &[]);
        assert!(matches!(&resolved[0], Resolved::Read(text, _) if text == "text"));
        assert!(matches!(resolved[1], Resolved::InBatch));
        assert!(matches!(&resolved[2], Resolved::Failed(error) if error.kind() == io::ErrorKind::NotFound));
    }

    #[test]
    fn a_save_as_target_another_write_is_creating_is_claimed() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("new.txt");
        let other = dir.path().join("other.txt");
        assert_eq!(save_as_claim(&target, &[], &[other]), SaveAsClaim::Free);
        // The pending write has not created the file yet.
        let alias = dir.path().join(".").join("new.txt");
        assert_eq!(save_as_claim(&target, &[], &[alias]), SaveAsClaim::Write);
    }
}
