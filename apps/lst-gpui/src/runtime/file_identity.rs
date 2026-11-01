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

/// The paths tabs and pending writes held when the UI took the snapshot.
#[derive(Clone, PartialEq, Eq)]
struct Claims {
    tabs: Vec<(TabId, PathBuf)>,
    writes: Vec<PathBuf>,
}

impl Claims {
    /// Off the UI thread: each tab's file identity, resolved once.
    fn tab_identities(&self) -> Vec<(FileIdentity, TabId)> {
        self.tabs
            .iter()
            .map(|(tab_id, path)| (FileIdentity::of(path), *tab_id))
            .collect()
    }
}

fn holder(identity: &FileIdentity, tabs: &[(FileIdentity, TabId)]) -> Option<TabId> {
    tabs.iter()
        .find(|(held, _)| held == identity)
        .map(|(_, tab_id)| *tab_id)
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
fn resolve_open_requests(requests: &[OpenRequest], claims: &Claims) -> Vec<Resolved> {
    let tabs = claims.tab_identities();
    let mut batch = Vec::new();
    requests
        .iter()
        .map(|request| {
            let identity = FileIdentity::of(&request.path);
            if let Some(tab_id) = holder(&identity, &tabs) {
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
fn save_as_claim(target: &Path, claims: &Claims) -> SaveAsClaim {
    let identity = FileIdentity::of(target);
    if let Some(tab_id) = holder(&identity, &claims.tab_identities()) {
        SaveAsClaim::Tab(tab_id)
    } else if claims.writes.iter().any(|path| FileIdentity::of(path) == identity) {
        SaveAsClaim::Write
    } else {
        SaveAsClaim::Free
    }
}

impl LstGpuiApp {
    fn claims(&self) -> Claims {
        Claims {
            tabs: self
                .model
                .tabs()
                .iter()
                .filter_map(|tab| Some((tab.id(), tab.path()?.clone())))
                .collect(),
            writes: self.saves.write_targets(),
        }
    }

    /// Runs `resolve` on a background thread against a snapshot of the
    /// claims, then `apply` on the UI thread while the claims still match
    /// that snapshot; otherwise resolves again.
    fn resolve_against_claims<I, O>(
        &mut self,
        input: I,
        resolve: impl Fn(&I, &Claims) -> O + Copy + Send + 'static,
        apply: impl FnOnce(&mut Self, I, O, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) where
        I: Send + 'static,
        O: Send + 'static,
    {
        let claims = self.claims();
        cx.spawn(async move |this, cx| {
            let snapshot = claims.clone();
            let (input, output) = cx
                .background_executor()
                .spawn(async move {
                    let output = resolve(&input, &snapshot);
                    (input, output)
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                if view.claims() == claims {
                    apply(view, input, output, cx);
                } else {
                    view.resolve_against_claims(input, resolve, apply, cx);
                }
            });
        })
        .detach();
    }

    /// Opens each requested document in a new tab, or activates the tab that
    /// already holds its file.
    pub(crate) fn open_documents(&mut self, requests: Vec<OpenRequest>, cx: &mut Context<Self>) {
        if !requests.is_empty() {
            self.resolve_against_claims(
                requests,
                |requests: &Vec<_>, claims| resolve_open_requests(requests, claims),
                Self::apply_opened_documents,
                cx,
            );
        }
    }

    fn apply_opened_documents(&mut self, requests: Vec<OpenRequest>, resolved: Vec<Resolved>, cx: &mut Context<Self>) {
        let mut failures = Vec::new();
        let mut pruned = false;
        for (request, resolved) in requests.into_iter().zip(resolved) {
            let origin = match resolved {
                Resolved::InTab(tab_id) => {
                    self.update_model(cx, true, |model| {
                        model.set_active_tab(tab_id);
                        if let Some(position) = request.position {
                            model.set_active_cursor_position(position.line, position.column);
                        }
                    });
                    RecentOrigin::of(self.model.active_tab())
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
        match failures.as_slice() {
            [] => {}
            [(path, message)] => {
                let (path, message) = (path.clone(), message.clone());
                self.update_model(cx, true, |model| model.open_file_failed(path, message));
            }
            many => {
                let failures = many
                    .iter()
                    .map(|(path, message)| format!("{}: {message}", path.display()))
                    .collect::<Vec<_>>();
                self.cleanup_message = Some(format!("Some files could not be opened: {}", failures.join("; ")));
                cx.notify();
            }
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
        self.resolve_against_claims(
            target,
            |target: &PathBuf, claims| save_as_claim(target, claims),
            apply,
            cx,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claims(tabs: Vec<(TabId, PathBuf)>, writes: Vec<PathBuf>) -> Claims {
        Claims { tabs, writes }
    }

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
        let tabs = claims(vec![(tab, file.clone())], vec![]);

        let requests = [&symlink, &hard_link, &dotted, &file]
            .map(|path| OpenRequest::new(path.to_path_buf(), RecentOrigin::Regular))
            .to_vec();
        for resolved in resolve_open_requests(&requests, &tabs) {
            assert!(matches!(resolved, Resolved::InTab(id) if id == tab));
        }
        for alias in [&symlink, &hard_link, &dotted] {
            assert_eq!(save_as_claim(alias, &tabs), SaveAsClaim::Tab(tab));
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
            .map(|path| OpenRequest::new(path.to_path_buf(), RecentOrigin::Regular))
            .to_vec();

        let resolved = resolve_open_requests(&requests, &claims(vec![], vec![]));
        assert!(matches!(&resolved[0], Resolved::Read(text, _) if text == "text"));
        assert!(matches!(resolved[1], Resolved::InBatch));
        assert!(matches!(&resolved[2], Resolved::Failed(error) if error.kind() == io::ErrorKind::NotFound));
    }

    #[test]
    fn a_save_as_target_another_write_is_creating_is_claimed() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("new.txt");
        let other = dir.path().join("other.txt");
        assert_eq!(save_as_claim(&target, &claims(vec![], vec![other])), SaveAsClaim::Free);
        // The pending write has not created the file yet.
        let alias = dir.path().join(".").join("new.txt");
        assert_eq!(save_as_claim(&target, &claims(vec![], vec![alias])), SaveAsClaim::Write);
    }
}
