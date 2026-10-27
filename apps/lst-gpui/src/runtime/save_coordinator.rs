use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use super::{lock_generation, QueuedSaveJob, SaveTicket};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WriteKind {
    Save,
    Autosave,
}

#[derive(Default)]
struct PathWrites {
    generation: Arc<Mutex<u64>>,
    inflight: Option<WriteKind>,
    queued: Option<QueuedSaveJob>,
}

/// Owns serialization and generation tracking for writes to each path.
#[derive(Default)]
pub(crate) struct SaveCoordinator {
    paths: HashMap<PathBuf, PathWrites>,
}

impl SaveCoordinator {
    pub(crate) fn saving(&self, path: &Path) -> bool {
        self.paths
            .get(path)
            .is_some_and(|state| state.inflight == Some(WriteKind::Save))
    }

    pub(super) fn busy(&self, path: &Path) -> bool {
        self.paths.get(path).is_some_and(|state| state.inflight.is_some())
    }

    pub(super) fn queued(&self, path: &Path) -> bool {
        self.paths.get(path).is_some_and(|state| state.queued.is_some())
    }

    pub(super) fn pending(&self, path: &Path) -> bool {
        self.paths
            .get(path)
            .is_some_and(|state| state.inflight.is_some() || state.queued.is_some())
    }

    pub(super) fn generation(&self, path: &Path) -> u64 {
        self.paths
            .get(path)
            .map_or(0, |state| *lock_generation(&state.generation))
    }

    pub(super) fn begin(&mut self, path: &Path, kind: WriteKind) -> Option<SaveTicket> {
        let state = self.paths.entry(path.to_owned()).or_default();
        if state.inflight.is_some() || (kind == WriteKind::Autosave && state.queued.is_some()) {
            return None;
        }
        state.inflight = Some(kind);
        Some(SaveTicket::issue(&state.generation))
    }

    pub(super) fn finish(&mut self, path: &Path, kind: WriteKind) {
        let state = self
            .paths
            .get_mut(path)
            .expect("write completion must have a path state");
        assert_eq!(
            state.inflight,
            Some(kind),
            "write completion must match the active write"
        );
        state.inflight = None;
    }

    pub(super) fn queue(&mut self, mut job: QueuedSaveJob) {
        let state = self.paths.entry(job.path.clone()).or_default();
        if let Some(previous) = state.queued.take() {
            if let Some(exit_save) = previous.kind.exit_save() {
                if previous.tab_id == job.tab_id && job.kind.exit_save().is_none() {
                    job.kind.set_exit_save(exit_save);
                } else if previous.tab_id != job.tab_id && job.kind.exit_save().is_none() {
                    state.queued = Some(previous);
                    return;
                }
            }
        }
        state.queued = Some(job);
    }

    pub(super) fn take_queued(&mut self, path: &Path) -> Option<QueuedSaveJob> {
        let state = self.paths.get_mut(path)?;
        if state.inflight.is_some() {
            return None;
        }
        state.queued.take()
    }
}

#[cfg(test)]
mod tests {
    use super::super::{SaveGuard, SaveKind};
    use super::*;
    use crate::{ExitSaveContinuation, PendingExitSave};
    use lst_editor::{SaveExpectation, TabId};

    fn job(id: u64, path: &Path, exit: bool) -> QueuedSaveJob {
        let tab_id = TabId::from_raw(id);
        QueuedSaveJob {
            tab_id,
            path: path.to_owned(),
            body: id.to_string(),
            revision: id,
            kind: SaveKind::Save {
                guard: SaveGuard::Exact(SaveExpectation::Unguarded),
                exit_save: exit.then_some(PendingExitSave {
                    tab_id,
                    continuation: ExitSaveContinuation::CloseTab,
                }),
            },
        }
    }

    #[test]
    fn writes_to_the_same_path_serialize_and_new_tickets_invalidate_old_ones() {
        let mut saves = SaveCoordinator::default();
        let path = Path::new("note");
        let other = Path::new("other");
        let first = saves.begin(path, WriteKind::Autosave).unwrap();
        assert!(saves.begin(path, WriteKind::Save).is_none());
        assert!(saves.begin(other, WriteKind::Save).is_some());
        saves.queue(job(1, path, false));
        assert!(saves.take_queued(path).is_none());
        saves.finish(path, WriteKind::Autosave);
        assert!(saves.begin(path, WriteKind::Autosave).is_none());
        assert!(saves.take_queued(path).is_some());
        let second = saves.begin(path, WriteKind::Save).unwrap();
        assert!(saves.saving(path));
        assert!(!first.is_current());
        assert!(second.is_current());
        assert_eq!(saves.generation(path), 2);
        saves.finish(path, WriteKind::Save);
        assert!(!saves.busy(path));
    }

    #[test]
    fn queued_exit_save_survives_replacement_and_other_tabs_cannot_displace_it() {
        let mut saves = SaveCoordinator::default();
        let path = Path::new("note");
        saves.queue(job(1, path, true));
        saves.queue(job(2, path, false));
        saves.queue(job(1, path, false));
        let queued = saves.take_queued(path).unwrap();
        assert_eq!(queued.tab_id, TabId::from_raw(1));
        assert_eq!(queued.kind.exit_save().unwrap().tab_id, queued.tab_id);
        assert!(!saves.queued(path));
    }
}
