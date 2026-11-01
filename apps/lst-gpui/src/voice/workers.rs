//! Voice worker threads. A worker can still own a segment's temporary WAV
//! after its session is cancelled: capture finalizes the file, and a cancelled
//! upload drops its request. Dropping the file deletes it, but `process::exit`
//! runs no destructors, so quitting waits briefly for workers to finish.

use std::{
    io,
    sync::{Mutex, PoisonError},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

static WORKERS: Mutex<Vec<JoinHandle<()>>> = Mutex::new(Vec::new());

pub(super) fn spawn(name: &str, work: impl FnOnce() + Send + 'static) -> io::Result<()> {
    let worker = thread::Builder::new().name(name.into()).spawn(work)?;
    let mut workers = WORKERS.lock().unwrap_or_else(PoisonError::into_inner);
    workers.retain(|worker| !worker.is_finished());
    workers.push(worker);
    Ok(())
}

/// Waits until every voice worker has finished, or `timeout` elapses: a
/// microphone or connection that never yields must not prevent exit.
pub(crate) fn wait_before_exit(timeout: Duration) {
    let deadline = Instant::now() + timeout;
    let workers = std::mem::take(&mut *WORKERS.lock().unwrap_or_else(PoisonError::into_inner));
    for worker in workers {
        while !worker.is_finished() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_waits_for_a_worker_to_release_its_temporary_audio() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let path = file.path().to_path_buf();
        spawn("voice-test", move || {
            thread::sleep(Duration::from_millis(50));
            drop(file);
        })
        .unwrap();
        wait_before_exit(Duration::from_secs(5));
        assert!(!path.exists());
    }
}
