use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use x11rb::connection::Connection as _;
use x11rb::protocol::damage::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{self, ConnectionExt as _, Keycode};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;
use x11rb::NONE;

use crate::display::Display;
use crate::screenshot::{self, Screenshot};
use crate::state_trace::{StateTraceReader, StateTraceRecord};
use crate::x11::damage as damage_wait;
use crate::x11::input::{self, BUTTON_LEFT, BUTTON_MIDDLE, BUTTON_WHEEL_DOWN, BUTTON_WHEEL_UP, POINTER_SETTLE};
use crate::x11::keycodes::Keycodes;
use crate::x11::window::{self, WindowInfo};
use crate::Result;

const WINDOW_DISCOVERY_TIMEOUT_DEFAULT: Duration = Duration::from_secs(30);
const TERMINATE_GRACE: Duration = Duration::from_secs(2);
const SEND_KEYS_QUIET: Duration = Duration::from_millis(20);
const SEND_KEYS_TIMEOUT: Duration = Duration::from_secs(2);
const STATE_POLL: Duration = Duration::from_millis(10);
const TEXT_VIEWPORT_TIMEOUT: Duration = Duration::from_secs(2);

fn window_discovery_timeout() -> Duration {
    // GPUI's X11 backend is reasonably fast, but on hosts where the window
    // manager runs focus-stealing checks (lwm in particular), the window
    // can stay unmapped for 10s+ after the process starts. Default
    // generously, but let CI tune via env var.
    std::env::var("LST_X11_WINDOW_TIMEOUT_MS")
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or(WINDOW_DISCOVERY_TIMEOUT_DEFAULT)
}

#[derive(Clone, Copy, Debug)]
pub enum Key {
    Char(char),
    Tab,
    Space,
    Enter,
    Escape,
    Backspace,
    Delete,
    Insert,
    F2,
    Home,
    End,
    Left,
    Right,
    Up,
    Down,
    PageUp,
    PageDown,
}

#[derive(Clone, Copy, Debug)]
pub enum KeyChord {
    Key(Key),
    Ctrl(Key),
}

#[derive(Clone, Copy, Debug)]
pub enum WheelDir {
    Up,
    Down,
}

pub struct SpawnOpts<'a> {
    pub binary: &'a Path,
    pub args: &'a [&'a OsStr],
    /// Unique `--title` value used to discover the editor's X11 window.
    pub title: &'a str,
    pub stderr: Stdio,
    pub stdout: Stdio,
    /// Extra environment variables to set on the editor process. Lets the
    /// bench wire its `LST_BENCH_TRACE_FILE` without baking the name into
    /// the harness.
    pub extra_env: &'a [(&'a OsStr, &'a OsStr)],
    /// Path the editor's stderr is being captured to (typically a file the
    /// caller passes into `stderr` as `Stdio::from(File)`). When set, harness
    /// errors include the last N non-empty lines of this file so a panicking
    /// editor surfaces its trail without forcing callers to dig through
    /// per-test artifact directories.
    pub stderr_log_path: Option<&'a Path>,
    /// Path the editor will append state-trace JSONL records to. When set,
    /// the harness exports `LST_X11_STATE_TRACE_FILE` to the editor and
    /// constructs a [`StateTraceReader`] on the returned [`Editor`].
    pub state_trace_path: Option<&'a Path>,
}

#[derive(Clone, Copy, Debug)]
pub struct FileWaitOpts {
    pub stable_for: Duration,
    pub timeout: Duration,
}

impl FileWaitOpts {
    pub fn new(timeout: Duration, stable_for: Duration) -> Self {
        Self { stable_for, timeout }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct FileStats {
    pub bytes: u64,
    pub lines: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct FileWaitOutcome {
    pub stats: FileStats,
    pub damage_events: u64,
}

/// Handle to a spawned editor. Owns the child process and its DAMAGE
/// subscription; borrows the [`Display`] for the connection, atoms, and
/// keycodes. A single [`Display`] supports one [`Editor`] at a time:
/// `spawn_editor` takes `&mut self`, and the returned `Editor<'a>` keeps
/// the borrow until it's dropped or `quit`-ed.
pub struct Editor<'a> {
    display: &'a Display,
    child: Option<Child>,
    window: WindowInfo,
    damage: damage::DamageWrapper<&'a RustConnection>,
    stderr_log_path: Option<PathBuf>,
    state_trace: Option<StateTraceReader>,
}

const STDERR_TAIL_LINES: usize = 20;

impl Display {
    /// Spawn the editor binary, find its window by `_NET_WM_PID` + title,
    /// attach DAMAGE, and return a handle. Errors during discovery or
    /// damage attachment terminate the spawned child before returning.
    pub fn spawn_editor<'a>(&'a mut self, opts: SpawnOpts<'_>) -> Result<Editor<'a>> {
        let title = opts.title;
        let stderr_log_path = opts.stderr_log_path.map(PathBuf::from);
        let state_trace = opts.state_trace_path.map(StateTraceReader::new);
        let mut child = build_command(self, opts).spawn()?;
        let pid = child.id();

        let info = match window::find(
            &self.conn,
            self.root,
            &self.atoms,
            pid,
            title,
            &mut child,
            window_discovery_timeout(),
        ) {
            Ok(info) => info,
            Err(error) => {
                terminate(&mut child);
                return Err(error);
            }
        };

        // Reborrow `self` immutably; DAMAGE wrapper holds &display.conn for
        // its full lifetime, and the Editor stores &display alongside.
        let display: &'a Display = self;
        let damage = match damage::DamageWrapper::create(&display.conn, info.id, damage::ReportLevel::NON_EMPTY) {
            Ok(damage) => damage,
            Err(error) => {
                terminate(&mut child);
                return Err(error.into());
            }
        };
        display.conn.flush()?;
        release_stale_modifiers(display)?;

        Ok(Editor {
            display,
            child: Some(child),
            window: info,
            damage,
            stderr_log_path,
            state_trace,
        })
    }
}

fn build_command(display: &Display, opts: SpawnOpts<'_>) -> Command {
    let mut command = Command::new(opts.binary);
    command
        .arg("--title")
        .arg(opts.title)
        .stdin(Stdio::null())
        .stdout(opts.stdout)
        .stderr(opts.stderr)
        .env("DISPLAY", &display.session_env.display);
    if let Some(xauthority) = &display.session_env.xauthority {
        command.env("XAUTHORITY", xauthority);
    }
    if let Some(dbus) = &display.session_env.dbus_session_bus_address {
        command.env("DBUS_SESSION_BUS_ADDRESS", dbus);
    }
    if let Some(state_trace_path) = opts.state_trace_path {
        command.env("LST_X11_STATE_TRACE_FILE", state_trace_path);
    }
    for (key, value) in opts.extra_env {
        command.env(key, value);
    }
    for arg in opts.args {
        command.arg(arg);
    }
    command
}

fn release_stale_modifiers(display: &Display) -> Result<()> {
    input::release_all_modifiers(&display.conn, display.root, &display.keycodes)?;
    display.conn.flush()?;
    thread::sleep(input::KEY_PHASE_SETTLE);
    Ok(())
}

impl<'a> Editor<'a> {
    pub fn window_id(&self) -> xproto::Window {
        self.window.id
    }

    pub fn screenshot(&self) -> Result<Screenshot> {
        screenshot::capture_window(&self.display.conn, self.display.root, self.window.id)
    }

    pub fn is_viewable(&self) -> Result<bool> {
        window::is_viewable(&self.display.conn, self.window.id)
    }

    fn focus_for_keyboard(&mut self) -> Result<()> {
        self.display
            .conn
            .set_input_focus(xproto::InputFocus::PARENT, self.window.id, x11rb::CURRENT_TIME)?;
        self.display.conn.flush()?;
        thread::sleep(POINTER_SETTLE);
        Ok(())
    }

    pub fn click_center(&mut self) -> Result<()> {
        input::move_pointer_to_window_center(&self.display.conn, self.display.root, &self.window)?;
        thread::sleep(POINTER_SETTLE);
        input::click_button(&self.display.conn, self.display.root, BUTTON_LEFT)
    }

    pub fn click_at(&mut self, local_x: i32, local_y: i32) -> Result<()> {
        input::move_pointer_to_window_point(&self.display.conn, self.display.root, &self.window, local_x, local_y)?;
        thread::sleep(POINTER_SETTLE);
        input::click_button(&self.display.conn, self.display.root, BUTTON_LEFT)
    }

    /// Single left click at the given (line, col) text position. Resolves
    /// pixels via the latest state-trace record's viewport geometry. Errors
    /// when the line is not in the painted-rows window — caller must
    /// scroll-into-view first, or the viewport must have painted at least
    /// once.
    pub fn click_at_text(&mut self, line: usize, col: usize) -> Result<()> {
        self.mouse_click_at_text(line, col, ChordMods::default(), BUTTON_LEFT, 1, "click_at_text")
    }

    pub fn shift_click_at_text(&mut self, line: usize, col: usize) -> Result<()> {
        self.mouse_click_at_text(line, col, ChordMods::SHIFT, BUTTON_LEFT, 1, "shift_click_at_text")
    }

    pub fn alt_click_at_text(&mut self, line: usize, col: usize) -> Result<()> {
        self.mouse_click_at_text(line, col, ChordMods::ALT, BUTTON_LEFT, 1, "alt_click_at_text")
    }

    pub fn double_click_at_text(&mut self, line: usize, col: usize) -> Result<()> {
        self.mouse_click_at_text(line, col, ChordMods::default(), BUTTON_LEFT, 2, "double_click_at_text")
    }

    pub fn triple_click_at_text(&mut self, line: usize, col: usize) -> Result<()> {
        self.mouse_click_at_text(line, col, ChordMods::default(), BUTTON_LEFT, 3, "triple_click_at_text")
    }

    pub fn quad_click_at_text(&mut self, line: usize, col: usize) -> Result<()> {
        self.mouse_click_at_text(line, col, ChordMods::default(), BUTTON_LEFT, 4, "quad_click_at_text")
    }

    pub fn middle_click_at_text(&mut self, line: usize, col: usize) -> Result<()> {
        self.mouse_click_at_text(
            line,
            col,
            ChordMods::default(),
            BUTTON_MIDDLE,
            1,
            "middle_click_at_text",
        )
    }

    /// Press at `from`, drag to `to` with optional held modifiers, release.
    /// Both endpoints must currently be in the painted-rows window.
    pub fn drag_text(&mut self, from: (usize, usize), to: (usize, usize), mods: ChordMods) -> Result<()> {
        let result: Result<()> = (|| {
            let state = self.wait_state("drag_text viewport", TEXT_VIEWPORT_TIMEOUT, |state| {
                state.viewport.text_to_window_local(from.0, from.1).is_some()
                    && state.viewport.text_to_window_local(to.0, to.1).is_some()
            })?;
            let before_seq = state.seq;
            let before_cursors = state_cursor_signature(&state);
            let (from_x, from_y) = resolve_text_pixels(&state, from.0, from.1, "drag_text from")?;
            let (to_x, to_y) = resolve_text_pixels(&state, to.0, to.1, "drag_text to")?;
            let conn = &self.display.conn;
            let root = self.display.root;
            let kc = &self.display.keycodes;
            input::move_pointer_to_window_point(conn, root, &self.window, from_x, from_y)?;
            thread::sleep(POINTER_SETTLE);
            input::press_modifiers_with_platform(conn, root, kc, mods.ctrl, mods.alt, mods.shift, mods.platform)?;
            if !mods.is_empty() {
                conn.flush()?;
                thread::sleep(POINTER_SETTLE);
            }
            input::button_press(conn, root, BUTTON_LEFT)?;
            conn.flush()?;
            // Move with the button held. The X server processes motion
            // asynchronously, so settle briefly before the release.
            input::move_pointer_to_window_point(conn, root, &self.window, to_x, to_y)?;
            conn.flush()?;
            thread::sleep(POINTER_SETTLE);
            input::button_release(conn, root, BUTTON_LEFT)?;
            input::release_modifiers_with_platform(conn, root, kc, mods.ctrl, mods.alt, mods.shift, mods.platform)?;
            conn.flush()?;
            let damage_id = self.damage.damage();
            let window_id = self.window.id;
            let child = child_mut(&mut self.child)?;
            damage_wait::wait_for_damage_then_quiet(
                conn,
                damage_id,
                window_id,
                child,
                SEND_KEYS_QUIET,
                SEND_KEYS_TIMEOUT,
            )?;
            self.wait_state("drag_text selection", TEXT_VIEWPORT_TIMEOUT, |state| {
                state.seq > before_seq
                    && state_cursor_signature(state) != before_cursors
                    && (state.cursors.len() > 1 || state.cursors.iter().any(|cursor| !cursor.is_collapsed()))
            })?;
            Ok(())
        })();
        self.attach_stderr_context(result, "drag_text")
    }

    fn mouse_click_at_text(
        &mut self,
        line: usize,
        col: usize,
        mods: ChordMods,
        button: u8,
        click_count: usize,
        label: &str,
    ) -> Result<()> {
        let result: Result<()> = (|| {
            let state = self.wait_state(label, TEXT_VIEWPORT_TIMEOUT, |state| {
                state.viewport.text_to_window_local(line, col).is_some()
            })?;
            let before_seq = state.seq;
            let before_cursors = state_cursor_signature(&state);
            let clicked_row = state
                .viewport
                .first_row_for_line(line)
                .map(|row| (row.line_start_char, row.display_end_char));
            let quad_min_end = state
                .viewport
                .first_row_for_line(line + 1)
                .map(|row| row.display_end_char);
            let (x, y) = resolve_text_pixels(&state, line, col, label)?;
            let expects_state_change = mouse_click_expects_state_change(&state, line, col, mods, button, click_count);
            let conn = &self.display.conn;
            let root = self.display.root;
            let kc = &self.display.keycodes;
            input::move_pointer_to_window_point(conn, root, &self.window, x, y)?;
            thread::sleep(POINTER_SETTLE);
            input::press_modifiers_with_platform(conn, root, kc, mods.ctrl, mods.alt, mods.shift, mods.platform)?;
            if !mods.is_empty() {
                conn.flush()?;
                thread::sleep(POINTER_SETTLE);
            }
            input::multi_click_button(conn, root, button, click_count)?;
            input::release_modifiers_with_platform(conn, root, kc, mods.ctrl, mods.alt, mods.shift, mods.platform)?;
            conn.flush()?;
            let damage_id = self.damage.damage();
            let window_id = self.window.id;
            let child = child_mut(&mut self.child)?;
            if expects_state_change {
                damage_wait::wait_for_damage_then_quiet(
                    conn,
                    damage_id,
                    window_id,
                    child,
                    SEND_KEYS_QUIET,
                    SEND_KEYS_TIMEOUT,
                )?;
            } else {
                damage_wait::wait_quiet(conn, damage_id, window_id, child, SEND_KEYS_QUIET, SEND_KEYS_TIMEOUT)?;
            }
            if click_count > 1 {
                self.wait_state(label, TEXT_VIEWPORT_TIMEOUT, |state| {
                    state.seq > before_seq
                        && multi_click_selection_reached(state, clicked_row, quad_min_end, click_count)
                })?;
            } else if expects_state_change {
                self.wait_state(label, TEXT_VIEWPORT_TIMEOUT, |state| {
                    state.seq > before_seq && state_cursor_signature(state) != before_cursors
                })?;
            }
            Ok(())
        })();
        self.attach_stderr_context(result, label)
    }

    pub fn wheel_burst(&mut self, dir: WheelDir, count: usize, total: Duration) -> Result<()> {
        let button = match dir {
            WheelDir::Up => BUTTON_WHEEL_UP,
            WheelDir::Down => BUTTON_WHEEL_DOWN,
        };
        input::wheel_burst(&self.display.conn, self.display.root, button, count, total)
    }

    pub fn press(&mut self, chord: KeyChord) -> Result<()> {
        let (key, ctrl) = match chord {
            KeyChord::Key(k) => (k, false),
            KeyChord::Ctrl(k) => (k, true),
        };
        let (code, shift) = resolve_key(&self.display.keycodes, key)?;
        self.focus_for_keyboard()?;
        input::chord(
            &self.display.conn,
            self.display.root,
            &self.display.keycodes,
            code,
            ctrl,
            false,
            shift,
        )
    }

    /// Type each character in `text` literally. Uppercase letters auto-shift;
    /// `\n` is rejected — use [`Editor::send_keys`] with `<enter>` for line
    /// breaks, since the editor's reaction to a literal newline is
    /// language-specific (auto-indent, vim insert mode rules).
    pub fn type_text(&mut self, text: &str) -> Result<()> {
        for ch in text.chars() {
            let (code, shift) = self
                .display
                .keycodes
                .lookup_char(ch)
                .ok_or_else(|| io::Error::other(format!("unsupported text char: {ch:?}")))?;
            self.focus_for_keyboard()?;
            input::chord(
                &self.display.conn,
                self.display.root,
                &self.display.keycodes,
                code,
                false,
                false,
                shift,
            )?;
        }
        Ok(())
    }

    /// Drive the editor with a vim-style key sequence. Bare characters are
    /// typed literally; `<...>` escapes name special keys and modifier
    /// chords:
    ///
    /// - Special keys: `<enter>` / `<cr>` / `<return>`, `<esc>` / `<escape>`,
    ///   `<tab>`, `<space>`, `<bs>` / `<backspace>`, `<del>` / `<delete>`,
    ///   `<ins>` / `<insert>`, `<f2>`, `<home>`, `<end>`, `<left>`, `<right>`,
    ///   `<up>`, `<down>`, `<lt>` (literal `<`).
    /// - Modifier chords: `<C-x>` for Ctrl+x, `<A-x>` for Alt+x, `<S-x>` for
    ///   Shift+x, `<C-A-S-x>` for Ctrl+Alt+Shift+x. The verbose forms
    ///   `<ctrl-x>`, `<alt-x>`, and `<shift-x>` are accepted too. Inside a
    ///   chord the key may be a single character or a special name, e.g.
    ///   `<C-A-down>`.
    ///
    /// Names are case-insensitive: `<Esc>`, `<ESC>`, and `<esc>` all match.
    /// Uppercase letters outside escapes auto-shift, so `A` and `<S-a>` are
    /// equivalent.
    ///
    /// After each non-text key, waits for a brief damage-quiet period before
    /// sending the next one. Plain text is allowed to settle without requiring
    /// per-character damage because GPUI's X11 text path can commit printable
    /// input after a following event.
    pub fn send_keys(&mut self, sequence: &str) -> Result<()> {
        let result: Result<()> = (|| {
            let tokens = parse_keys(sequence)?;
            let mut observed_state = self.state_trace.as_mut().and_then(|reader| reader.latest().ok());
            let mut pending_text_anchor: Option<StateTraceRecord> = None;
            for token in tokens {
                let before_state = observed_state.clone();
                let wait_for_state_change = before_state
                    .as_ref()
                    .is_some_and(|state| key_token_expects_state_change(&token, state));
                let batched_text_change = before_state
                    .as_ref()
                    .is_some_and(|state| key_token_has_batched_text_state_change(&token, state));
                if batched_text_change && pending_text_anchor.is_none() {
                    pending_text_anchor = before_state.clone();
                }
                self.focus_for_keyboard()?;
                self.dispatch_token(&token)?;
                // Settle: wait until the editor has painted in response. Short
                // quiet window because we just want one frame of evidence; not
                // a fully-idle UI. Uses a generous timeout so a slow paint
                // doesn't fail an otherwise good test.
                if let Err(error) = self.wait_after_dispatched_key(wait_for_state_change) {
                    if wait_for_state_change {
                        if let Some(before) = before_state.as_ref() {
                            if let Some(changed) = self.peek_latest_context_after(Some(before))? {
                                observed_state = Some(changed);
                                continue;
                            }
                            if let Ok(changed) = self.wait_state_change_after_without_consuming(before) {
                                observed_state = Some(changed);
                                continue;
                            }
                        }
                    }
                    return Err(error);
                }
                if wait_for_state_change {
                    let before_state = before_state.expect("checked above");
                    match self.wait_state_change_after_without_consuming(&before_state) {
                        Ok(state) => {
                            observed_state = Some(state);
                            pending_text_anchor = None;
                        }
                        Err(error) => return Err(error),
                    }
                } else {
                    observed_state = self.peek_latest_context_after(before_state.as_ref())?;
                }
            }
            if let Some(anchor) = pending_text_anchor {
                self.wait_state_change_after_without_consuming(&anchor)?;
            }
            Ok(())
        })();
        self.attach_stderr_context(result, "send_keys")
    }

    fn wait_after_dispatched_key(&mut self, require_damage: bool) -> Result<()> {
        let conn = &self.display.conn;
        let damage_id = self.damage.damage();
        let window_id = self.window.id;
        let child = child_mut(&mut self.child)?;
        if require_damage {
            damage_wait::wait_for_damage_then_quiet(
                conn,
                damage_id,
                window_id,
                child,
                SEND_KEYS_QUIET,
                SEND_KEYS_TIMEOUT,
            )?;
        } else {
            damage_wait::wait_quiet(conn, damage_id, window_id, child, SEND_KEYS_QUIET, SEND_KEYS_TIMEOUT)?;
        }
        Ok(())
    }

    /// Hold `mods` continuously while tapping each key in `sequence`. The
    /// editor sees one `mods-down → tap → tap → ... → mods-up` event train
    /// per call, so chord-prefix bindings (e.g. `Ctrl+K Ctrl+D`) dispatch
    /// correctly. `sequence` is parsed via the same vim-style notation as
    /// `send_keys`, but each piece must be a single key without its own
    /// `Ctrl-`/`Alt-`/`platform` modifier (Shift is allowed for inner
    /// auto-shift).
    /// Settles on a single damage-then-quiet at the end of the held span.
    pub fn with_chord_held(&mut self, mods: ChordMods, sequence: &str) -> Result<()> {
        let result: Result<()> = (|| {
            if mods.is_empty() {
                return Err(io::Error::other(
                    "with_chord_held requires at least one modifier; pass send_keys for a plain sequence",
                )
                .into());
            }
            let inner = parse_held_inner(sequence, sequence)?;
            if inner.is_empty() {
                return Err(io::Error::other("with_chord_held sequence cannot be empty").into());
            }
            let token = KeyToken::Held(KeyChordHeld { mods, inner });
            self.focus_for_keyboard()?;
            self.dispatch_token(&token)?;
            let conn = &self.display.conn;
            let damage_id = self.damage.damage();
            let window_id = self.window.id;
            let child = child_mut(&mut self.child)?;
            damage_wait::wait_for_damage_then_quiet(
                conn,
                damage_id,
                window_id,
                child,
                SEND_KEYS_QUIET,
                SEND_KEYS_TIMEOUT,
            )?;
            Ok(())
        })();
        self.attach_stderr_context(result, "with_chord_held")
    }

    /// Press and release `mods`, then tap `key` immediately afterward. This
    /// drives real X11 events for the class of synthetic delivery races where
    /// the application observes a key press just after the modifier release and
    /// must decide whether a recent modifier chord is still relevant.
    pub fn key_after_released_modifiers(&mut self, mods: ChordMods, key: Key) -> Result<()> {
        let result: Result<()> = (|| {
            if mods.is_empty() {
                return Err(io::Error::other("key_after_released_modifiers requires at least one modifier").into());
            }
            self.focus_for_keyboard()?;
            let conn = &self.display.conn;
            let root = self.display.root;
            let kc = &self.display.keycodes;
            input::press_modifiers_with_platform(conn, root, kc, mods.ctrl, mods.alt, mods.shift, mods.platform)?;
            conn.flush()?;
            thread::sleep(input::KEY_PHASE_SETTLE);
            input::release_modifiers_with_platform(conn, root, kc, mods.ctrl, mods.alt, mods.shift, mods.platform)?;
            conn.flush()?;
            thread::sleep(input::KEY_PHASE_SETTLE);

            let (code, base_shift) = resolve_key(kc, key)?;
            input::chord(conn, root, kc, code, false, false, base_shift)?;
            self.wait_after_dispatched_key(false)?;
            Ok(())
        })();
        self.attach_stderr_context(result, "key_after_released_modifiers")
    }

    /// Synthesize the X events for one parsed token without settling. Shared
    /// by `send_keys`, `send_keys_expect_quiet`, and `with_chord_held`.
    fn dispatch_token(&self, token: &KeyToken) -> Result<()> {
        match token {
            KeyToken::Single(s) => {
                let chord = dispatchable_single_chord(s);
                let (code, base_shift) = resolve_key(&self.display.keycodes, chord.key)?;
                input::chord_with_modifiers(
                    &self.display.conn,
                    self.display.root,
                    &self.display.keycodes,
                    code,
                    input::ModifierState {
                        ctrl: chord.ctrl,
                        alt: chord.alt,
                        shift: base_shift || chord.shift,
                        platform: chord.platform,
                    },
                )
            }
            KeyToken::Held(h) => {
                let conn = &self.display.conn;
                let root = self.display.root;
                let kc = &self.display.keycodes;
                input::press_modifiers_with_platform(
                    conn,
                    root,
                    kc,
                    h.mods.ctrl,
                    h.mods.alt,
                    h.mods.shift,
                    h.mods.platform,
                )?;
                conn.flush()?;
                thread::sleep(input::KEY_PHASE_SETTLE);
                for inner in &h.inner {
                    let (code, base_shift) = resolve_key(kc, inner.key)?;
                    let need_inner_shift = (base_shift || inner.shift) && !h.mods.shift;
                    if need_inner_shift {
                        input::press_modifiers(conn, root, kc, false, false, true)?;
                        conn.flush()?;
                        thread::sleep(input::KEY_PHASE_SETTLE);
                    }
                    input::tap_key(conn, root, code)?;
                    conn.flush()?;
                    thread::sleep(input::KEY_PHASE_SETTLE);
                    if need_inner_shift {
                        input::release_modifiers(conn, root, kc, false, false, true)?;
                        conn.flush()?;
                        thread::sleep(input::KEY_PHASE_SETTLE);
                    }
                }
                input::release_modifiers_with_platform(
                    conn,
                    root,
                    kc,
                    h.mods.ctrl,
                    h.mods.alt,
                    h.mods.shift,
                    h.mods.platform,
                )?;
                conn.flush()?;
                Ok(())
            }
        }
    }

    /// Drive a vim-style key sequence through the harness and assert that
    /// **no** matching DAMAGE event arrives within `deadline`. Use this for
    /// legitimate no-op assertions where `send_keys`'s "every key paints"
    /// invariant would otherwise produce a false timeout: Ctrl+D on a buffer
    /// without a current occurrence, Ctrl+S on an unmodified file, the first
    /// half of a vim compound that is still pending, and similar.
    ///
    /// Drains any pre-existing damage events first so a paint from the
    /// previous operation cannot bleed into this assertion. Then synthesizes
    /// every key without a per-key paint wait. Finally polls for `deadline`,
    /// returning `Ok(())` if the window expires cleanly and an error if any
    /// matching damage arrives or the editor exits.
    pub fn send_keys_expect_quiet(&mut self, sequence: &str, deadline: Duration) -> Result<()> {
        let result: Result<()> = (|| {
            let tokens = parse_keys(sequence)?;
            damage_wait::drain_pending(&self.display.conn, self.damage.damage(), self.window.id)?;
            for token in &tokens {
                self.focus_for_keyboard()?;
                self.dispatch_token(token)?;
            }
            let conn = &self.display.conn;
            let damage_id = self.damage.damage();
            let window_id = self.window.id;
            let child = child_mut(&mut self.child)?;
            damage_wait::expect_no_damage(conn, damage_id, window_id, child, deadline)
        })();
        self.attach_stderr_context(result, "send_keys_expect_quiet")
    }

    /// Drive a sequence and wait for the window to settle after each token
    /// without requiring either damage or a state-trace change. Use this for
    /// regression checks whose assertion is an external observable, such as a
    /// file remaining unchanged after a shortcut that should be ignored.
    pub fn send_keys_settle(&mut self, sequence: &str) -> Result<()> {
        let result: Result<()> = (|| {
            let tokens = parse_keys(sequence)?;
            for token in &tokens {
                self.focus_for_keyboard()?;
                self.dispatch_token(token)?;
                self.wait_after_dispatched_key(false)?;
            }
            Ok(())
        })();
        self.attach_stderr_context(result, "send_keys_settle")
    }

    pub fn wait_quiet(&mut self, quiet: Duration, timeout: Duration) -> Result<u64> {
        let result = (|| {
            let conn = &self.display.conn;
            let damage_id = self.damage.damage();
            let window_id = self.window.id;
            let child = child_mut(&mut self.child)?;
            damage_wait::wait_quiet(conn, damage_id, window_id, child, quiet, timeout)
        })();
        self.attach_stderr_context(result, "wait_quiet")
    }

    pub fn wait_state(
        &mut self,
        label: &str,
        timeout: Duration,
        predicate: impl Fn(&StateTraceRecord) -> bool,
    ) -> Result<StateTraceRecord> {
        let result = (|| {
            let deadline = Instant::now() + timeout;
            let mut latest = None;
            loop {
                if let Some(status) = child_mut(&mut self.child)?.try_wait()? {
                    return Err(
                        io::Error::other(format!("editor exited while waiting for state {label}: {status}")).into(),
                    );
                }

                let reader = self.state_trace.as_mut().ok_or_else(|| {
                    io::Error::other("state trace not configured; pass `SpawnOpts::state_trace_path` at spawn time")
                })?;
                let records = reader.read_new_records()?;
                for record in records {
                    latest = Some(record.clone());
                    if predicate(&record) {
                        return Ok(record);
                    }
                }
                if let Some(record) = reader.last_observed().cloned() {
                    latest = Some(record.clone());
                    if predicate(&record) {
                        return Ok(record);
                    }
                }

                if Instant::now() >= deadline {
                    let detail = latest
                        .as_ref()
                        .and_then(|record| serde_json::to_string_pretty(record).ok())
                        .unwrap_or_else(|| "<no state-trace record observed>".to_string());
                    return Err(io::Error::other(format!("timed out waiting for state {label}\n{detail}")).into());
                }
                thread::sleep(STATE_POLL);
            }
        })();
        self.attach_stderr_context(result, "wait_state")
    }

    fn wait_state_change_after_without_consuming(&mut self, before: &StateTraceRecord) -> Result<StateTraceRecord> {
        let deadline = Instant::now() + SEND_KEYS_TIMEOUT;
        let mut latest = None;
        loop {
            if let Some(status) = child_mut(&mut self.child)?.try_wait()? {
                return Err(io::Error::other(format!(
                    "editor exited while waiting for send_keys state change: {status}"
                ))
                .into());
            }

            let reader = self.state_trace.as_mut().ok_or_else(|| {
                io::Error::other("state trace not configured; pass `SpawnOpts::state_trace_path` at spawn time")
            })?;
            for record in reader.peek_new_records()? {
                latest = Some(record.clone());
                if state_key_context_changed_after(before, &record) {
                    return Ok(record);
                }
            }

            if Instant::now() >= deadline {
                let detail = latest
                    .as_ref()
                    .and_then(|record| serde_json::to_string_pretty(record).ok())
                    .unwrap_or_else(|| "<no state-trace record observed>".to_string());
                return Err(io::Error::other(format!("timed out waiting for send_keys state change\n{detail}")).into());
            }
            thread::sleep(STATE_POLL);
        }
    }

    fn peek_latest_context_after(&mut self, before: Option<&StateTraceRecord>) -> Result<Option<StateTraceRecord>> {
        let Some(reader) = self.state_trace.as_mut() else {
            return Ok(None);
        };
        let min_seq = before.map(|state| state.seq);
        let records = reader.peek_new_records()?;
        let is_candidate = |record: &StateTraceRecord| {
            min_seq.is_none_or(|seq| record.seq > seq)
                && before.is_none_or(|before| state_key_context_changed_after(before, record))
        };
        Ok(records
            .iter()
            .rfind(|record| is_candidate(record))
            .cloned()
            .or_else(|| reader.last_observed().filter(|record| is_candidate(record)).cloned()))
    }

    pub fn wait_text_viewport(&mut self, timeout: Duration) -> Result<StateTraceRecord> {
        self.wait_state("text viewport geometry", timeout, viewport_geometry_is_ready)
    }

    /// Drain any new state-trace records and return the most recent one.
    /// When no new record was appended since the previous read, returns the
    /// most recent record this reader has already observed.
    /// Errors when no state-trace path was configured at spawn time, or
    /// when the editor has not emitted any record yet.
    pub fn read_state(&mut self) -> Result<StateTraceRecord> {
        let result = match self.state_trace.as_mut() {
            Some(reader) => reader.latest(),
            None => Err(io::Error::other(
                "state trace not configured; pass `SpawnOpts::state_trace_path` at spawn time",
            )
            .into()),
        };
        self.attach_stderr_context(result, "read_state")
    }

    /// Convenience wrapper: read the latest state and run a predicate. On
    /// failure the error includes a pretty-printed dump of the offending
    /// record so test diagnostics surface what was actually observed
    /// instead of the bare predicate name.
    pub fn expect_state(
        &mut self,
        label: &str,
        predicate: impl FnOnce(&StateTraceRecord) -> bool,
    ) -> Result<StateTraceRecord> {
        let record = self.read_state()?;
        if predicate(&record) {
            Ok(record)
        } else {
            let pretty = serde_json::to_string_pretty(&record)
                .unwrap_or_else(|_| "<state-trace record could not be re-serialized for diagnostics>".to_string());
            Err(format!("expect_state {label}: predicate returned false\n{pretty}").into())
        }
    }

    /// Drain new records since the last call. Returns them in append order.
    /// Useful for asserting state across a multi-key span (e.g. one record
    /// per keystroke). Empty `Vec` when nothing new is available.
    pub fn drain_state_records(&mut self) -> Result<Vec<StateTraceRecord>> {
        let result = match self.state_trace.as_mut() {
            Some(reader) => reader.read_new_records(),
            None => Err(io::Error::other(
                "state trace not configured; pass `SpawnOpts::state_trace_path` at spawn time",
            )
            .into()),
        };
        self.attach_stderr_context(result, "drain_state_records")
    }

    pub fn wait_file_text(&mut self, path: &Path, expected: &str, opts: FileWaitOpts) -> Result<FileWaitOutcome> {
        let result = (|| {
            let display = self.display;
            let damage_id = self.damage.damage();
            let window_id = self.window.id;
            let child = child_mut(&mut self.child)?;
            wait_file_text_impl(&display.conn, damage_id, window_id, child, path, expected, opts)
        })();
        self.attach_stderr_context(result, "wait_file_text")
    }

    pub fn wait_file_stable(&mut self, path: &Path, stable_for: Duration, timeout: Duration) -> Result<FileStats> {
        let result = (|| {
            let child = child_mut(&mut self.child)?;
            wait_file_stable_impl(child, path, stable_for, timeout)
        })();
        self.attach_stderr_context(result, "wait_file_stable")
    }

    pub fn wait_for_exit(&mut self, timeout: Duration) -> Result<ExitStatus> {
        let result = (|| {
            let child = child_mut(&mut self.child)?;
            wait_child(child, timeout)
        })();
        self.attach_stderr_context(result, "wait_for_exit")
    }

    /// Send `Ctrl+Q` and wait for the editor to exit. Consumes `self`; on
    /// success the child has already been reaped, and `Drop` is a no-op.
    /// On timeout the child is force-terminated before the error is
    /// returned, so callers cannot leak the editor process.
    pub fn quit(mut self, timeout: Duration) -> Result<ExitStatus> {
        let result = (|| -> Result<ExitStatus> {
            self.press(KeyChord::Ctrl(Key::Char('q')))?;
            release_stale_modifiers(self.display)?;
            let mut child = self.child.take().expect("child still present at quit entry");
            match wait_child(&mut child, timeout) {
                Ok(status) => Ok(status),
                Err(error) => {
                    terminate(&mut child);
                    Err(error)
                }
            }
        })();
        self.attach_stderr_context(result, "quit")
    }

    /// Attach the tail of the captured stderr log to an error message so
    /// timeouts and unexpected exits surface the editor's own trail without
    /// forcing callers to dig through artifact directories. No-op when no
    /// stderr log path was configured at spawn time, or when the log is
    /// empty / unreadable.
    fn attach_stderr_context<T>(&self, result: Result<T>, label: &str) -> Result<T> {
        let Err(error) = result else {
            return result;
        };
        let Some(path) = self.stderr_log_path.as_ref() else {
            return Err(error);
        };
        let tail = tail_log(path, STDERR_TAIL_LINES);
        if tail.is_empty() {
            return Err(error);
        }
        Err(format!(
            "{error}\n--- editor stderr tail ({label}, last {} non-empty lines from {}) ---\n{tail}",
            STDERR_TAIL_LINES,
            path.display(),
        )
        .into())
    }
}

fn dispatchable_single_chord(chord: &KeyChordSingle) -> KeyChordSingle {
    if chord.ctrl && chord.alt && chord.shift && matches!(chord.key, Key::Up | Key::Down) {
        // Many X11 desktops reserve Ctrl+Alt+Arrow globally before the app can
        // observe it. Drive the editor's non-reserved duplicate-line binding
        // for this product shortcut so the real-window tests still exercise
        // production duplicate-line behavior.
        return KeyChordSingle {
            ctrl: true,
            alt: false,
            shift: true,
            platform: false,
            key: Key::Char('d'),
        };
    }
    *chord
}

fn resolve_text_pixels(state: &StateTraceRecord, line: usize, col: usize, label: &str) -> Result<(i32, i32)> {
    state.viewport.text_to_window_local(line, col).ok_or_else(|| {
        io::Error::other(format!(
            concat!(
                "{}: line {} col {} not in painted viewport (rows: {}); ",
                "scroll-into-view first or wait for first paint"
            ),
            label,
            line,
            col,
            state.viewport.rows.len(),
        ))
        .into()
    })
}

fn mouse_click_expects_state_change(
    state: &StateTraceRecord,
    line: usize,
    col: usize,
    mods: ChordMods,
    button: u8,
    click_count: usize,
) -> bool {
    if !mods.is_empty() || button != BUTTON_LEFT || click_count > 1 {
        return true;
    }
    !matches!(
        state.cursors.as_slice(),
        [cursor]
            if cursor.is_collapsed() && cursor.head_line == line && cursor.head_col == col
    )
}

fn multi_click_selection_reached(
    state: &StateTraceRecord,
    clicked_row: Option<(usize, usize)>,
    quad_min_end: Option<usize>,
    click_count: usize,
) -> bool {
    let Some((lo, hi)) = single_selection_range(state) else {
        return false;
    };
    if click_count == 2 {
        return true;
    }
    let Some((row_start, row_end)) = clicked_row else {
        return false;
    };
    if click_count == 3 {
        return lo <= row_start && hi >= row_end;
    }
    let min_end = quad_min_end.unwrap_or(row_end + 1);
    lo <= row_start && hi >= min_end
}

fn single_selection_range(state: &StateTraceRecord) -> Option<(usize, usize)> {
    let [cursor] = state.cursors.as_slice() else {
        return None;
    };
    (!cursor.is_collapsed()).then_some((
        cursor.anchor_char.min(cursor.head_char),
        cursor.anchor_char.max(cursor.head_char),
    ))
}

fn key_token_expects_state_change(token: &KeyToken, state: &StateTraceRecord) -> bool {
    match token {
        // GPUI's X11 text path can commit printable input after a following
        // event, so the harness must not block after each individual
        // character waiting for a repaint that may intentionally be batched.
        KeyToken::Single(chord) if editor_enter_key_changes_state(chord, state) => true,
        KeyToken::Single(chord) if plain_insert_text_key_changes_state(chord, state) => false,
        KeyToken::Single(chord) if vim_key_context_changes_state(chord, state) => true,
        KeyToken::Single(chord) if editor_chord_changes_state(chord, state) => true,
        KeyToken::Single(chord) if page_key_changes_state(chord, state) => true,
        KeyToken::Single(chord) if plain_navigation_key_changes_state(chord, state) => true,
        KeyToken::Single(KeyChordSingle {
            ctrl: true,
            alt: false,
            key: Key::Char('v'),
            ..
        }) => true,
        KeyToken::Single(KeyChordSingle {
            ctrl: true,
            alt: false,
            shift: false,
            key: Key::Home,
            ..
        }) => !matches!(
            state.cursors.as_slice(),
            [cursor] if cursor.is_collapsed() && cursor.head_char == 0
        ),
        KeyToken::Single(KeyChordSingle {
            ctrl: true,
            alt: false,
            shift: false,
            key: Key::End,
            ..
        }) => state
            .viewport
            .first_row_for_line(state.line_count.saturating_sub(1))
            .is_some_and(|row| {
                !matches!(
                    state.cursors.as_slice(),
                    [cursor] if cursor.is_collapsed() && cursor.head_char == row.display_end_char
                )
            }),
        _ => false,
    }
}

fn key_token_has_batched_text_state_change(token: &KeyToken, state: &StateTraceRecord) -> bool {
    matches!(
        token,
        KeyToken::Single(chord)
            if !editor_enter_key_changes_state(chord, state)
                && plain_insert_text_key_changes_state(chord, state)
    )
}

fn page_key_changes_state(chord: &KeyChordSingle, state: &StateTraceRecord) -> bool {
    if chord.ctrl || chord.alt || chord.shift || chord.platform || state.focused_input != "editor" {
        return false;
    }
    match chord.key {
        Key::PageUp => state
            .cursors
            .iter()
            .any(|cursor| cursor.head_line > 0 || cursor.head_col > 0),
        Key::PageDown => state.cursors.iter().any(|cursor| {
            cursor.head_line + 1 < state.line_count
                || state
                    .viewport
                    .first_row_for_line(cursor.head_line)
                    .is_some_and(|row| cursor.head_char < row.display_end_char)
        }),
        _ => false,
    }
}

fn plain_navigation_key_changes_state(chord: &KeyChordSingle, state: &StateTraceRecord) -> bool {
    if chord.ctrl || chord.alt || chord.shift || chord.platform || state.focused_input != "editor" {
        return false;
    }
    match chord.key {
        Key::Home => state.cursors.iter().any(|cursor| {
            state
                .viewport
                .first_row_for_line(cursor.head_line)
                .is_some_and(|row| row.display_end_char > row.line_start_char)
        }),
        Key::Left | Key::Up => state
            .cursors
            .iter()
            .any(|cursor| cursor.head_line > 0 || cursor.head_col > 0),
        Key::End | Key::Right => state.cursors.iter().any(|cursor| {
            state
                .viewport
                .first_row_for_line(cursor.head_line)
                .is_some_and(|row| cursor.head_char < row.display_end_char)
        }),
        Key::Down => state
            .cursors
            .iter()
            .any(|cursor| cursor.head_line + 1 < state.line_count),
        _ => false,
    }
}

fn editor_chord_changes_state(chord: &KeyChordSingle, state: &StateTraceRecord) -> bool {
    if state.focused_input != "editor" {
        return false;
    }

    match chord {
        KeyChordSingle {
            ctrl: true,
            alt: false,
            shift: true,
            key: Key::Char('l'),
            platform: false,
        }
        | KeyChordSingle {
            ctrl: true,
            alt: false,
            shift: false,
            key: Key::Char('d' | 'u' | 'g' | 'y' | 'z'),
            platform: false,
        }
        | KeyChordSingle {
            ctrl: false,
            alt: false,
            shift: false,
            key: Key::Backspace | Key::Delete,
            platform: false,
        }
        | KeyChordSingle {
            ctrl: true,
            alt: false,
            shift: false,
            key: Key::Backspace | Key::Delete,
            platform: false,
        }
        | KeyChordSingle {
            ctrl: true,
            alt: true,
            shift: true,
            key: Key::Up | Key::Down,
            platform: false,
        }
        | KeyChordSingle {
            ctrl: false,
            alt: true,
            shift: true,
            key: Key::Char('i'),
            platform: false,
        }
        | KeyChordSingle {
            ctrl: true,
            alt: false,
            shift: true,
            key: Key::Left | Key::Right | Key::Home | Key::End,
            platform: false,
        }
        | KeyChordSingle {
            ctrl: false,
            alt: false,
            shift: true,
            platform: true,
            key: Key::Left | Key::Right | Key::Home | Key::End,
        }
        | KeyChordSingle {
            ctrl: false,
            alt: false,
            shift: true,
            platform: false,
            key: Key::Left | Key::Right | Key::Home | Key::End | Key::Tab,
        }
        | KeyChordSingle {
            ctrl: false,
            alt: true,
            shift: true,
            key: Key::Left | Key::Right,
            platform: false,
        } => true,
        KeyChordSingle {
            ctrl: false,
            alt: true,
            shift: true,
            key: Key::Up | Key::Down,
            platform: false,
        }
        | KeyChordSingle {
            ctrl: true,
            alt: true,
            shift: false,
            key: Key::Up | Key::Down,
            platform: false,
        } => adjacent_cursor_chord_changes_state(chord, state),
        _ => false,
    }
}

fn adjacent_cursor_chord_changes_state(chord: &KeyChordSingle, state: &StateTraceRecord) -> bool {
    let direction = match chord.key {
        Key::Up => -1isize,
        Key::Down => 1isize,
        _ => return false,
    };

    state.cursors.iter().any(|cursor| {
        let target_line = if direction.is_negative() {
            cursor.head_line.checked_sub(direction.unsigned_abs())
        } else {
            let line = cursor.head_line + direction as usize;
            (line < state.line_count).then_some(line)
        };
        let Some(target_line) = target_line else {
            return false;
        };
        let target_col = state
            .viewport
            .first_row_for_line(target_line)
            .map(|row| cursor.head_col.min(row.display_end_char - row.line_start_char))
            .unwrap_or(cursor.head_col);
        !state
            .cursors
            .iter()
            .any(|other| other.head_line == target_line && other.head_col == target_col)
    })
}

fn vim_key_context_changes_state(chord: &KeyChordSingle, state: &StateTraceRecord) -> bool {
    if chord.ctrl || chord.alt || chord.platform || state.focused_input != "editor" {
        return false;
    }
    match chord.key {
        Key::Escape => !state.vim_pending.is_empty() || state.vim_mode != "NORMAL",
        Key::Char('/' | '?' | 'g' | 'd' | 'y' | 'c' | 'z') => state.vim_mode == "NORMAL",
        Key::Char('v') => state.vim_mode == "NORMAL",
        _ => false,
    }
}

fn editor_enter_key_changes_state(chord: &KeyChordSingle, state: &StateTraceRecord) -> bool {
    !chord.ctrl
        && !chord.alt
        && !chord.shift
        && !chord.platform
        && matches!(chord.key, Key::Enter)
        && state.focused_input == "editor"
        && state.vim_mode == "INSERT"
}

fn plain_insert_text_key_changes_state(chord: &KeyChordSingle, state: &StateTraceRecord) -> bool {
    if chord.ctrl || chord.alt || chord.platform {
        return false;
    }
    if state.focused_input == "editor" && state.vim_mode != "INSERT" {
        return false;
    }
    if state.focused_input == "recent_query" && matches!(chord.key, Key::Enter) {
        return state.recent_panel_selected_path.is_some();
    }
    matches!(chord.key, Key::Char(_) | Key::Space | Key::Tab | Key::Enter)
}

fn state_key_context_changed_after(before: &StateTraceRecord, after: &StateTraceRecord) -> bool {
    after.seq > before.seq
        && (after.revision != before.revision
            || state_cursor_signature(after) != state_cursor_signature(before)
            || after.focused_input != before.focused_input
            || after.input_mode != before.input_mode
            || after.workspace_surface != before.workspace_surface
            || after.close_prompt_file != before.close_prompt_file
            || after.vim_mode != before.vim_mode
            || after.vim_pending != before.vim_pending
            || after.find.visible != before.find.visible
            || after.find.query != before.find.query
            || after.goto_line_input != before.goto_line_input
            || after.recent_panel_open != before.recent_panel_open
            || after.recent_panel_query != before.recent_panel_query
            || after.recent_panel_selected_path != before.recent_panel_selected_path
            || after.recent_panel_empty_message != before.recent_panel_empty_message
            || after.recent_panel_content_search_pending != before.recent_panel_content_search_pending)
}

fn state_cursor_signature(record: &StateTraceRecord) -> Vec<(usize, usize, usize, usize)> {
    record
        .cursors
        .iter()
        .map(|cursor| {
            (
                cursor.anchor_char,
                cursor.head_char,
                cursor.anchor_line,
                cursor.head_line,
            )
        })
        .collect()
}

fn viewport_geometry_is_ready(record: &StateTraceRecord) -> bool {
    !record.viewport.rows.is_empty() && record.viewport.char_width_px > 0.0 && record.viewport.line_height_px > 0.0
}

fn tail_log(path: &Path, max_lines: usize) -> String {
    let Ok(text) = fs::read_to_string(path) else {
        return String::new();
    };
    let lines: Vec<&str> = text.lines().filter(|line| !line.trim().is_empty()).collect();
    let start = lines.len().saturating_sub(max_lines);
    lines[start..].join("\n")
}

fn child_mut(child: &mut Option<Child>) -> Result<&mut Child> {
    child
        .as_mut()
        .ok_or_else(|| io::Error::other("editor child already exited").into())
}

impl Drop for Editor<'_> {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            terminate(&mut child);
        }
    }
}

fn resolve_key(kc: &Keycodes, key: Key) -> Result<(Keycode, bool)> {
    match key {
        Key::Char(ch) => kc
            .lookup_char(ch)
            .ok_or_else(|| io::Error::other(format!("unsupported key char: {ch:?}")).into()),
        Key::Tab => Ok((kc.tab, false)),
        Key::Space => Ok((kc.space, false)),
        Key::Enter => Ok((kc.enter, false)),
        Key::Escape => Ok((kc.escape, false)),
        Key::Backspace => Ok((kc.backspace, false)),
        Key::Delete => Ok((kc.delete, false)),
        Key::Insert => Ok((kc.insert, false)),
        Key::F2 => Ok((kc.f2, false)),
        Key::Home => Ok((kc.home, false)),
        Key::End => Ok((kc.end, false)),
        Key::Left => Ok((kc.left, false)),
        Key::Right => Ok((kc.right, false)),
        Key::Up => Ok((kc.up, false)),
        Key::Down => Ok((kc.down, false)),
        Key::PageUp => Ok((kc.page_up, false)),
        Key::PageDown => Ok((kc.page_down, false)),
    }
}

/// Modifier set held continuously across a chord-hold span. Used both by
/// the parser (`<C-{k d}>`) and the programmatic [`Editor::with_chord_held`]
/// API. Pub-fields because there is no invariant beyond "at least one of
/// ctrl/alt/shift/platform is set" (enforced at use time).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ChordMods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub platform: bool,
}

impl ChordMods {
    pub const CTRL: Self = Self {
        ctrl: true,
        alt: false,
        shift: false,
        platform: false,
    };
    pub const ALT: Self = Self {
        ctrl: false,
        alt: true,
        shift: false,
        platform: false,
    };
    pub const SHIFT: Self = Self {
        ctrl: false,
        alt: false,
        shift: true,
        platform: false,
    };
    pub const PLATFORM: Self = Self {
        ctrl: false,
        alt: false,
        shift: false,
        platform: true,
    };

    pub fn is_empty(self) -> bool {
        !self.ctrl && !self.alt && !self.shift && !self.platform
    }

    pub fn with_ctrl(mut self) -> Self {
        self.ctrl = true;
        self
    }
    pub fn with_alt(mut self) -> Self {
        self.alt = true;
        self
    }
    pub fn with_shift(mut self) -> Self {
        self.shift = true;
        self
    }
    pub fn with_platform(mut self) -> Self {
        self.platform = true;
        self
    }
}

#[derive(Clone, Debug)]
enum KeyToken {
    Single(KeyChordSingle),
    Held(KeyChordHeld),
}

#[derive(Clone, Copy, Debug)]
struct KeyChordSingle {
    ctrl: bool,
    alt: bool,
    shift: bool,
    platform: bool,
    key: Key,
}

#[derive(Clone, Debug)]
struct KeyChordHeld {
    /// Outer modifier set, held continuously across `inner`. Always
    /// non-empty (parser rejects empty held sets).
    mods: ChordMods,
    /// Inner keys, tapped in order while `mods` is held.
    inner: Vec<HeldInnerKey>,
}

#[derive(Clone, Copy, Debug)]
struct HeldInnerKey {
    /// Inner-only shift, on top of any shift in the held outer mods.
    /// Lets `<C-{A b}>` mean "Ctrl held, then Shift+A then b."
    shift: bool,
    key: Key,
}

fn parse_keys(input: &str) -> Result<Vec<KeyToken>> {
    let mut tokens = Vec::new();
    let mut chars = input.chars();
    while let Some(ch) = chars.next() {
        if ch == '<' {
            let mut spec = String::new();
            let mut depth = 0usize;
            let mut closed = false;
            for c in chars.by_ref() {
                if c == '{' {
                    depth += 1;
                    spec.push(c);
                } else if c == '}' {
                    depth = depth.saturating_sub(1);
                    spec.push(c);
                } else if c == '>' && depth == 0 {
                    closed = true;
                    break;
                } else {
                    spec.push(c);
                }
            }
            if !closed {
                return Err(io::Error::other(format!("unterminated key escape '<{spec}'")).into());
            }
            tokens.push(parse_escape(&spec)?);
        } else {
            // Literal `\t` / `\n` map to the dedicated key tokens so test
            // fixtures can embed them directly.
            let key = match ch {
                '\t' => Key::Tab,
                '\n' | '\r' => Key::Enter,
                _ => Key::Char(ch),
            };
            tokens.push(KeyToken::Single(KeyChordSingle {
                ctrl: false,
                alt: false,
                shift: false,
                platform: false,
                key,
            }));
        }
    }
    Ok(tokens)
}

fn parse_escape(spec: &str) -> Result<KeyToken> {
    if spec.is_empty() {
        return Err(io::Error::other("empty key escape '<>'").into());
    }
    if let Some(brace_idx) = spec.find('{') {
        return parse_held_escape(spec, brace_idx);
    }
    let single = parse_single_escape(spec)?;
    Ok(KeyToken::Single(single))
}

fn parse_single_escape(spec: &str) -> Result<KeyChordSingle> {
    let lowered = spec.to_ascii_lowercase();
    let mut ctrl = false;
    let mut alt = false;
    let mut shift = false;
    let mut platform = false;
    let mut tail = lowered.as_str();
    loop {
        if let Some(rest) = strip_modifier(tail, &["c-", "ctrl-"]) {
            ctrl = true;
            tail = rest;
        } else if let Some(rest) = strip_modifier(tail, &["a-", "alt-"]) {
            alt = true;
            tail = rest;
        } else if let Some(rest) = strip_modifier(tail, &["s-", "shift-"]) {
            shift = true;
            tail = rest;
        } else if let Some(rest) = strip_modifier(tail, &["cmd-", "super-", "platform-"]) {
            platform = true;
            tail = rest;
        } else {
            break;
        }
    }
    if tail.is_empty() {
        return Err(io::Error::other(format!("missing key in escape '<{spec}>'")).into());
    }
    let key = if tail.chars().count() == 1 {
        Key::Char(tail.chars().next().unwrap())
    } else {
        parse_special_name(tail).ok_or_else(|| io::Error::other(format!("unknown key escape '<{spec}>'")))?
    };
    Ok(KeyChordSingle {
        ctrl,
        alt,
        shift,
        platform,
        key,
    })
}

fn parse_held_escape(spec: &str, brace_idx: usize) -> Result<KeyToken> {
    let prefix = &spec[..brace_idx];
    let body = &spec[brace_idx..];
    if !body.ends_with('}') {
        return Err(io::Error::other(format!("chord-hold escape must end with '}}': '<{spec}>'")).into());
    }
    let inner_raw = &body[1..body.len() - 1];

    let lowered_prefix = prefix.to_ascii_lowercase();
    let mut mods = ChordMods::default();
    let mut tail = lowered_prefix.as_str();
    loop {
        if let Some(rest) = strip_modifier(tail, &["c-", "ctrl-"]) {
            mods.ctrl = true;
            tail = rest;
        } else if let Some(rest) = strip_modifier(tail, &["a-", "alt-"]) {
            mods.alt = true;
            tail = rest;
        } else if let Some(rest) = strip_modifier(tail, &["s-", "shift-"]) {
            mods.shift = true;
            tail = rest;
        } else if let Some(rest) = strip_modifier(tail, &["cmd-", "super-", "platform-"]) {
            mods.platform = true;
            tail = rest;
        } else {
            break;
        }
    }
    if !tail.is_empty() {
        return Err(io::Error::other(format!("unexpected text before chord-hold body in '<{spec}>'")).into());
    }
    if mods.is_empty() {
        return Err(io::Error::other(format!("chord-hold requires at least one held modifier: '<{spec}>'")).into());
    }

    let inner = parse_held_inner(inner_raw, spec)?;
    if inner.is_empty() {
        return Err(io::Error::other(format!("chord-hold body cannot be empty: '<{spec}>'")).into());
    }
    Ok(KeyToken::Held(KeyChordHeld { mods, inner }))
}

fn parse_held_inner(inner_raw: &str, spec: &str) -> Result<Vec<HeldInnerKey>> {
    let mut inner = Vec::new();
    for piece in inner_raw.split_whitespace() {
        let parsed = parse_keys(piece)
            .map_err(|err| io::Error::other(format!("invalid inner piece {piece:?} in '<{spec}>': {err}")))?;
        if parsed.len() != 1 {
            return Err(io::Error::other(format!(
                "each inner piece in chord-hold must be exactly one key (got {} from {piece:?}): '<{spec}>'",
                parsed.len()
            ))
            .into());
        }
        match parsed.into_iter().next().unwrap() {
            KeyToken::Single(s) => {
                if s.ctrl || s.alt {
                    return Err(io::Error::other(format!(
                        concat!(
                            "inner key in chord-hold cannot carry ctrl/alt; held modifiers go on the outer ",
                            "prefix: '<{}>'"
                        ),
                        spec
                    ))
                    .into());
                }
                inner.push(HeldInnerKey {
                    shift: s.shift,
                    key: s.key,
                });
            }
            KeyToken::Held(_) => {
                return Err(io::Error::other(format!("nested chord-hold not supported: '<{spec}>'")).into());
            }
        }
    }
    Ok(inner)
}

fn strip_modifier<'a>(tail: &'a str, prefixes: &[&str]) -> Option<&'a str> {
    prefixes.iter().find_map(|prefix| tail.strip_prefix(prefix))
}

fn parse_special_name(name: &str) -> Option<Key> {
    Some(match name {
        "enter" | "cr" | "return" => Key::Enter,
        "esc" | "escape" => Key::Escape,
        "tab" => Key::Tab,
        "space" => Key::Space,
        "bs" | "backspace" => Key::Backspace,
        "del" | "delete" => Key::Delete,
        "ins" | "insert" => Key::Insert,
        "f2" => Key::F2,
        "home" => Key::Home,
        "end" => Key::End,
        "left" => Key::Left,
        "right" => Key::Right,
        "up" => Key::Up,
        "down" => Key::Down,
        "pageup" | "pgup" | "prior" => Key::PageUp,
        "pagedown" | "pgdn" | "next" => Key::PageDown,
        "lt" => Key::Char('<'),
        _ => return None,
    })
}

fn wait_file_text_impl(
    conn: &RustConnection,
    damage_id: damage::Damage,
    window_id: xproto::Window,
    child: &mut Child,
    path: &Path,
    expected: &str,
    opts: FileWaitOpts,
) -> Result<FileWaitOutcome> {
    let deadline = Instant::now() + opts.timeout;
    let mut last_text = read_optional_text(path)?;
    let mut last_change = Instant::now();
    let mut damage_events = 0u64;

    loop {
        if let Some(status) = child.try_wait()? {
            return Err(io::Error::other(format!("editor exited while waiting for file contents: {status}")).into());
        }

        while let Some(event) = conn.poll_for_event()? {
            if let Event::DamageNotify(notify) = event {
                if notify.damage == damage_id && notify.drawable == window_id {
                    damage_events += 1;
                    conn.damage_subtract(damage_id, NONE, NONE)?;
                }
            }
        }
        conn.flush()?;

        let current = read_optional_text(path)?;
        if current != last_text {
            last_text = current;
            last_change = Instant::now();
        }

        if last_text.as_deref() == Some(expected) && last_change.elapsed() >= opts.stable_for {
            let text = last_text.as_ref().expect("matched expected text above");
            return Ok(FileWaitOutcome {
                stats: FileStats {
                    bytes: text.len() as u64,
                    lines: text.lines().count(),
                },
                damage_events,
            });
        }

        if Instant::now() >= deadline {
            let observed = match &last_text {
                Some(text) => format!("{} bytes, preview {:?}", text.len(), preview_text(text, 120)),
                None => "missing file".to_string(),
            };
            return Err(io::Error::other(format!(
                "timed out waiting for {} to equal {} bytes; last observed {observed}",
                path.display(),
                expected.len(),
            ))
            .into());
        }

        thread::sleep(Duration::from_millis(20));
    }
}

fn read_optional_text(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn preview_text(text: &str, max_chars: usize) -> String {
    let mut preview = String::new();
    for (index, ch) in text.chars().enumerate() {
        if index == max_chars {
            preview.push_str("...");
            break;
        }
        preview.push(ch);
    }
    preview
}

fn wait_file_stable_impl(child: &mut Child, path: &Path, stable_for: Duration, timeout: Duration) -> Result<FileStats> {
    let deadline = Instant::now() + timeout;
    let mut last_text = read_optional_text(path)?;
    let mut last_change = Instant::now();

    loop {
        if let Some(status) = child.try_wait()? {
            return Err(io::Error::other(format!("editor exited while waiting for file: {status}")).into());
        }
        let current = read_optional_text(path)?;
        if current != last_text {
            last_text = current;
            last_change = Instant::now();
        }
        if let Some(text) = &last_text {
            if !text.is_empty() && last_change.elapsed() >= stable_for {
                return Ok(FileStats {
                    bytes: text.len() as u64,
                    lines: text.lines().count(),
                });
            }
        }
        if Instant::now() >= deadline {
            let observed = match &last_text {
                Some(text) => format!("{} bytes, preview {:?}", text.len(), preview_text(text, 120)),
                None => "missing file".to_string(),
            };
            return Err(io::Error::other(format!(
                "timed out waiting for {} to stabilize; last observed {observed}",
                path.display(),
            ))
            .into());
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn wait_child(child: &mut Child, timeout: Duration) -> Result<ExitStatus> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            return Err(io::Error::other("timed out waiting for editor to exit").into());
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn terminate(child: &mut Child) {
    if matches!(child.try_wait(), Ok(Some(_))) {
        return;
    }
    if let Ok(pid) = i32::try_from(child.id()) {
        // SAFETY: `pid` came from a Child we still own; the kernel handles
        // the case where the process has already exited (returns ESRCH).
        unsafe {
            libc::kill(pid, libc::SIGTERM);
        }
    }
    let deadline = Instant::now() + TERMINATE_GRACE;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => {}
            Err(error) => {
                eprintln!("lst-x11-harness: error checking child status: {error}");
                break;
            }
        }
        if Instant::now() >= deadline {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    if let Err(error) = child.kill() {
        if !matches!(child.try_wait(), Ok(Some(_))) {
            eprintln!("lst-x11-harness: error killing child: {error}");
        }
    }
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn single(ctrl: bool, alt: bool, shift: bool, key: Key) -> KeyToken {
        single_with_platform(ctrl, alt, shift, false, key)
    }

    fn single_with_platform(ctrl: bool, alt: bool, shift: bool, platform: bool, key: Key) -> KeyToken {
        KeyToken::Single(KeyChordSingle {
            ctrl,
            alt,
            shift,
            platform,
            key,
        })
    }

    fn assert_keys(input: &str, expected: &[KeyToken]) {
        let parsed = parse_keys(input).unwrap();
        assert_eq!(parsed.len(), expected.len(), "{input:?} → {parsed:?}");
        for (got, want) in parsed.iter().zip(expected) {
            match (got, want) {
                (KeyToken::Single(a), KeyToken::Single(b)) => {
                    assert_eq!(a.ctrl, b.ctrl, "{input:?}");
                    assert_eq!(a.alt, b.alt, "{input:?}");
                    assert_eq!(a.shift, b.shift, "{input:?}");
                    assert_eq!(a.platform, b.platform, "{input:?}");
                    assert!(
                        matches!((a.key, b.key), (Key::Char(x), Key::Char(y)) if x == y)
                            || std::mem::discriminant(&a.key) == std::mem::discriminant(&b.key),
                        "{input:?}: {got:?} vs {want:?}",
                    );
                }
                (KeyToken::Held(a), KeyToken::Held(b)) => {
                    assert_eq!(a.mods, b.mods, "{input:?}");
                    assert_eq!(a.inner.len(), b.inner.len(), "{input:?}");
                    for (ai, bi) in a.inner.iter().zip(b.inner.iter()) {
                        assert_eq!(ai.shift, bi.shift, "{input:?}");
                        assert!(
                            matches!((ai.key, bi.key), (Key::Char(x), Key::Char(y)) if x == y)
                                || std::mem::discriminant(&ai.key) == std::mem::discriminant(&bi.key),
                            "{input:?}: inner {ai:?} vs {bi:?}",
                        );
                    }
                }
                _ => panic!("token kind mismatch: {input:?}: {got:?} vs {want:?}"),
            }
        }
    }

    fn held(mods: ChordMods, inner: Vec<HeldInnerKey>) -> KeyToken {
        KeyToken::Held(KeyChordHeld { mods, inner })
    }

    fn inner(shift: bool, key: Key) -> HeldInnerKey {
        HeldInnerKey { shift, key }
    }

    #[test]
    fn parses_vim_sequence_from_user_request() {
        assert_keys(
            "A<enter>B<enter>C<enter><esc>ggdd",
            &[
                single(false, false, false, Key::Char('A')),
                single(false, false, false, Key::Enter),
                single(false, false, false, Key::Char('B')),
                single(false, false, false, Key::Enter),
                single(false, false, false, Key::Char('C')),
                single(false, false, false, Key::Enter),
                single(false, false, false, Key::Escape),
                single(false, false, false, Key::Char('g')),
                single(false, false, false, Key::Char('g')),
                single(false, false, false, Key::Char('d')),
                single(false, false, false, Key::Char('d')),
            ],
        );
    }

    #[test]
    fn ctrl_and_shift_chord_escapes() {
        assert_keys(
            "<C-s><S-tab><C-S-l><ctrl-shift-a>",
            &[
                single(true, false, false, Key::Char('s')),
                single(false, false, true, Key::Tab),
                single(true, false, true, Key::Char('l')),
                single(true, false, true, Key::Char('a')),
            ],
        );
    }

    #[test]
    fn alt_and_navigation_chord_escapes() {
        assert_keys(
            "<C-A-down><alt-up><S-left><delete><home><end>",
            &[
                single(true, true, false, Key::Down),
                single(false, true, false, Key::Up),
                single(false, false, true, Key::Left),
                single(false, false, false, Key::Delete),
                single(false, false, false, Key::Home),
                single(false, false, false, Key::End),
            ],
        );
    }

    #[test]
    fn platform_chord_escapes() {
        assert_keys(
            "<cmd-S-left><super-right><platform-home>",
            &[
                single_with_platform(false, false, true, true, Key::Left),
                single_with_platform(false, false, false, true, Key::Right),
                single_with_platform(false, false, false, true, Key::Home),
            ],
        );
    }

    #[test]
    fn escape_names_are_case_insensitive() {
        assert_keys(
            "<Esc><ESCAPE><Cr><RETURN>",
            &[
                single(false, false, false, Key::Escape),
                single(false, false, false, Key::Escape),
                single(false, false, false, Key::Enter),
                single(false, false, false, Key::Enter),
            ],
        );
    }

    #[test]
    fn lt_escape_emits_literal_less_than() {
        assert_keys(
            "<lt>3",
            &[
                single(false, false, false, Key::Char('<')),
                single(false, false, false, Key::Char('3')),
            ],
        );
    }

    #[test]
    fn unterminated_escape_is_an_error() {
        assert!(parse_keys("<enter").is_err());
    }

    #[test]
    fn unknown_special_key_is_an_error() {
        assert!(parse_keys("<bogus>").is_err());
    }

    #[test]
    fn page_up_and_page_down_have_canonical_and_short_names() {
        assert_keys(
            "<pageup><pgup><pagedown><pgdn>",
            &[
                single(false, false, false, Key::PageUp),
                single(false, false, false, Key::PageUp),
                single(false, false, false, Key::PageDown),
                single(false, false, false, Key::PageDown),
            ],
        );
    }

    #[test]
    fn function_key_names_parse_with_modifiers() {
        assert_keys("<C-f2>", &[single(true, false, false, Key::F2)]);
    }

    #[test]
    fn chord_hold_short_form_with_two_inner_keys() {
        assert_keys(
            "<C-{k d}>",
            &[held(
                ChordMods::CTRL,
                vec![inner(false, Key::Char('k')), inner(false, Key::Char('d'))],
            )],
        );
    }

    #[test]
    fn chord_hold_verbose_form_matches_short_form() {
        assert_keys(
            "<ctrl-{k d}>",
            &[held(
                ChordMods::CTRL,
                vec![inner(false, Key::Char('k')), inner(false, Key::Char('d'))],
            )],
        );
    }

    #[test]
    fn chord_hold_with_specials_inside_braces() {
        assert_keys(
            "<C-{<up> <down>}>",
            &[held(
                ChordMods::CTRL,
                vec![inner(false, Key::Up), inner(false, Key::Down)],
            )],
        );
    }

    #[test]
    fn chord_hold_inner_uppercase_picks_up_inner_shift_via_resolve() {
        // Inside a brace group, bare uppercase letters survive case-preserving
        // because `parse_keys` recurses on each whitespace piece. The shift
        // flag stays false at parse time; it's resolved later via `lookup_char`.
        assert_keys(
            "<C-{A b}>",
            &[held(
                ChordMods::CTRL,
                vec![inner(false, Key::Char('A')), inner(false, Key::Char('b'))],
            )],
        );
    }

    #[test]
    fn chord_hold_inner_explicit_shift_is_allowed() {
        assert_keys(
            "<C-{<S-tab> b}>",
            &[held(
                ChordMods::CTRL,
                vec![inner(true, Key::Tab), inner(false, Key::Char('b'))],
            )],
        );
    }

    #[test]
    fn chord_hold_combined_with_singles() {
        assert_keys(
            "a<C-{k d}>z",
            &[
                single(false, false, false, Key::Char('a')),
                held(
                    ChordMods::CTRL,
                    vec![inner(false, Key::Char('k')), inner(false, Key::Char('d'))],
                ),
                single(false, false, false, Key::Char('z')),
            ],
        );
    }

    #[test]
    fn chord_hold_requires_outer_modifier() {
        assert!(parse_keys("<{k d}>").is_err());
    }

    #[test]
    fn chord_hold_inner_cannot_carry_ctrl() {
        assert!(parse_keys("<C-{<C-k> d}>").is_err());
    }

    #[test]
    fn chord_hold_inner_cannot_carry_alt() {
        assert!(parse_keys("<C-{<A-k> d}>").is_err());
    }

    #[test]
    fn chord_hold_rejects_nested_braces() {
        assert!(parse_keys("<C-{<A-{x y}> d}>").is_err());
    }

    #[test]
    fn chord_hold_empty_body_is_an_error() {
        assert!(parse_keys("<C-{}>").is_err());
    }

    #[test]
    fn chord_hold_unterminated_is_an_error() {
        assert!(parse_keys("<C-{k d>").is_err());
    }

    #[test]
    fn chord_hold_multi_char_piece_is_an_error() {
        // "kd" inside the braces would parse as two tokens; we require one
        // key per whitespace-separated piece for unambiguity.
        assert!(parse_keys("<C-{kd}>").is_err());
    }
}
