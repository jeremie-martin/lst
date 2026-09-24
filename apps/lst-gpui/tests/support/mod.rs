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

use lst_x11_harness::{
    clipboard::{write_clipboard_text, Selection},
    Display, Editor, FileWaitOpts, SpawnOpts, StateTraceRecord,
};

pub type TestResult = Result<(), Box<dyn Error + Send + Sync>>;
pub type SupportResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Clone, Copy, Debug)]
pub enum FindChip {
    CaseSensitive,
    WholeWord,
    Regex,
    Scope,
}

#[derive(Clone, Copy, Debug)]
pub enum FileConflictAction {
    Reload,
    KeepMine,
    Dismiss,
}

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
    config_home: PathBuf,
    state_home: PathBuf,
    drop_handled: bool,
}

impl ScratchpadSession {
    pub fn new(label: &str) -> SupportResult<Self> {
        let display = Display::from_env()?;
        let binary = editor_binary()?;
        let root = temp_dir(&format!("lst-real-x11-{label}"))?;
        // Quitting hands the clipboards to a process that outlives the
        // editor, so start each test from contents no earlier test produced.
        let sentinel = format!("clipboard before {}", root.display());
        write_clipboard_text(Selection::Clipboard, &sentinel)?;
        write_clipboard_text(Selection::Primary, &sentinel)?;
        let artifacts = root.join("artifacts");
        let home = root.join("home");
        let config_home = root.join("config");
        let state_home = root.join("state");
        fs::create_dir_all(&artifacts)?;
        fs::create_dir_all(&home)?;
        fs::create_dir_all(&config_home)?;
        fs::create_dir_all(&state_home)?;
        env::set_var("LST_X11_ARTIFACT_DIR", &artifacts);
        Ok(Self {
            display,
            binary,
            root,
            artifacts,
            home,
            config_home,
            state_home,
            drop_handled: false,
        })
    }

    /// Spawn the editor against a fresh scratchpad subdirectory, focus the
    /// window for keyboard input, and return the autosave path the editor
    /// will write to plus the focused [`Editor`] handle. The autosave path
    /// already exists at return time.
    pub fn open(&mut self, name: &str) -> SupportResult<(Editor<'_>, PathBuf)> {
        self.open_scratchpad(name, &[], false, false)
    }

    pub fn open_vim(&mut self, name: &str) -> SupportResult<(Editor<'_>, PathBuf)> {
        self.open_scratchpad(name, &[], true, false)
    }

    /// Same as [`open`], but exports `extra_env` to the spawned editor.
    /// Threads through to `SpawnOpts::extra_env` so boundary fixtures (like a
    /// `prompt-add` executable on PATH) can be selected per-test
    /// without leaking into the parent process or other tests.
    pub fn open_with_env(
        &mut self,
        name: &str,
        extra_env: &[(&OsStr, &OsStr)],
    ) -> SupportResult<(Editor<'_>, PathBuf)> {
        self.open_scratchpad(name, extra_env, false, false)
    }

    pub fn open_dictation_with_env(
        &mut self,
        name: &str,
        extra_env: &[(&OsStr, &OsStr)],
    ) -> SupportResult<(Editor<'_>, PathBuf)> {
        self.open_scratchpad(name, extra_env, false, true)
    }

    fn open_scratchpad(
        &mut self,
        name: &str,
        extra_env: &[(&OsStr, &OsStr)],
        vim: bool,
        dictate: bool,
    ) -> SupportResult<(Editor<'_>, PathBuf)> {
        let dir = self.root.join(name);
        fs::create_dir_all(&dir)?;
        let title = unique_title(name);
        let mut args = Vec::with_capacity(3);
        if vim {
            args.push(OsStr::new("--vim"));
        }
        if dictate {
            args.push(OsStr::new("--dictate"));
        } else {
            args.push(OsStr::new("--scratchpad-dir"));
            args.push(dir.as_os_str());
        }
        let stderr_log_path = self.stderr_log_path(name);
        let state_trace_path = self.state_trace_path(name);
        let (stdout, stderr) = self.log_stdio(name)?;
        let env = spawn_env(&self.home, &self.config_home, &self.state_home, extra_env);
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
        let path = if dictate {
            PathBuf::from(
                editor
                    .wait_state("dictation note", SCRATCHPAD_DISCOVERY, |s| {
                        s.voice_status.as_deref().is_some_and(|s| s.starts_with("Recording"))
                    })?
                    .active_tab_path
                    .ok_or("missing voice note path")?,
            )
        } else {
            wait_for_single_file(&dir, SCRATCHPAD_DISCOVERY)?
        };
        editor.click_center()?;
        editor.wait_quiet(FOCUS_QUIET, FOCUS_TIMEOUT)?;
        editor.wait_text_viewport(FOCUS_TIMEOUT)?;
        Ok((editor, path))
    }

    /// Spawn the editor with the given file path as a positional arg and
    /// return the focused [`Editor`] handle.
    pub fn open_file(&mut self, name: &str, file: &Path) -> SupportResult<Editor<'_>> {
        self.open_files_with_mode(name, &[file.to_path_buf()], false)
    }

    pub fn open_file_with_env(
        &mut self,
        name: &str,
        file: &Path,
        extra_env: &[(&OsStr, &OsStr)],
    ) -> SupportResult<Editor<'_>> {
        self.open_files_with_mode_and_env(name, &[file.to_path_buf()], false, extra_env)
    }

    pub fn open_vim_file(&mut self, name: &str, file: &Path) -> SupportResult<Editor<'_>> {
        self.open_files_with_mode(name, &[file.to_path_buf()], true)
    }

    /// Spawn the editor with the given file paths as positional args and
    /// return the focused [`Editor`] handle.
    pub fn open_files(&mut self, name: &str, files: &[PathBuf]) -> SupportResult<Editor<'_>> {
        self.open_files_with_mode(name, files, false)
    }

    fn open_files_with_mode(&mut self, name: &str, files: &[PathBuf], vim: bool) -> SupportResult<Editor<'_>> {
        self.open_files_with_mode_and_env(name, files, vim, &[])
    }

    fn open_files_with_mode_and_env(
        &mut self,
        name: &str,
        files: &[PathBuf],
        vim: bool,
        extra_env: &[(&OsStr, &OsStr)],
    ) -> SupportResult<Editor<'_>> {
        let title = unique_title(name);
        let mut args = Vec::with_capacity(files.len() + usize::from(vim));
        if vim {
            args.push(OsStr::new("--vim"));
        }
        args.extend(files.iter().map(|file| file.as_os_str()));
        let stderr_log_path = self.stderr_log_path(name);
        let state_trace_path = self.state_trace_path(name);
        let (stdout, stderr) = self.log_stdio(name)?;
        let env = spawn_env(&self.home, &self.config_home, &self.state_home, extra_env);
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

    pub fn seed_settings(&self, contents: &str) -> SupportResult<PathBuf> {
        let path = self.config_home.join("lst").join("config.toml");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, contents)?;
        Ok(path)
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
        eprintln!("support: preserving temp dir after {reason}: {}", self.root.display());
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
    config_home: &'a Path,
    state_home: &'a Path,
    extra_env: &'a [(&'a OsStr, &'a OsStr)],
) -> Vec<(&'a OsStr, &'a OsStr)> {
    let mut env = vec![
        (OsStr::new("HOME"), home.as_os_str()),
        (OsStr::new("XDG_CONFIG_HOME"), config_home.as_os_str()),
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

pub fn run_x11_test(label: &str, test: impl FnOnce(&mut ScratchpadSession) -> TestResult) -> TestResult {
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
    fn expect_cursor_heads(&mut self, cursors: &[(usize, usize)]) -> SupportResult<StateTraceRecord>;

    /// Assert the find panel is visible with the given query and match count.
    fn expect_find_state(&mut self, query: &str, match_count: usize) -> SupportResult<StateTraceRecord>;

    /// Send `ignored` keys that an open surface must swallow, then `fence`,
    /// a key the surface does react to. X events are handled in order, so
    /// once `fenced` holds every ignored key has been processed, and every
    /// state recorded on the way must satisfy `unchanged`.
    fn expect_keys_ignored(
        &mut self,
        ignored: &str,
        fence: &str,
        fenced: impl Fn(&StateTraceRecord) -> bool,
        unchanged: impl Fn(&StateTraceRecord) -> bool,
    ) -> SupportResult<StateTraceRecord>;

    /// Click one of the visible find-panel option chips.
    fn click_find_chip(&mut self, chip: FindChip) -> SupportResult<()>;

    /// Click the status-bar cleanup (sparkle) button.
    fn click_cleanup_button(&mut self) -> SupportResult<()>;

    /// Click the visible tab-strip application-menu button.
    fn click_app_menu_button(&mut self) -> SupportResult<()>;

    /// Click the pinned tab-strip control that lists every open tab.
    fn click_all_tabs_button(&mut self) -> SupportResult<()>;

    /// Click the visible tab-strip recent-files button.
    fn click_recent_files_button(&mut self) -> SupportResult<()>;

    /// Click the visible tab-strip new-tab button.
    fn click_new_tab_button(&mut self) -> SupportResult<()>;

    /// Click one of the active document's external-change banner actions.
    fn click_file_conflict_action(&mut self, action: FileConflictAction) -> SupportResult<()>;
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
        let result = self.wait_file_text(path, expected, FileWaitOpts::new(FILE_WAIT_TIMEOUT, FILE_STABLE));
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

    fn expect_cursor_heads(&mut self, cursors: &[(usize, usize)]) -> SupportResult<StateTraceRecord> {
        self.wait_state("cursor heads", FOCUS_TIMEOUT, |record| {
            let actual: Vec<(usize, usize)> = record.cursors.iter().map(|c| (c.head_line, c.head_col)).collect();
            actual.as_slice() == cursors
        })
    }

    fn expect_find_state(&mut self, query: &str, match_count: usize) -> SupportResult<StateTraceRecord> {
        self.wait_state("find state", FOCUS_TIMEOUT, |record| {
            record.find.visible && record.find.query == query && record.find.match_count == match_count
        })
    }

    fn expect_keys_ignored(
        &mut self,
        ignored: &str,
        fence: &str,
        fenced: impl Fn(&StateTraceRecord) -> bool,
        unchanged: impl Fn(&StateTraceRecord) -> bool,
    ) -> SupportResult<StateTraceRecord> {
        self.drain_state_records()?;
        self.send_keys_settle(ignored)?;
        self.send_keys_settle(fence)?;
        let deadline = Instant::now() + FOCUS_TIMEOUT;
        loop {
            for record in self.drain_state_records()? {
                if !unchanged(&record) {
                    return Err(format!("{ignored:?} changed state before {fence:?} took effect: {record:#?}").into());
                }
                if fenced(&record) {
                    return Ok(record);
                }
            }
            if Instant::now() >= deadline {
                return Err(format!("fence key {fence:?} never took effect after {ignored:?}").into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn click_find_chip(&mut self, chip: FindChip) -> SupportResult<()> {
        let record = self.wait_state("find chip bounds", FOCUS_TIMEOUT, |state| {
            state.find.visible && find_chip_bounds(state, chip).is_some()
        })?;
        let (ox, oy, w, h) = find_chip_bounds(&record, chip).ok_or("find chip bounds missing after wait")?;
        click_trace_bounds_center(self, ox, oy, w, h, record.viewport.scale_factor, "find-chip-click")
    }

    fn click_cleanup_button(&mut self) -> SupportResult<()> {
        let record = self.wait_state("cleanup button bounds", FOCUS_TIMEOUT, |state| {
            state.cleanup_button_bounds_px.is_some()
        })?;
        let (ox, oy, w, h) = record
            .cleanup_button_bounds_px
            .ok_or("cleanup button bounds missing after wait")?;
        click_trace_bounds_center(self, ox, oy, w, h, record.viewport.scale_factor, "cleanup-click")
    }

    fn click_app_menu_button(&mut self) -> SupportResult<()> {
        let record = self.wait_state("app menu button bounds", FOCUS_TIMEOUT, |state| {
            state.app_menu_button_bounds_px.is_some()
        })?;
        let (ox, oy, w, h) = record
            .app_menu_button_bounds_px
            .ok_or("app menu button bounds missing after wait")?;
        click_trace_bounds_center(self, ox, oy, w, h, record.viewport.scale_factor, "app-menu-click")
    }

    fn click_all_tabs_button(&mut self) -> SupportResult<()> {
        let record = self.wait_state("all tabs button bounds", FOCUS_TIMEOUT, |state| {
            state.all_tabs_button_bounds_px.is_some()
        })?;
        let (ox, oy, w, h) = record
            .all_tabs_button_bounds_px
            .ok_or("all tabs button bounds missing after wait")?;
        click_trace_bounds_center(self, ox, oy, w, h, record.viewport.scale_factor, "all-tabs-click")
    }

    fn click_recent_files_button(&mut self) -> SupportResult<()> {
        let record = self.wait_state("recent button bounds", FOCUS_TIMEOUT, |state| {
            state.recent_button_bounds_px.is_some()
        })?;
        let (ox, oy, w, h) = record
            .recent_button_bounds_px
            .ok_or("recent button bounds missing after wait")?;
        click_trace_bounds_center(self, ox, oy, w, h, record.viewport.scale_factor, "recent-button-click")
    }

    fn click_new_tab_button(&mut self) -> SupportResult<()> {
        let record = self.wait_state("new tab button bounds", FOCUS_TIMEOUT, |state| {
            state.new_tab_button_bounds_px.is_some()
        })?;
        let (ox, oy, w, h) = record
            .new_tab_button_bounds_px
            .ok_or("new tab button bounds missing after wait")?;
        click_trace_bounds_center(self, ox, oy, w, h, record.viewport.scale_factor, "new-tab-button-click")
    }

    fn click_file_conflict_action(&mut self, action: FileConflictAction) -> SupportResult<()> {
        let record = self.wait_state("file conflict action bounds", FOCUS_TIMEOUT, |state| {
            state.file_conflict_path.is_some() && file_conflict_action_bounds(state, action).is_some()
        })?;
        let (ox, oy, w, h) =
            file_conflict_action_bounds(&record, action).ok_or("file conflict action bounds missing after wait")?;
        click_trace_bounds_center(
            self,
            ox,
            oy,
            w,
            h,
            record.viewport.scale_factor,
            "file-conflict-action-click",
        )
    }
}

/// A selection as `(anchor, head)`, each a `(line, col)` position.
pub type SelectionSpan = ((usize, usize), (usize, usize));

/// Selection-state expectations and the selection gestures the harness
/// cannot express directly.
pub trait SelectionTestExt {
    /// Wait until the newest trace record holds exactly `selections`, in
    /// document order.
    fn expect_selections(&mut self, selections: &[SelectionSpan]) -> SupportResult<StateTraceRecord>;

    /// Wait until the newest trace record's selections cover exactly
    /// `ranges`, each a direction-agnostic `(start, end)` char range, in
    /// document order.
    fn expect_selection_ranges(&mut self, ranges: &[(usize, usize)]) -> SupportResult<StateTraceRecord>;

    /// Click the line-number gutter beside `line`, holding Shift when
    /// `shift` is set, and wait for the selection to change.
    fn click_gutter(&mut self, line: usize, shift: bool) -> SupportResult<StateTraceRecord>;

    /// Click the empty text area one row below the last painted row.
    fn click_below_last_row(&mut self) -> SupportResult<()>;

    /// Press the left button `clicks` times at `from`, keep it down on the
    /// last press while moving to `to`, then release: a word (2) or line
    /// (3) drag. Waits for the selection to change.
    fn multi_click_drag_text(
        &mut self,
        clicks: usize,
        from: (usize, usize),
        to: (usize, usize),
    ) -> SupportResult<StateTraceRecord>;
}

impl SelectionTestExt for Editor<'_> {
    fn expect_selections(&mut self, selections: &[SelectionSpan]) -> SupportResult<StateTraceRecord> {
        self.wait_state("selections", FOCUS_TIMEOUT, |record| {
            record
                .cursors
                .iter()
                .map(|cursor| (cursor.anchor_pos(), cursor.head_pos()))
                .eq(selections.iter().copied())
        })
    }

    fn expect_selection_ranges(&mut self, ranges: &[(usize, usize)]) -> SupportResult<StateTraceRecord> {
        self.wait_state("selection ranges", FOCUS_TIMEOUT, |record| {
            record
                .cursors
                .iter()
                .map(|cursor| {
                    (
                        cursor.anchor_char.min(cursor.head_char),
                        cursor.anchor_char.max(cursor.head_char),
                    )
                })
                .eq(ranges.iter().copied())
        })
    }

    fn click_gutter(&mut self, line: usize, shift: bool) -> SupportResult<StateTraceRecord> {
        let before = self.wait_state("gutter row geometry", FOCUS_TIMEOUT, |record| {
            record.viewport.gutter_width_px > 0.0
                && record.viewport.bounds_origin_px.is_some()
                && record.viewport.first_row_for_line(line).is_some()
        })?;
        let viewport = &before.viewport;
        let (origin_x, _) = viewport.bounds_origin_px.ok_or("viewport origin missing after wait")?;
        let top = viewport
            .first_row_for_line(line)
            .ok_or("gutter row missing after wait")?
            .top_px;
        let mut held = HeldKeys::new()?;
        if shift {
            held.press(XK_SHIFT_L)?;
        }
        click_trace_bounds_center(
            self,
            origin_x,
            top,
            viewport.gutter_width_px,
            viewport.line_height_px,
            viewport.scale_factor,
            "gutter-click",
        )?;
        let selection = |record: &StateTraceRecord| {
            record
                .cursors
                .iter()
                .map(|cursor| (cursor.anchor_char, cursor.head_char))
                .collect::<Vec<_>>()
        };
        let changed = self.wait_state("gutter click selection", FOCUS_TIMEOUT, |record| {
            record.seq > before.seq && selection(record) != selection(&before)
        })?;
        held.release_all()?;
        Ok(changed)
    }

    fn click_below_last_row(&mut self) -> SupportResult<()> {
        let record = self.wait_state("painted rows", FOCUS_TIMEOUT, |record| {
            !record.viewport.rows.is_empty() && record.viewport.bounds_origin_px.is_some()
        })?;
        let viewport = &record.viewport;
        let (origin_x, _) = viewport.bounds_origin_px.ok_or("viewport origin missing after wait")?;
        let (width, _) = viewport.bounds_size_px.ok_or("viewport size missing")?;
        let last_top = viewport.rows.last().ok_or("rows missing after wait")?.top_px;
        click_trace_bounds_center(
            self,
            origin_x,
            last_top + viewport.line_height_px,
            width,
            viewport.line_height_px,
            viewport.scale_factor,
            "below-last-row-click",
        )
    }

    fn multi_click_drag_text(
        &mut self,
        clicks: usize,
        from: (usize, usize),
        to: (usize, usize),
    ) -> SupportResult<StateTraceRecord> {
        use x11rb::connection::Connection as _;
        use x11rb::protocol::xproto::{self, ConnectionExt as _};
        use x11rb::protocol::xtest::ConnectionExt as _;

        let before = self.wait_state("multi-click drag geometry", FOCUS_TIMEOUT, |record| {
            record.viewport.text_to_window_local(from.0, from.1).is_some()
                && record.viewport.text_to_window_local(to.0, to.1).is_some()
        })?;
        let local = |(line, col): (usize, usize)| {
            before
                .viewport
                .text_to_window_local(line, col)
                .ok_or("drag endpoint left the viewport")
        };
        let (from_x, from_y) = local(from)?;
        let (to_x, to_y) = local(to)?;
        let (conn, screen) = x11rb::connect(None)?;
        let root = conn.setup().roots[screen].root;
        let to_root = |x: i32, y: i32| -> SupportResult<(i16, i16)> {
            let reply = conn
                .translate_coordinates(self.window_id(), root, x as i16, y as i16)?
                .reply()?;
            Ok((reply.dst_x, reply.dst_y))
        };
        let from_root = to_root(from_x, from_y)?;
        let to_root = to_root(to_x, to_y)?;
        let button = |kind: u8, (x, y): (i16, i16)| conn.xtest_fake_input(kind, 1, 0, root, x, y, 0).map(|_| ());
        conn.warp_pointer(x11rb::NONE, root, 0, 0, 0, 0, from_root.0, from_root.1)?;
        conn.flush()?;
        std::thread::sleep(POINTER_SETTLE);
        for click in 0..clicks {
            button(xproto::BUTTON_PRESS_EVENT, from_root)?;
            conn.flush()?;
            std::thread::sleep(Duration::from_millis(5));
            if click + 1 < clicks {
                button(xproto::BUTTON_RELEASE_EVENT, from_root)?;
                conn.flush()?;
                std::thread::sleep(Duration::from_millis(25));
            }
        }
        conn.warp_pointer(x11rb::NONE, root, 0, 0, 0, 0, to_root.0, to_root.1)?;
        conn.flush()?;
        std::thread::sleep(POINTER_SETTLE);
        button(xproto::BUTTON_RELEASE_EVENT, to_root)?;
        conn.get_input_focus()?.reply()?;
        let changed = self.wait_state("multi-click drag selection", FOCUS_TIMEOUT, |record| {
            record.seq > before.seq
                && record
                    .cursors
                    .iter()
                    .map(|cursor| (cursor.anchor_char, cursor.head_char))
                    .ne(before
                        .cursors
                        .iter()
                        .map(|cursor| (cursor.anchor_char, cursor.head_char)))
        })?;
        Ok(changed)
    }
}

const POINTER_SETTLE: Duration = Duration::from_millis(50);
pub const XK_SHIFT_L: u32 = 0xffe1;
pub const XK_LEFT: u32 = 0xff51;
pub const XK_UP: u32 = 0xff52;
pub const XK_RIGHT: u32 = 0xff53;
pub const XK_DOWN: u32 = 0xff54;

/// Keys held down through a separate XTEST connection until released or
/// dropped, for input the key DSL cannot express: auto-repeat and
/// modifier-held clicks. Each press is flushed and synchronized with the
/// server before it returns, so later input from the harness connection
/// arrives while the key is down.
pub struct HeldKeys {
    conn: x11rb::rust_connection::RustConnection,
    root: u32,
    keysyms_per_keycode: usize,
    min_keycode: u8,
    keysyms: Vec<u32>,
    pressed: Vec<u8>,
}

impl HeldKeys {
    pub fn new() -> SupportResult<Self> {
        use x11rb::connection::Connection as _;
        use x11rb::protocol::xproto::ConnectionExt as _;

        let (conn, screen) = x11rb::connect(None)?;
        let setup = conn.setup();
        let root = setup.roots[screen].root;
        let min_keycode = setup.min_keycode;
        let mapping = conn
            .get_keyboard_mapping(min_keycode, setup.max_keycode - min_keycode + 1)?
            .reply()?;
        Ok(Self {
            conn,
            root,
            keysyms_per_keycode: usize::from(mapping.keysyms_per_keycode),
            min_keycode,
            keysyms: mapping.keysyms,
            pressed: Vec::new(),
        })
    }

    pub fn press(&mut self, keysym: u32) -> SupportResult<()> {
        let index = self
            .keysyms
            .chunks(self.keysyms_per_keycode)
            .position(|symbols| symbols.contains(&keysym))
            .ok_or_else(|| format!("no keycode for keysym {keysym:#x}"))?;
        let code = self.min_keycode + u8::try_from(index)?;
        self.fake_key(x11rb::protocol::xproto::KEY_PRESS_EVENT, code)?;
        self.pressed.push(code);
        Ok(())
    }

    pub fn release_all(&mut self) -> SupportResult<()> {
        while let Some(code) = self.pressed.pop() {
            self.fake_key(x11rb::protocol::xproto::KEY_RELEASE_EVENT, code)?;
        }
        Ok(())
    }

    fn fake_key(&self, kind: u8, code: u8) -> SupportResult<()> {
        use x11rb::protocol::xproto::ConnectionExt as _;
        use x11rb::protocol::xtest::ConnectionExt as _;

        self.conn.xtest_fake_input(kind, code, 0, self.root, 0, 0, 0)?;
        // A round trip proves the server processed the event; synthetic
        // events still pending when a client disconnects may be discarded.
        self.conn.get_input_focus()?.reply()?;
        Ok(())
    }
}

impl Drop for HeldKeys {
    fn drop(&mut self) {
        let _ = self.release_all();
    }
}

fn click_trace_bounds_center(
    editor: &mut Editor<'_>,
    ox: f32,
    oy: f32,
    w: f32,
    h: f32,
    scale_factor: f32,
    artifact: &str,
) -> SupportResult<()> {
    let scale = if scale_factor > 0.0 { scale_factor } else { 1.0 };
    let cx = ((ox + w * 0.5) * scale).round() as i32;
    let cy = ((oy + h * 0.5) * scale).round() as i32;
    let result = editor.click_at(cx, cy);
    with_window_artifact(editor, artifact, result)?;
    Ok(())
}

fn find_chip_bounds(record: &StateTraceRecord, chip: FindChip) -> Option<(f32, f32, f32, f32)> {
    match chip {
        FindChip::CaseSensitive => record.find.chip_bounds_px.case_sensitive,
        FindChip::WholeWord => record.find.chip_bounds_px.whole_word,
        FindChip::Regex => record.find.chip_bounds_px.regex,
        FindChip::Scope => record.find.chip_bounds_px.scope,
    }
}

fn file_conflict_action_bounds(record: &StateTraceRecord, action: FileConflictAction) -> Option<(f32, f32, f32, f32)> {
    match action {
        FileConflictAction::Reload => record.file_conflict_button_bounds_px.reload,
        FileConflictAction::KeepMine => record.file_conflict_button_bounds_px.keep_mine,
        FileConflictAction::Dismiss => record.file_conflict_button_bounds_px.dismiss,
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

fn editor_binary() -> SupportResult<PathBuf> {
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
    Err("could not find lst binary; run `cargo build -p lst-gpui --bin lst` or set LST_GPUI_BIN".into())
}

fn wait_for_single_file(dir: &Path, timeout: Duration) -> SupportResult<PathBuf> {
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

fn temp_dir(label: &str) -> SupportResult<PathBuf> {
    let dir = env::temp_dir().join(format!("{label}-{}", unique_id()));
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn unique_title(label: &str) -> String {
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

pub fn secs(value: u64) -> Duration {
    Duration::from_secs(value)
}

/// The path as the state trace reports it (`active_tab_path`, recent-panel
/// selection, close-prompt and quit-review identities).
pub fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Poll `path` until its contents satisfy `predicate` and return them. For
/// files the editor rewrites whole, such as `config.toml`, where only part of
/// the text is the subject.
pub fn wait_file_matching(path: &Path, label: &str, predicate: impl Fn(&str) -> bool) -> SupportResult<String> {
    let deadline = Instant::now() + FILE_WAIT_TIMEOUT;
    loop {
        let text = fs::read_to_string(path).unwrap_or_default();
        if predicate(&text) {
            return Ok(text);
        }
        if Instant::now() >= deadline {
            return Err(format!("timed out waiting for {label} in {}:\n{text}", path.display()).into());
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// Close the active tab without quitting: closing the only tab quits the
/// editor, so first open a sibling scratchpad and return to the tab. The tab
/// must close without a prompt. Returns the record with the sibling active.
pub fn close_active_tab_keeping_window(editor: &mut Editor<'_>) -> SupportResult<StateTraceRecord> {
    let tab_id = editor.read_state()?.active_tab_id;
    editor.keys("<C-n>")?;
    editor.wait_state("sibling tab active", FOCUS_TIMEOUT, |record| {
        record.active_tab_id != tab_id
    })?;
    editor.keys("<C-S-tab>")?;
    editor.wait_state("tab to close active again", FOCUS_TIMEOUT, |record| {
        record.active_tab_id == tab_id
    })?;
    editor.keys("<C-w>")?;
    editor.wait_state("tab closed", FOCUS_TIMEOUT, |record| record.active_tab_id != tab_id)
}

/// Sets a path's permission bits and restores the original bits on drop, so
/// a failing test does not leave an unwritable directory behind.
#[cfg(unix)]
pub struct RestorePermissions {
    path: PathBuf,
    original: fs::Permissions,
}

#[cfg(unix)]
impl RestorePermissions {
    pub fn set_mode(path: &Path, mode: u32) -> SupportResult<Self> {
        use std::os::unix::fs::PermissionsExt;

        let original = fs::metadata(path)?.permissions();
        fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
        Ok(Self {
            path: path.to_path_buf(),
            original,
        })
    }
}

#[cfg(unix)]
impl Drop for RestorePermissions {
    fn drop(&mut self) {
        let _ = fs::set_permissions(&self.path, self.original.clone());
    }
}
