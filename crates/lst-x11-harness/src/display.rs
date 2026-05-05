use std::collections::HashMap;
use std::env;
use std::fs::{self, File, OpenOptions};
use std::os::fd::AsRawFd;

use x11rb::connection::Connection as _;
use x11rb::protocol::damage::ConnectionExt as _;
use x11rb::protocol::xkb::ConnectionExt as _;
use x11rb::protocol::xproto::Window;
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;

use crate::clipboard;
use crate::x11::{Atoms, Keycodes};
use crate::Result;

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
    _lock: SessionLock,
}

impl Display {
    /// Resolve the X session, connect, query DAMAGE/XTEST/XKB extensions,
    /// and verify `xclip` is on `PATH`.
    pub fn from_env() -> Result<Self> {
        let lock = SessionLock::acquire()?;
        clipboard::require_xclip()?;
        let session_env = resolve_session_env()?;
        apply_session_env(&session_env);

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
        let score = usize::from(candidate.xauthority.is_some())
            + usize::from(candidate.dbus_session_bus_address.is_some());
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
