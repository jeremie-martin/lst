use std::io;
use std::process::Child;
use std::thread;
use std::time::{Duration, Instant};

use x11rb::errors::ReplyError;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, MapState, Window};
use x11rb::protocol::ErrorKind;
use x11rb::rust_connection::RustConnection;

use crate::x11::atoms::Atoms;
use crate::Result;

pub(crate) struct WindowInfo {
    pub(crate) id: Window,
    pub(crate) width: u16,
    pub(crate) height: u16,
}

pub(crate) fn find(
    conn: &RustConnection,
    root: Window,
    atoms: &Atoms,
    pid: u32,
    title: &str,
    child: &mut Child,
    timeout: Duration,
) -> Result<WindowInfo> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return Err(io::Error::other(format!(
                "editor exited before its window appeared: {status}"
            ))
            .into());
        }
        if let Some(info) = find_recursive(conn, root, atoms, pid, title)? {
            return Ok(info);
        }
        if Instant::now() >= deadline {
            return Err(io::Error::other("timed out waiting for editor window").into());
        }
        thread::sleep(Duration::from_millis(25));
    }
}

pub(crate) fn is_viewable(conn: &RustConnection, window: Window) -> Result<bool> {
    let attrs = match conn.get_window_attributes(window)?.reply() {
        Ok(attrs) => attrs,
        Err(error) if is_stale_window_error(&error) => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    Ok(attrs.map_state == MapState::VIEWABLE)
}

fn find_recursive(
    conn: &RustConnection,
    window: Window,
    atoms: &Atoms,
    pid: u32,
    title: &str,
) -> Result<Option<WindowInfo>> {
    if matches(conn, window, atoms, pid, title)? {
        let attrs = match conn.get_window_attributes(window)?.reply() {
            Ok(attrs) => attrs,
            Err(error) if is_stale_window_error(&error) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        if attrs.map_state == MapState::VIEWABLE {
            let geometry = match conn.get_geometry(window)?.reply() {
                Ok(geometry) => geometry,
                Err(error) if is_stale_window_error(&error) => return Ok(None),
                Err(error) => return Err(error.into()),
            };
            return Ok(Some(WindowInfo {
                id: window,
                width: geometry.width,
                height: geometry.height,
            }));
        }
    }

    let tree = match conn.query_tree(window)?.reply() {
        Ok(tree) => tree,
        Err(error) if is_stale_window_error(&error) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    for child in tree.children {
        if let Some(info) = find_recursive(conn, child, atoms, pid, title)? {
            return Ok(Some(info));
        }
    }
    Ok(None)
}

fn matches(
    conn: &RustConnection,
    window: Window,
    atoms: &Atoms,
    pid: u32,
    title: &str,
) -> Result<bool> {
    let Some(window_pid) = read_pid(conn, window, atoms)? else {
        return Ok(false);
    };
    if window_pid != pid {
        return Ok(false);
    }
    Ok(read_title(conn, window, atoms)?.as_deref() == Some(title))
}

fn read_pid(conn: &RustConnection, window: Window, atoms: &Atoms) -> Result<Option<u32>> {
    let reply = match conn
        .get_property(false, window, atoms.net_wm_pid, AtomEnum::CARDINAL, 0, 1)?
        .reply()
    {
        Ok(reply) => reply,
        Err(error) if is_stale_window_error(&error) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    Ok(reply.value32().and_then(|mut values| values.next()))
}

fn read_title(conn: &RustConnection, window: Window, atoms: &Atoms) -> Result<Option<String>> {
    let utf8 = match conn
        .get_property(false, window, atoms.net_wm_name, atoms.utf8_string, 0, 1024)?
        .reply()
    {
        Ok(reply) => reply,
        Err(error) if is_stale_window_error(&error) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if !utf8.value.is_empty() {
        return Ok(Some(String::from_utf8_lossy(&utf8.value).into_owned()));
    }

    let legacy = match conn
        .get_property(false, window, AtomEnum::WM_NAME, AtomEnum::STRING, 0, 1024)?
        .reply()
    {
        Ok(reply) => reply,
        Err(error) if is_stale_window_error(&error) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if legacy.value.is_empty() {
        return Ok(None);
    }
    Ok(Some(String::from_utf8_lossy(&legacy.value).into_owned()))
}

fn is_stale_window_error(error: &ReplyError) -> bool {
    matches!(
        error,
        ReplyError::X11Error(error) if error.error_kind == ErrorKind::Window
    )
}
