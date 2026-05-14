//! Shared fixture for the real-display test suites under `tests/real_x11_*`.
//!
//! Each suite holds one [`ScratchpadSession`] per `#[test]`. The session
//! owns a private temp directory and the X11 [`Display`] connection;
//! [`ScratchpadSession::open`] spawns the editor against a child scratchpad
//! directory, focuses the window, and returns a ready-to-drive
//! `(Editor, autosave_path)` pair. Use [`run_x11_test`] so successful tests
//! clean their temp tree and failed tests preserve it for inspection.
//!
//! The nextest `x11` profiles run these tests serially so separate suites do
//! not race for keyboard focus or the global pointer. The harness uses
//! `LST_X11_WINDOW_TIMEOUT_MS` for the window-discovery timeout (default 30s).

#![allow(dead_code)]

use std::env;
use std::error::Error;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use lst_x11_harness::{Display, Editor, FileWaitOpts, SpawnOpts, StateTraceRecord};

pub type TestResult = Result<(), Box<dyn Error + Send + Sync>>;
pub type SupportResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

const FOCUS_QUIET: Duration = Duration::from_millis(75);
const FOCUS_TIMEOUT: Duration = Duration::from_secs(5);
const FILE_STABLE: Duration = Duration::from_millis(200);
const FILE_WAIT_TIMEOUT: Duration = Duration::from_secs(10);
const SAVE_WAIT_TIMEOUT: Duration = Duration::from_secs(30);
const SCRATCHPAD_DISCOVERY: Duration = Duration::from_secs(10);
const QUIT_TIMEOUT: Duration = Duration::from_secs(10);

/// Owns a private temp dir and an X11 connection for the duration of a
/// test. One session per `#[test]` keeps lifetimes simple — the
/// `&mut Display` borrow inside `open()` enforces "one editor at a time".
pub struct ScratchpadSession {
    display: Display,
    binary: PathBuf,
    root: PathBuf,
    artifacts: PathBuf,
    home: PathBuf,
    state_home: PathBuf,
    drop_handled: bool,
}

impl ScratchpadSession {
    pub fn new(label: &str) -> SupportResult<Self> {
        let display = Display::from_env()?;
        let binary = editor_binary()?;
        let root = temp_dir(&format!("lst-real-x11-{label}"))?;
        let artifacts = root.join("artifacts");
        let home = root.join("home");
        let state_home = root.join("state");
        fs::create_dir_all(&artifacts)?;
        fs::create_dir_all(&home)?;
        fs::create_dir_all(&state_home)?;
        env::set_var("LST_X11_ARTIFACT_DIR", &artifacts);
        Ok(Self {
            display,
            binary,
            root,
            artifacts,
            home,
            state_home,
            drop_handled: false,
        })
    }

    /// Spawn the editor against a fresh scratchpad subdirectory, focus the
    /// window for keyboard input, and return the autosave path the editor
    /// will write to plus the focused [`Editor`] handle. The autosave path
    /// already exists at return time.
    pub fn open(&mut self, name: &str) -> SupportResult<(Editor<'_>, PathBuf)> {
        self.open_with_env(name, &[])
    }

    /// Same as [`open`], but exports `extra_env` to the spawned editor.
    /// Threads through to `SpawnOpts::extra_env` so test seams (like the
    /// `LST_LLM_FAKE_RESPONSE` fake LLM client) can be activated per-test
    /// without leaking into the parent process or other tests.
    pub fn open_with_env(
        &mut self,
        name: &str,
        extra_env: &[(&OsStr, &OsStr)],
    ) -> SupportResult<(Editor<'_>, PathBuf)> {
        let dir = self.root.join(name);
        fs::create_dir_all(&dir)?;
        let title = unique_title(name);
        let args: [&OsStr; 2] = [OsStr::new("--scratchpad-dir"), dir.as_os_str()];
        let stderr_log_path = self.stderr_log_path(name);
        let state_trace_path = self.state_trace_path(name);
        let (stdout, stderr) = self.log_stdio(name)?;
        let env = spawn_env(&self.home, &self.state_home, extra_env);
        let mut editor = self.display.spawn_editor(SpawnOpts {
            binary: &self.binary,
            args: &args,
            title: &title,
            stderr,
            stdout,
            extra_env: &env,
            stderr_log_path: Some(&stderr_log_path),
            state_trace_path: Some(&state_trace_path),
        })?;
        let path = wait_for_single_file(&dir, SCRATCHPAD_DISCOVERY)?;
        editor.click_center()?;
        editor.wait_quiet(FOCUS_QUIET, FOCUS_TIMEOUT)?;
        editor.wait_text_viewport(FOCUS_TIMEOUT)?;
        Ok((editor, path))
    }

    /// Spawn the editor with the given file path as a positional arg and
    /// return the focused [`Editor`] handle.
    pub fn open_file(&mut self, name: &str, file: &Path) -> SupportResult<Editor<'_>> {
        self.open_files(name, &[file.to_path_buf()])
    }

    /// Spawn the editor with the given file paths as positional args and
    /// return the focused [`Editor`] handle.
    pub fn open_files(&mut self, name: &str, files: &[PathBuf]) -> SupportResult<Editor<'_>> {
        let title = unique_title(name);
        let args = files
            .iter()
            .map(|file| file.as_os_str())
            .collect::<Vec<_>>();
        let stderr_log_path = self.stderr_log_path(name);
        let state_trace_path = self.state_trace_path(name);
        let (stdout, stderr) = self.log_stdio(name)?;
        let env = spawn_env(&self.home, &self.state_home, &[]);
        let mut editor = self.display.spawn_editor(SpawnOpts {
            binary: &self.binary,
            args: &args,
            title: &title,
            stderr,
            stdout,
            extra_env: &env,
            stderr_log_path: Some(&stderr_log_path),
            state_trace_path: Some(&state_trace_path),
        })?;
        editor.click_center()?;
        editor.wait_quiet(FOCUS_QUIET, FOCUS_TIMEOUT)?;
        editor.wait_text_viewport(FOCUS_TIMEOUT)?;
        Ok(editor)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn artifacts(&self) -> &Path {
        &self.artifacts
    }

    pub fn seed_file(&self, name: &str, contents: &str) -> SupportResult<PathBuf> {
        let path = self.root.join(name);
        fs::write(&path, contents)?;
        Ok(path)
    }

    pub fn seed_recent_files(&self, paths: &[PathBuf]) -> SupportResult<PathBuf> {
        let state_path = self.state_home.join("lst").join("recent-files");
        if let Some(parent) = state_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut body = String::from("lst-recent-files-v1\n");
        for path in paths {
            for byte in recent_path_bytes(path) {
                body.push(hex_digit(byte >> 4));
                body.push(hex_digit(byte & 0x0f));
            }
            body.push('\n');
        }
        fs::write(&state_path, body)?;
        Ok(state_path)
    }

    fn cleanup(&mut self) -> SupportResult<()> {
        if env::var_os("LST_X11_KEEP_TEMP").is_some() {
            self.preserve("LST_X11_KEEP_TEMP");
            return Ok(());
        }
        match fs::remove_dir_all(&self.root) {
            Ok(()) => {
                self.drop_handled = true;
                Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.drop_handled = true;
                Ok(())
            }
            Err(error) => Err(error.into()),
        }
    }

    fn preserve(&mut self, reason: &str) {
        eprintln!(
            "support: preserving temp dir after {reason}: {}",
            self.root.display()
        );
        self.drop_handled = true;
    }

    fn log_stdio(&self, name: &str) -> SupportResult<(Stdio, Stdio)> {
        let stdout = File::create(self.artifacts.join(format!("{name}-stdout.log")))?;
        let stderr = File::create(self.stderr_log_path(name))?;
        Ok((Stdio::from(stdout), Stdio::from(stderr)))
    }

    fn stderr_log_path(&self, name: &str) -> PathBuf {
        self.artifacts.join(format!("{name}-stderr.log"))
    }

    fn state_trace_path(&self, name: &str) -> PathBuf {
        self.artifacts.join(format!("{name}-state-trace.jsonl"))
    }
}

fn spawn_env<'a>(
    home: &'a Path,
    state_home: &'a Path,
    extra_env: &'a [(&'a OsStr, &'a OsStr)],
) -> Vec<(&'a OsStr, &'a OsStr)> {
    let mut env = vec![
        (OsStr::new("HOME"), home.as_os_str()),
        (OsStr::new("XDG_STATE_HOME"), state_home.as_os_str()),
    ];
    env.extend_from_slice(extra_env);
    env
}

impl Drop for ScratchpadSession {
    fn drop(&mut self) {
        if self.drop_handled {
            return;
        }
        if std::thread::panicking() {
            self.preserve("panic");
        } else if env::var_os("LST_X11_KEEP_TEMP").is_some() {
            self.preserve("LST_X11_KEEP_TEMP");
        } else {
            self.preserve("test failure or missing explicit cleanup");
        }
    }
}

pub fn run_x11_test(
    label: &str,
    test: impl FnOnce(&mut ScratchpadSession) -> TestResult,
) -> TestResult {
    let mut session = ScratchpadSession::new(label)?;
    let result = test(&mut session);
    match result {
        Ok(()) => session.cleanup(),
        Err(error) => {
            session.preserve("error");
            Err(error)
        }
    }
}

/// Test-side conveniences over [`Editor`]. Anything lst-specific
/// (autosave semantics, the Ctrl+S save chord, default wait windows)
/// lives here, not in the harness.
pub trait EditorTestExt {
    /// Drive a vim-style key sequence through the harness and capture a window
    /// artifact if the sequence fails.
    fn keys(&mut self, sequence: &str) -> SupportResult<()>;

    /// Press `Ctrl+S`.
    fn save(&mut self) -> SupportResult<()>;

    /// Assert the path's content matches `expected` without first saving.
    fn expect_file(&mut self, path: &Path, expected: &str) -> SupportResult<()>;

    /// Press `Ctrl+S` and assert the autosave path's content matches
    /// `expected` (10s timeout, 200ms stability window).
    fn save_then_expect_file(&mut self, path: &Path, expected: &str) -> SupportResult<()>;

    /// Wait for the editor process to exit successfully.
    fn wait_for_successful_exit(&mut self, timeout: Duration) -> SupportResult<()>;

    /// Convenience: `quit(QUIT_TIMEOUT)`.
    fn quit_default(self) -> SupportResult<()>;

    /// Move the visible editor cursor to the start of the document and
    /// assert the setup state. Use this when a behavior spec needs a stable
    /// starting point before the actual gesture under test.
    fn place_cursor_at_document_start(&mut self) -> SupportResult<StateTraceRecord>;

    /// Drain the state trace and assert the latest record's vim mode label
    /// matches `mode` (e.g. `"NORMAL"`, `"INSERT"`, `"VISUAL"`, `"V-LINE"`).
    fn expect_vim_mode(&mut self, mode: &str) -> SupportResult<StateTraceRecord>;

    /// Drain the state trace and assert the latest record has exactly
    /// `cursors.len()` cursors at the given (line, col) head positions in
    /// document order. Useful for proving a multi-cursor creation gesture
    /// landed correctly without typing-and-inspecting-the-file.
    fn expect_cursor_heads(
        &mut self,
        cursors: &[(usize, usize)],
    ) -> SupportResult<StateTraceRecord>;

    /// Assert the find panel is visible with the given query and match count.
    fn expect_find_state(
        &mut self,
        query: &str,
        match_count: usize,
    ) -> SupportResult<StateTraceRecord>;

    /// Click the status-bar cleanup (sparkle) button.
    fn click_cleanup_button(&mut self) -> SupportResult<()>;
}

impl EditorTestExt for Editor<'_> {
    fn keys(&mut self, sequence: &str) -> SupportResult<()> {
        let result = self.send_keys(sequence);
        with_window_artifact(self, "send-keys", result)
    }

    fn save(&mut self) -> SupportResult<()> {
        let result = self.send_keys("<C-s>");
        with_window_artifact(self, "save", result)
    }

    fn expect_file(&mut self, path: &Path, expected: &str) -> SupportResult<()> {
        let result = self.wait_file_text(
            path,
            expected,
            FileWaitOpts::new(FILE_WAIT_TIMEOUT, FILE_STABLE),
        );
        with_window_artifact(self, "expect-file", result)?;
        Ok(())
    }

    fn save_then_expect_file(&mut self, path: &Path, expected: &str) -> SupportResult<()> {
        self.save()?;
        self.expect_file(path, expected)?;
        self.wait_state("save completion", SAVE_WAIT_TIMEOUT, |record| {
            !record.active_tab_modified
        })?;
        Ok(())
    }

    fn wait_for_successful_exit(&mut self, timeout: Duration) -> SupportResult<()> {
        let status = self.wait_for_exit(timeout)?;
        require_success(status)
    }

    fn quit_default(self) -> SupportResult<()> {
        let status = self.quit(QUIT_TIMEOUT)?;
        require_success(status)
    }

    fn place_cursor_at_document_start(&mut self) -> SupportResult<StateTraceRecord> {
        self.keys("<C-home>")?;
        self.expect_cursor_heads(&[(0, 0)])
    }

    fn expect_vim_mode(&mut self, mode: &str) -> SupportResult<StateTraceRecord> {
        self.wait_state("vim mode", FOCUS_TIMEOUT, |record| record.vim_mode == mode)
    }

    fn expect_cursor_heads(
        &mut self,
        cursors: &[(usize, usize)],
    ) -> SupportResult<StateTraceRecord> {
        self.wait_state("cursor heads", FOCUS_TIMEOUT, |record| {
            let actual: Vec<(usize, usize)> = record
                .cursors
                .iter()
                .map(|c| (c.head_line, c.head_col))
                .collect();
            actual.as_slice() == cursors
        })
    }

    fn expect_find_state(
        &mut self,
        query: &str,
        match_count: usize,
    ) -> SupportResult<StateTraceRecord> {
        self.wait_state("find state", FOCUS_TIMEOUT, |record| {
            record.find.visible
                && record.find.query == query
                && record.find.match_count == match_count
        })
    }

    fn click_cleanup_button(&mut self) -> SupportResult<()> {
        let record = self.wait_state("cleanup button bounds", FOCUS_TIMEOUT, |state| {
            state.cleanup_button_bounds_px.is_some()
        })?;
        let (ox, oy, w, h) = record
            .cleanup_button_bounds_px
            .ok_or("cleanup button bounds missing after wait")?;
        let scale = if record.viewport.scale_factor > 0.0 {
            record.viewport.scale_factor
        } else {
            1.0
        };
        let cx = ((ox + w * 0.5) * scale).round() as i32;
        let cy = ((oy + h * 0.5) * scale).round() as i32;
        let result = self.click_at(cx, cy);
        with_window_artifact(self, "cleanup-click", result)?;
        Ok(())
    }
}

fn require_success(status: ExitStatus) -> SupportResult<()> {
    if status.success() {
        Ok(())
    } else {
        Err(format!("editor exited with non-success status: {status}").into())
    }
}

fn with_window_artifact<T>(
    editor: &mut Editor<'_>,
    label: &str,
    result: lst_x11_harness::Result<T>,
) -> SupportResult<T> {
    match result {
        Ok(value) => Ok(value),
        Err(error) => {
            let artifact = capture_window_artifact(editor, label);
            let suffix = artifact
                .as_ref()
                .map(|path| format!("; window artifact: {}", path.display()))
                .unwrap_or_default();
            Err(format!("{error}{suffix}").into())
        }
    }
}

fn capture_window_artifact(editor: &Editor<'_>, label: &str) -> Option<PathBuf> {
    let dir = env::var_os("LST_X11_ARTIFACT_DIR").map(PathBuf::from)?;
    if fs::create_dir_all(&dir).is_err() {
        return None;
    }
    let path = dir.join(format!("{label}-{}.xwd", unique_id()));
    let window_id = editor.window_id().to_string();
    let path_text = path.to_string_lossy().into_owned();
    let status = std::process::Command::new("xwd")
        .args(["-silent", "-id", &window_id, "-out", &path_text])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .ok()?;
    status.success().then_some(path)
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

fn hex_digit(value: u8) -> char {
    match value {
        0..=9 => (b'0' + value) as char,
        10..=15 => (b'a' + (value - 10)) as char,
        _ => unreachable!("nibble should fit in hex digit"),
    }
}

#[cfg(unix)]
fn recent_path_bytes(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;

    path.as_os_str().as_bytes().to_vec()
}

#[cfg(not(unix))]
fn recent_path_bytes(path: &Path) -> Vec<u8> {
    path.as_os_str().to_string_lossy().as_bytes().to_vec()
}

pub fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
}

pub fn secs(value: u64) -> Duration {
    Duration::from_secs(value)
}
