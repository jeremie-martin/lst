use x11rb::protocol::xproto::{Atom, ConnectionExt as _};
use x11rb::rust_connection::RustConnection;

use crate::Result;

pub(crate) struct Atoms {
    pub(crate) net_active_window: Atom,
    pub(crate) net_current_desktop: Atom,
    pub(crate) net_wm_desktop: Atom,
    pub(crate) net_wm_state: Atom,
    pub(crate) net_wm_state_above: Atom,
    pub(crate) net_wm_name: Atom,
    pub(crate) net_wm_pid: Atom,
    pub(crate) utf8_string: Atom,
}

impl Atoms {
    pub(crate) fn intern(conn: &RustConnection) -> Result<Self> {
        Ok(Self {
            net_active_window: intern_atom(conn, b"_NET_ACTIVE_WINDOW")?,
            net_current_desktop: intern_atom(conn, b"_NET_CURRENT_DESKTOP")?,
            net_wm_desktop: intern_atom(conn, b"_NET_WM_DESKTOP")?,
            net_wm_state: intern_atom(conn, b"_NET_WM_STATE")?,
            net_wm_state_above: intern_atom(conn, b"_NET_WM_STATE_ABOVE")?,
            net_wm_name: intern_atom(conn, b"_NET_WM_NAME")?,
            net_wm_pid: intern_atom(conn, b"_NET_WM_PID")?,
            utf8_string: intern_atom(conn, b"UTF8_STRING")?,
        })
    }
}

fn intern_atom(conn: &RustConnection, name: &[u8]) -> Result<Atom> {
    Ok(conn.intern_atom(false, name)?.reply()?.atom)
}
