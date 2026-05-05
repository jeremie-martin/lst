use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::Path;
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
use crate::x11::damage as damage_wait;
use crate::x11::input::{
    self, BUTTON_LEFT, BUTTON_MIDDLE, BUTTON_WHEEL_DOWN, BUTTON_WHEEL_UP, POINTER_SETTLE,
};
use crate::x11::keycodes::Keycodes;
use crate::x11::window::{self, WindowInfo};
use crate::Result;

const WINDOW_DISCOVERY_TIMEOUT_DEFAULT: Duration = Duration::from_secs(30);
const TERMINATE_GRACE: Duration = Duration::from_secs(2);
const SEND_KEYS_QUIET: Duration = Duration::from_millis(20);
const SEND_KEYS_TIMEOUT: Duration = Duration::from_secs(2);

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
}

impl<'a> SpawnOpts<'a> {
    pub fn new(binary: &'a Path, title: &'a str) -> Self {
        Self {
            binary,
            args: &[],
            title,
            stderr: Stdio::inherit(),
            stdout: Stdio::null(),
            extra_env: &[],
        }
    }

    pub fn with_args(mut self, args: &'a [&'a OsStr]) -> Self {
        self.args = args;
        self
    }

    pub fn with_extra_env(mut self, extra_env: &'a [(&'a OsStr, &'a OsStr)]) -> Self {
        self.extra_env = extra_env;
        self
    }

    pub fn with_stderr(mut self, stderr: Stdio) -> Self {
        self.stderr = stderr;
        self
    }

    pub fn with_stdout(mut self, stdout: Stdio) -> Self {
        self.stdout = stdout;
        self
    }
}

#[derive(Clone, Copy, Debug)]
pub struct FileWaitOpts {
    pub stable_for: Duration,
    pub timeout: Duration,
    /// If `Some`, periodically inject `Ctrl+S` until the file matches.
    /// Bench callers use this; smoke tests usually pass `None`.
    pub save_retry_every: Option<Duration>,
}

impl FileWaitOpts {
    pub fn new(timeout: Duration, stable_for: Duration) -> Self {
        Self {
            stable_for,
            timeout,
            save_retry_every: None,
        }
    }

    pub fn with_save_retry(mut self, every: Duration) -> Self {
        self.save_retry_every = Some(every);
        self
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
    pub save_retries: u64,
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
}

impl Display {
    /// Spawn the editor binary, find its window by `_NET_WM_PID` + title,
    /// attach DAMAGE, and return a handle. Errors during discovery or
    /// damage attachment terminate the spawned child before returning.
    pub fn spawn_editor<'a>(&'a mut self, opts: SpawnOpts<'_>) -> Result<Editor<'a>> {
        let title = opts.title;
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
        let damage = match damage::DamageWrapper::create(
            &display.conn,
            info.id,
            damage::ReportLevel::NON_EMPTY,
        ) {
            Ok(damage) => damage,
            Err(error) => {
                terminate(&mut child);
                return Err(error.into());
            }
        };
        display.conn.flush()?;

        Ok(Editor {
            display,
            child: Some(child),
            window: info,
            damage,
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
    for (key, value) in opts.extra_env {
        command.env(key, value);
    }
    for arg in opts.args {
        command.arg(arg);
    }
    command
}

impl<'a> Editor<'a> {
    pub fn child_pid(&self) -> u32 {
        self.child.as_ref().map(Child::id).unwrap_or(0)
    }

    pub fn window_id(&self) -> xproto::Window {
        self.window.id
    }

    /// Returns `(width, height, root_x, root_y)` in pixels.
    pub fn window_geometry(&self) -> (u16, u16, i16, i16) {
        (
            self.window.width,
            self.window.height,
            self.window.root_x,
            self.window.root_y,
        )
    }

    pub fn is_viewable(&self) -> Result<bool> {
        window::is_viewable(&self.display.conn, self.window.id)
    }

    pub fn focus_for_keyboard(&mut self) -> Result<()> {
        self.display.conn.set_input_focus(
            xproto::InputFocus::PARENT,
            self.window.id,
            x11rb::CURRENT_TIME,
        )?;
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
        input::move_pointer_to_window_point(
            &self.display.conn,
            self.display.root,
            &self.window,
            local_x,
            local_y,
        )?;
        thread::sleep(POINTER_SETTLE);
        input::click_button(&self.display.conn, self.display.root, BUTTON_LEFT)
    }

    pub fn middle_click_at(&mut self, local_x: i32, local_y: i32) -> Result<()> {
        input::move_pointer_to_window_point(
            &self.display.conn,
            self.display.root,
            &self.window,
            local_x,
            local_y,
        )?;
        thread::sleep(POINTER_SETTLE);
        input::click_button(&self.display.conn, self.display.root, BUTTON_MIDDLE)
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
        input::chord(
            &self.display.conn,
            self.display.root,
            &self.display.keycodes,
            code,
            ctrl,
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
            input::chord(
                &self.display.conn,
                self.display.root,
                &self.display.keycodes,
                code,
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
    ///   `<tab>`, `<space>`, `<bs>` / `<backspace>`, `<lt>` (literal `<`).
    /// - Modifier chords: `<C-x>` for Ctrl+x, `<S-x>` for Shift+x,
    ///   `<C-S-x>` for Ctrl+Shift+x. The verbose forms `<ctrl-x>` /
    ///   `<shift-x>` are accepted too. Inside a chord the key may be a
    ///   single character or a special name, e.g. `<C-enter>`.
    ///
    /// Names are case-insensitive: `<Esc>`, `<ESC>`, and `<esc>` all match.
    /// Uppercase letters outside escapes auto-shift, so `A` and `<S-a>` are
    /// equivalent.
    ///
    /// After each key, waits for a brief damage-quiet period before sending
    /// the next one. That mirrors human typing: each keystroke drives a
    /// paint, and the next stroke only arrives after the editor has
    /// observed and rendered the previous one. Without this, GPUI's
    /// frame loop can batch several synthesized keystrokes into a single
    /// frame and only the cursor's first move is observed.
    pub fn send_keys(&mut self, sequence: &str) -> Result<()> {
        let tokens = parse_keys(sequence)?;
        for token in tokens {
            let (code, base_shift) = resolve_key(&self.display.keycodes, token.key)?;
            input::chord(
                &self.display.conn,
                self.display.root,
                &self.display.keycodes,
                code,
                token.ctrl,
                base_shift || token.shift,
            )?;
            // Settle: wait until the editor has painted in response. Short
            // quiet window because we just want one frame of evidence; not
            // a fully-idle UI. Uses a generous timeout so a slow paint
            // doesn't fail an otherwise good test.
            let conn = &self.display.conn;
            let damage_id = self.damage.damage();
            let window_id = self.window.id;
            let child = child_mut(&mut self.child)?;
            damage_wait::wait_quiet(
                conn,
                damage_id,
                window_id,
                child,
                SEND_KEYS_QUIET,
                SEND_KEYS_TIMEOUT,
            )?;
        }
        Ok(())
    }

    pub fn wait_quiet(&mut self, quiet: Duration, timeout: Duration) -> Result<u64> {
        let conn = &self.display.conn;
        let damage_id = self.damage.damage();
        let window_id = self.window.id;
        let child = child_mut(&mut self.child)?;
        damage_wait::wait_quiet(conn, damage_id, window_id, child, quiet, timeout)
    }

    pub fn wait_file_text(
        &mut self,
        path: &Path,
        expected: &str,
        opts: FileWaitOpts,
    ) -> Result<FileWaitOutcome> {
        let display = self.display;
        let damage_id = self.damage.damage();
        let window_id = self.window.id;
        let child = child_mut(&mut self.child)?;
        wait_file_text_impl(
            &display.conn,
            damage_id,
            window_id,
            display.root,
            &display.keycodes,
            child,
            path,
            expected,
            opts,
        )
    }

    pub fn wait_file_stable(
        &mut self,
        path: &Path,
        stable_for: Duration,
        timeout: Duration,
    ) -> Result<FileStats> {
        let child = child_mut(&mut self.child)?;
        wait_file_stable_impl(child, path, stable_for, timeout)
    }

    pub fn wait_for_exit(&mut self, timeout: Duration) -> Result<ExitStatus> {
        let child = child_mut(&mut self.child)?;
        wait_child(child, timeout)
    }

    /// Send `Ctrl+Q` and wait for the editor to exit. Consumes `self`; on
    /// success the child has already been reaped, and `Drop` is a no-op.
    /// On timeout the child is force-terminated before the error is
    /// returned, so callers cannot leak the editor process.
    pub fn quit(mut self, timeout: Duration) -> Result<ExitStatus> {
        self.press(KeyChord::Ctrl(Key::Char('q')))?;
        let mut child = self
            .child
            .take()
            .expect("child still present at quit entry");
        match wait_child(&mut child, timeout) {
            Ok(status) => Ok(status),
            Err(error) => {
                terminate(&mut child);
                Err(error)
            }
        }
    }
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
    }
}

#[derive(Clone, Copy, Debug)]
struct KeyToken {
    ctrl: bool,
    shift: bool,
    key: Key,
}

fn parse_keys(input: &str) -> Result<Vec<KeyToken>> {
    let mut tokens = Vec::new();
    let mut chars = input.chars();
    while let Some(ch) = chars.next() {
        if ch == '<' {
            let mut spec = String::new();
            let mut closed = false;
            for c in chars.by_ref() {
                if c == '>' {
                    closed = true;
                    break;
                }
                spec.push(c);
            }
            if !closed {
                return Err(io::Error::other(format!("unterminated key escape '<{spec}'")).into());
            }
            tokens.push(parse_escape(&spec)?);
        } else {
            tokens.push(KeyToken {
                ctrl: false,
                shift: false,
                key: Key::Char(ch),
            });
        }
    }
    Ok(tokens)
}

fn parse_escape(spec: &str) -> Result<KeyToken> {
    if spec.is_empty() {
        return Err(io::Error::other("empty key escape '<>'").into());
    }
    let lowered = spec.to_ascii_lowercase();
    let mut ctrl = false;
    let mut shift = false;
    let mut tail = lowered.as_str();
    loop {
        if let Some(rest) = strip_modifier(tail, &["c-", "ctrl-"]) {
            ctrl = true;
            tail = rest;
        } else if let Some(rest) = strip_modifier(tail, &["s-", "shift-"]) {
            shift = true;
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
        parse_special_name(tail)
            .ok_or_else(|| io::Error::other(format!("unknown key escape '<{spec}>'")))?
    };
    Ok(KeyToken { ctrl, shift, key })
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
        "lt" => Key::Char('<'),
        _ => return None,
    })
}

#[allow(clippy::too_many_arguments)]
fn wait_file_text_impl(
    conn: &RustConnection,
    damage_id: damage::Damage,
    window_id: xproto::Window,
    root: xproto::Window,
    kc: &Keycodes,
    child: &mut Child,
    path: &Path,
    expected: &str,
    opts: FileWaitOpts,
) -> Result<FileWaitOutcome> {
    let deadline = Instant::now() + opts.timeout;
    let mut last_text = fs::read_to_string(path).unwrap_or_default();
    let mut last_change = Instant::now();
    let mut last_save: Option<Instant> = None;
    let mut damage_events = 0u64;
    let mut save_retries = 0u64;

    loop {
        if let Some(status) = child.try_wait()? {
            return Err(io::Error::other(format!(
                "editor exited while waiting for file contents: {status}"
            ))
            .into());
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

        let current = fs::read_to_string(path).unwrap_or_default();
        if current != last_text {
            last_text = current;
            last_change = Instant::now();
        }

        if last_text == expected && last_change.elapsed() >= opts.stable_for {
            return Ok(FileWaitOutcome {
                stats: FileStats {
                    bytes: last_text.len() as u64,
                    lines: last_text.lines().count(),
                },
                damage_events,
                save_retries,
            });
        }

        if let Some(retry_every) = opts.save_retry_every {
            let should_retry = match last_save {
                Some(when) => when.elapsed() >= retry_every,
                None => true,
            };
            if should_retry {
                let (s_code, _) = kc
                    .lookup_char('s')
                    .ok_or_else(|| io::Error::other("missing keycode for 's' (save retry)"))?;
                input::chord(conn, root, kc, s_code, true, false)?;
                last_save = Some(Instant::now());
                save_retries += 1;
            }
        }

        if Instant::now() >= deadline {
            return Err(io::Error::other(format!(
                "timed out waiting for {} to reach {} bytes; last observed {} bytes",
                path.display(),
                expected.len(),
                last_text.len()
            ))
            .into());
        }

        thread::sleep(Duration::from_millis(20));
    }
}

fn wait_file_stable_impl(
    child: &mut Child,
    path: &Path,
    stable_for: Duration,
    timeout: Duration,
) -> Result<FileStats> {
    let deadline = Instant::now() + timeout;
    let mut last_text = fs::read_to_string(path).unwrap_or_default();
    let mut last_change = Instant::now();

    loop {
        if let Some(status) = child.try_wait()? {
            return Err(io::Error::other(format!(
                "editor exited while waiting for file: {status}"
            ))
            .into());
        }
        let current = fs::read_to_string(path).unwrap_or_default();
        if current != last_text {
            last_text = current;
            last_change = Instant::now();
        }
        if !last_text.is_empty() && last_change.elapsed() >= stable_for {
            return Ok(FileStats {
                bytes: last_text.len() as u64,
                lines: last_text.lines().count(),
            });
        }
        if Instant::now() >= deadline {
            return Err(io::Error::other(format!(
                "timed out waiting for {} to stabilize",
                path.display()
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

    fn token(ctrl: bool, shift: bool, key: Key) -> KeyToken {
        KeyToken { ctrl, shift, key }
    }

    fn assert_keys(input: &str, expected: &[KeyToken]) {
        let parsed = parse_keys(input).unwrap();
        assert_eq!(parsed.len(), expected.len(), "{input:?} → {parsed:?}");
        for (got, want) in parsed.iter().zip(expected) {
            assert_eq!(got.ctrl, want.ctrl, "{input:?}");
            assert_eq!(got.shift, want.shift, "{input:?}");
            assert!(
                matches!((got.key, want.key),
                    (Key::Char(a), Key::Char(b)) if a == b)
                    || std::mem::discriminant(&got.key) == std::mem::discriminant(&want.key),
                "{input:?}: {got:?} vs {want:?}",
            );
        }
    }

    #[test]
    fn parses_vim_sequence_from_user_request() {
        assert_keys(
            "A<enter>B<enter>C<enter><esc>ggdd",
            &[
                token(false, false, Key::Char('A')),
                token(false, false, Key::Enter),
                token(false, false, Key::Char('B')),
                token(false, false, Key::Enter),
                token(false, false, Key::Char('C')),
                token(false, false, Key::Enter),
                token(false, false, Key::Escape),
                token(false, false, Key::Char('g')),
                token(false, false, Key::Char('g')),
                token(false, false, Key::Char('d')),
                token(false, false, Key::Char('d')),
            ],
        );
    }

    #[test]
    fn ctrl_and_shift_chord_escapes() {
        assert_keys(
            "<C-s><S-tab><C-S-l><ctrl-shift-a>",
            &[
                token(true, false, Key::Char('s')),
                token(false, true, Key::Tab),
                token(true, true, Key::Char('l')),
                token(true, true, Key::Char('a')),
            ],
        );
    }

    #[test]
    fn escape_names_are_case_insensitive() {
        assert_keys(
            "<Esc><ESCAPE><Cr><RETURN>",
            &[
                token(false, false, Key::Escape),
                token(false, false, Key::Escape),
                token(false, false, Key::Enter),
                token(false, false, Key::Enter),
            ],
        );
    }

    #[test]
    fn lt_escape_emits_literal_less_than() {
        assert_keys(
            "<lt>3",
            &[
                token(false, false, Key::Char('<')),
                token(false, false, Key::Char('3')),
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
}
