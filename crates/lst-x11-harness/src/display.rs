use std::collections::HashMap;
use std::env;
use std::fs::{self, File, OpenOptions};
use std::os::fd::AsRawFd;
use std::process::{Command, Stdio};

use x11rb::connection::Connection as _;
use x11rb::protocol::damage::ConnectionExt as _;
use x11rb::protocol::xkb::ConnectionExt as _;
use x11rb::protocol::xproto::Window;
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;

use crate::clipboard;
use crate::x11::{Atoms, Keycodes};
use crate::Result;

const HARNESS_LAYOUT_ENV: &str = "LST_X11_HARNESS_LAYOUT";
const HARNESS_VARIANT_ENV: &str = "LST_X11_HARNESS_VARIANT";
const HARNESS_OPTIONS_ENV: &str = "LST_X11_HARNESS_OPTIONS";

#[derive(Clone, Debug)]
pub(crate) struct SessionEnv {
    pub(crate) display: String,
    pub(crate) xauthority: Option<String>,
    pub(crate) dbus_session_bus_address: Option<String>,
}

/// Connection to an X server plus the per-session resources the harness
/// needs to drive a single editor at a time.
pub struct Display {
    pub(crate) conn: RustConnection,
    pub(crate) root: Window,
    pub(crate) atoms: Atoms,
    pub(crate) keycodes: Keycodes,
    pub(crate) session_env: SessionEnv,
    // `_layout` must drop before `_lock` so the keymap restore happens while
    // the cross-process lock is still held. Rust drops fields in declaration
    // order, so keep this ordering.
    _layout: LayoutGuard,
    _lock: SessionLock,
}

impl Display {
    /// Resolve the X session, connect, query DAMAGE/XTEST/XKB extensions,
    /// and verify `xclip` is on `PATH`. Pins the keyboard layout for the
    /// harness's lifetime so character → keycode lookups are deterministic
    /// across developer machines, and restores the prior layout on drop.
    /// Defaults to US; layout regression tests can override this with
    /// `LST_X11_HARNESS_LAYOUT`.
    pub fn from_env() -> Result<Self> {
        let lock = SessionLock::acquire()?;
        clipboard::require_xclip()?;
        let session_env = resolve_session_env()?;
        apply_session_env(&session_env);
        let layout = LayoutGuard::pin_target(&session_env)?;

        let (conn, screen_num) = x11rb::connect(Some(session_env.display.as_str()))?;
        conn.damage_query_version(1, 1)?.reply()?;
        conn.xtest_get_version(2, 2)?.reply()?;
        conn.xkb_use_extension(1, 0)?.reply()?;
        let root = conn.setup().roots[screen_num].root;
        let atoms = Atoms::intern(&conn)?;
        let keycodes = Keycodes::resolve(&conn)?;

        Ok(Self {
            conn,
            root,
            atoms,
            keycodes,
            session_env,
            _layout: layout,
            _lock: lock,
        })
    }
}

struct SessionLock {
    file: File,
}

impl SessionLock {
    fn acquire() -> Result<Self> {
        let path = env::temp_dir().join("lst-x11-harness.lock");
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)?;
        // SAFETY: `file` is an open lock-file descriptor we keep alive inside
        // `SessionLock`; `flock` only mutates kernel lock state for that fd.
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
        if rc == 0 {
            Ok(Self { file })
        } else {
            Err(std::io::Error::last_os_error().into())
        }
    }
}

impl Drop for SessionLock {
    fn drop(&mut self) {
        // SAFETY: the descriptor is still owned by `self.file`; unlocking is
        // best-effort because the kernel will also release it on close.
        let _ = unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) };
    }
}

fn resolve_session_env() -> Result<SessionEnv> {
    if let Ok(display) = env::var("DISPLAY") {
        return Ok(SessionEnv {
            display,
            xauthority: env::var("XAUTHORITY").ok(),
            dbus_session_bus_address: env::var("DBUS_SESSION_BUS_ADDRESS").ok(),
        });
    }

    let proc_dir = fs::read_dir("/proc")?;
    let mut best: Option<(usize, SessionEnv)> = None;

    for entry in proc_dir {
        let Ok(entry) = entry else { continue };
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.chars().all(|ch| ch.is_ascii_digit()) {
            continue;
        }
        let Ok(environ) = fs::read(entry.path().join("environ")) else {
            continue;
        };
        let vars = parse_proc_environ(&environ);
        let Some(display) = vars.get("DISPLAY").cloned() else {
            continue;
        };
        if matches!(vars.get("XDG_SESSION_TYPE"), Some(value) if value != "x11") {
            continue;
        }
        let candidate = SessionEnv {
            display,
            xauthority: vars.get("XAUTHORITY").cloned(),
            dbus_session_bus_address: vars.get("DBUS_SESSION_BUS_ADDRESS").cloned(),
        };
        let score =
            usize::from(candidate.xauthority.is_some()) + usize::from(candidate.dbus_session_bus_address.is_some());
        if best.as_ref().map(|(s, _)| score > *s).unwrap_or(true) {
            best = Some((score, candidate));
        }
    }

    best.map(|(_, env)| env).ok_or_else(|| {
        "could not find an X11 desktop session; run from a desktop terminal or set DISPLAY/XAUTHORITY explicitly".into()
    })
}

fn parse_proc_environ(bytes: &[u8]) -> HashMap<String, String> {
    let mut vars = HashMap::new();
    for entry in bytes.split(|byte| *byte == 0) {
        if entry.is_empty() {
            continue;
        }
        let text = String::from_utf8_lossy(entry);
        let Some((key, value)) = text.split_once('=') else {
            continue;
        };
        vars.insert(key.to_string(), value.to_string());
    }
    vars
}

fn apply_session_env(env: &SessionEnv) {
    env::set_var("DISPLAY", &env.display);
    if let Some(xauthority) = &env.xauthority {
        env::set_var("XAUTHORITY", xauthority);
    }
    if let Some(dbus) = &env.dbus_session_bus_address {
        env::set_var("DBUS_SESSION_BUS_ADDRESS", dbus);
    }
}

/// Snapshot of the user's `setxkbmap` state at the moment `Display::from_env`
/// took ownership of the session. `Drop` restores it.
struct LayoutGuard {
    original: LayoutSnapshot,
    session_env: SessionEnv,
    /// `true` when the original layout already matched the harness's required
    /// state, so we never mutated the X server's keymap. In that case Drop
    /// does nothing — restoring would be a no-op subprocess.
    needs_restore: bool,
}

impl LayoutGuard {
    fn pin_target(env: &SessionEnv) -> Result<Self> {
        require_setxkbmap()?;
        let original = LayoutSnapshot::query(env)?;
        let target = requested_layout_snapshot()?;
        let needs_restore = original != target;
        if needs_restore {
            target.apply(env)?;
        }
        Ok(Self {
            original,
            session_env: env.clone(),
            needs_restore,
        })
    }
}

fn requested_layout_snapshot() -> Result<LayoutSnapshot> {
    let layout = env::var(HARNESS_LAYOUT_ENV).unwrap_or_else(|_| "us".to_string());
    let layout = layout.trim();
    if layout.is_empty() {
        return Err(format!("{HARNESS_LAYOUT_ENV} must not be empty").into());
    }
    Ok(LayoutSnapshot {
        layout: layout.to_string(),
        variant: non_empty_env(HARNESS_VARIANT_ENV),
        options: non_empty_env(HARNESS_OPTIONS_ENV),
    })
}

fn non_empty_env(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

impl Drop for LayoutGuard {
    fn drop(&mut self) {
        if !self.needs_restore {
            return;
        }
        if let Err(error) = self.original.apply(&self.session_env) {
            eprintln!(
                "lst-x11-harness: failed to restore keyboard layout (was {:?}): {error}",
                self.original
            );
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LayoutSnapshot {
    layout: String,
    /// `None` when no variant is configured (the typical case for plain `us`).
    variant: Option<String>,
    /// Comma-separated `setxkbmap -option` value, e.g. `grp:alt_shift_toggle`.
    /// `None` when no options are configured.
    options: Option<String>,
}

impl LayoutSnapshot {
    /// Parse the output of `setxkbmap -query`. Format is one `key: value` line
    /// per setting; we only care about layout, variant, and options. Empty
    /// values (e.g. `variant:`) are normalized to `None`.
    fn query(env: &SessionEnv) -> Result<Self> {
        let output = setxkbmap_command(env)
            .arg("-query")
            .stdin(Stdio::null())
            .stderr(Stdio::piped())
            .output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(format!("setxkbmap -query exited with {}: {stderr}", output.status,).into());
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut layout: Option<String> = None;
        let mut variant: Option<String> = None;
        let mut options: Option<String> = None;
        for line in stdout.lines() {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let value = value.trim();
            match key.trim() {
                "layout" if !value.is_empty() => layout = Some(value.to_string()),
                "variant" if !value.is_empty() => variant = Some(value.to_string()),
                "options" if !value.is_empty() => options = Some(value.to_string()),
                _ => {}
            }
        }
        let layout =
            layout.ok_or_else(|| format!("setxkbmap -query did not include a `layout:` line; got: {stdout}",))?;
        Ok(Self {
            layout,
            variant,
            options,
        })
    }

    /// Apply this snapshot via `setxkbmap`. Always passes explicit `-variant ""`
    /// and `-option ""` first so we are guaranteed to clear any state from a
    /// previous invocation, then layers our values on top.
    fn apply(&self, env: &SessionEnv) -> Result<()> {
        let mut cmd = setxkbmap_command(env);
        cmd.arg("-layout")
            .arg(&self.layout)
            .arg("-variant")
            .arg(self.variant.as_deref().unwrap_or(""))
            .arg("-option")
            .arg("");
        if let Some(options) = &self.options {
            cmd.arg("-option").arg(options);
        }
        let status = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .status()?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("setxkbmap exited with {status} while applying {self:?}").into())
        }
    }
}

fn setxkbmap_command(env: &SessionEnv) -> Command {
    let mut cmd = Command::new("setxkbmap");
    cmd.env("DISPLAY", &env.display);
    if let Some(xauth) = &env.xauthority {
        cmd.env("XAUTHORITY", xauth);
    }
    cmd
}

fn require_setxkbmap() -> Result<()> {
    let status = Command::new("sh")
        .arg("-c")
        .arg("command -v setxkbmap >/dev/null 2>&1")
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err("setxkbmap is required on PATH for the X11 harness (used to pin keyboard layout to a known state)".into())
    }
}
