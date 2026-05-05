//! Shared fixture for the real-display test suites under `tests/real_x11_*`.
//!
//! Each suite holds one [`ScratchpadSession`] per `#[test]`. The session
//! owns a private temp directory and the X11 [`Display`] connection;
//! [`ScratchpadSession::open`] spawns the editor against a child scratchpad
//! directory, focuses the window, and returns a ready-to-drive
//! `(Editor, autosave_path)` pair. The session deletes its temp tree on
//! drop so a panicking test still cleans up.
//!
//! To keep different suites from racing for keyboard focus and the global
//! pointer, run with `--test-threads=1`. `cargo test --tests` uses
//! `LST_X11_WINDOW_TIMEOUT_MS` for the window-discovery timeout (default
//! 30s).

#![allow(dead_code)]

use std::env;
use std::error::Error;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use lst_x11_harness::{Display, Editor, FileWaitOpts, Key, KeyChord, SpawnOpts};

pub type TestResult = Result<(), Box<dyn Error + Send + Sync>>;
pub type SupportResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

const FOCUS_QUIET: Duration = Duration::from_millis(75);
const FOCUS_TIMEOUT: Duration = Duration::from_secs(5);
const FILE_STABLE: Duration = Duration::from_millis(200);
const FILE_WAIT_TIMEOUT: Duration = Duration::from_secs(10);
const SCRATCHPAD_DISCOVERY: Duration = Duration::from_secs(10);
const QUIT_TIMEOUT: Duration = Duration::from_secs(10);

/// Owns a private temp dir and an X11 connection for the duration of a
/// test. One session per `#[test]` keeps lifetimes simple — the
/// `&mut Display` borrow inside `open()` enforces "one editor at a time".
pub struct ScratchpadSession {
    display: Display,
    binary: PathBuf,
    root: PathBuf,
}

impl ScratchpadSession {
    pub fn new(label: &str) -> SupportResult<Self> {
        let display = Display::from_env()?;
        let binary = editor_binary()?;
        let root = temp_dir(&format!("lst-real-x11-{label}"))?;
        Ok(Self {
            display,
            binary,
            root,
        })
    }

    /// Spawn the editor against a fresh scratchpad subdirectory, focus the
    /// window for keyboard input, and return the autosave path the editor
    /// will write to plus the focused [`Editor`] handle. The autosave path
    /// already exists at return time.
    pub fn open(&mut self, name: &str) -> SupportResult<(Editor<'_>, PathBuf)> {
        let dir = self.root.join(name);
        fs::create_dir_all(&dir)?;
        let title = unique_title(name);
        let args: [&OsStr; 2] = [OsStr::new("--scratchpad-dir"), dir.as_os_str()];
        let mut editor = self.display.spawn_editor(SpawnOpts {
            binary: &self.binary,
            args: &args,
            title: &title,
            stderr: Stdio::inherit(),
            stdout: Stdio::null(),
            extra_env: &[],
        })?;
        let path = wait_for_single_file(&dir, SCRATCHPAD_DISCOVERY)?;
        editor.click_center()?;
        editor.wait_quiet(FOCUS_QUIET, FOCUS_TIMEOUT)?;
        Ok((editor, path))
    }

    /// Spawn the editor with the given file path as a positional arg and
    /// return the focused [`Editor`] handle.
    pub fn open_file(&mut self, name: &str, file: &Path) -> SupportResult<Editor<'_>> {
        let title = unique_title(name);
        let args: [&OsStr; 1] = [file.as_os_str()];
        let mut editor = self.display.spawn_editor(SpawnOpts {
            binary: &self.binary,
            args: &args,
            title: &title,
            stderr: Stdio::inherit(),
            stdout: Stdio::null(),
            extra_env: &[],
        })?;
        editor.click_center()?;
        editor.wait_quiet(FOCUS_QUIET, FOCUS_TIMEOUT)?;
        Ok(editor)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl Drop for ScratchpadSession {
    fn drop(&mut self) {
        // Preserve artifacts when a panic is unwinding so the file the
        // editor was last writing to is still on disk for inspection. On
        // success, the test calls `cleanup()` explicitly. Setting
        // `LST_X11_KEEP_TEMP=1` also preserves them unconditionally.
        if std::thread::panicking() || env::var_os("LST_X11_KEEP_TEMP").is_some() {
            eprintln!(
                "support: preserving temp dir for inspection: {}",
                self.root.display()
            );
            return;
        }
        if let Err(error) = fs::remove_dir_all(&self.root) {
            if error.kind() != std::io::ErrorKind::NotFound {
                eprintln!(
                    "support: failed to remove temp dir {}: {error}",
                    self.root.display()
                );
            }
        }
    }
}

/// Test-side conveniences over [`Editor`]. Anything lst-specific
/// (autosave semantics, the Ctrl+S save chord, default wait windows)
/// lives here, not in the harness.
pub trait EditorTestExt {
    /// Press `Ctrl+S` and assert the autosave path's content matches
    /// `expected` (10s timeout, 200ms stability window).
    fn save_then_expect_file(&mut self, path: &Path, expected: &str) -> SupportResult<()>;

    /// Convenience: `quit(QUIT_TIMEOUT)`.
    fn quit_default(self) -> SupportResult<()>;
}

impl EditorTestExt for Editor<'_> {
    fn save_then_expect_file(&mut self, path: &Path, expected: &str) -> SupportResult<()> {
        self.press(KeyChord::Ctrl(Key::Char('s')))?;
        self.wait_file_text(
            path,
            expected,
            FileWaitOpts::new(FILE_WAIT_TIMEOUT, FILE_STABLE),
        )?;
        Ok(())
    }

    fn quit_default(self) -> SupportResult<()> {
        self.quit(QUIT_TIMEOUT)?;
        Ok(())
    }
}

pub fn editor_binary() -> SupportResult<PathBuf> {
    if let Some(path) = env::var_os("LST_GPUI_BIN") {
        return Ok(PathBuf::from(path));
    }
    if let Some(path) = option_env!("CARGO_BIN_EXE_lst") {
        return Ok(PathBuf::from(path));
    }
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    for name in ["lst", "lst-gpui"] {
        let fallback = manifest_dir.join("../../target/debug").join(name);
        if fallback.exists() {
            return Ok(fallback);
        }
    }
    Err(
        "could not find lst binary; run `cargo build -p lst-gpui --bin lst` or set LST_GPUI_BIN"
            .into(),
    )
}

pub fn wait_for_single_file(dir: &Path, timeout: Duration) -> SupportResult<PathBuf> {
    let deadline = Instant::now() + timeout;
    loop {
        let mut files = fs::read_dir(dir)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<Vec<_>, _>>()?;
        files.retain(|path| path.is_file());
        files.sort();
        if files.len() == 1 {
            return Ok(files[0].clone());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "timed out waiting for one scratchpad file in {}; found {}",
                dir.display(),
                files.len()
            )
            .into());
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

pub fn count_files(dir: &Path) -> SupportResult<usize> {
    let mut count = 0;
    for entry in fs::read_dir(dir)? {
        if entry?.path().is_file() {
            count += 1;
        }
    }
    Ok(count)
}

pub fn temp_dir(label: &str) -> SupportResult<PathBuf> {
    let dir = env::temp_dir().join(format!("{label}-{}", unique_id()));
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn unique_title(label: &str) -> String {
    format!("lst-real-x11-{label}-{}", unique_id())
}

fn unique_id() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

pub fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
}

pub fn secs(value: u64) -> Duration {
    Duration::from_secs(value)
}
