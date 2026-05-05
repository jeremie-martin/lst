use std::collections::HashMap;
use std::io;

use x11rb::connection::Connection as _;
use x11rb::protocol::xkb::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{ConnectionExt as _, GetKeyboardMappingReply, Keycode};
use x11rb::rust_connection::RustConnection;

use crate::Result;

const KEYSYM_CONTROL_L: u32 = 0xffe3;
const KEYSYM_SHIFT_L: u32 = 0xffe1;
const KEYSYM_ALT_L: u32 = 0xffe9;
const KEYSYM_TAB: u32 = 0xff09;
const KEYSYM_SPACE: u32 = 0x20;
const KEYSYM_RETURN: u32 = 0xff0d;
const KEYSYM_ESCAPE: u32 = 0xff1b;
const KEYSYM_BACKSPACE: u32 = 0xff08;
const KEYSYM_DELETE: u32 = 0xffff;
const KEYSYM_HOME: u32 = 0xff50;
const KEYSYM_LEFT: u32 = 0xff51;
const KEYSYM_UP: u32 = 0xff52;
const KEYSYM_RIGHT: u32 = 0xff53;
const KEYSYM_DOWN: u32 = 0xff54;
const KEYSYM_END: u32 = 0xff57;
const KEYSYM_PAGE_UP: u32 = 0xff55;
const KEYSYM_PAGE_DOWN: u32 = 0xff56;

pub(crate) struct Keycodes {
    pub(crate) control_l: Keycode,
    pub(crate) shift_l: Keycode,
    pub(crate) alt_l: Keycode,
    pub(crate) tab: Keycode,
    pub(crate) space: Keycode,
    pub(crate) enter: Keycode,
    pub(crate) escape: Keycode,
    pub(crate) backspace: Keycode,
    pub(crate) delete: Keycode,
    pub(crate) home: Keycode,
    pub(crate) end: Keycode,
    pub(crate) left: Keycode,
    pub(crate) right: Keycode,
    pub(crate) up: Keycode,
    pub(crate) down: Keycode,
    pub(crate) page_up: Keycode,
    pub(crate) page_down: Keycode,
    /// Printable ASCII (`0x20..=0x7E`) → (keycode, needs_shift). The full
    /// table is populated at startup so callers don't have to extend the
    /// harness every time they want to type a digit or punctuation char.
    chars: HashMap<char, (Keycode, bool)>,
}

impl Keycodes {
    pub(crate) fn resolve(conn: &RustConnection) -> Result<Self> {
        let setup = conn.setup();
        let count = setup.max_keycode - setup.min_keycode + 1;
        let reply = conn
            .get_keyboard_mapping(setup.min_keycode, count)?
            .reply()?;
        let active_group = active_group(conn)?;

        let mut chars = HashMap::new();
        for cp in 0x20u32..=0x7E {
            if let Some(entry) = lookup(&reply, setup.min_keycode, cp, active_group) {
                if let Some(ch) = char::from_u32(cp) {
                    chars.insert(ch, entry);
                }
            }
        }

        Ok(Self {
            control_l: require(&reply, setup.min_keycode, KEYSYM_CONTROL_L, active_group)?,
            shift_l: require(&reply, setup.min_keycode, KEYSYM_SHIFT_L, active_group)?,
            alt_l: require(&reply, setup.min_keycode, KEYSYM_ALT_L, active_group)?,
            tab: require(&reply, setup.min_keycode, KEYSYM_TAB, active_group)?,
            space: require(&reply, setup.min_keycode, KEYSYM_SPACE, active_group)?,
            enter: require(&reply, setup.min_keycode, KEYSYM_RETURN, active_group)?,
            escape: require(&reply, setup.min_keycode, KEYSYM_ESCAPE, active_group)?,
            backspace: require(&reply, setup.min_keycode, KEYSYM_BACKSPACE, active_group)?,
            delete: require(&reply, setup.min_keycode, KEYSYM_DELETE, active_group)?,
            home: require(&reply, setup.min_keycode, KEYSYM_HOME, active_group)?,
            end: require(&reply, setup.min_keycode, KEYSYM_END, active_group)?,
            left: require(&reply, setup.min_keycode, KEYSYM_LEFT, active_group)?,
            right: require(&reply, setup.min_keycode, KEYSYM_RIGHT, active_group)?,
            up: require(&reply, setup.min_keycode, KEYSYM_UP, active_group)?,
            down: require(&reply, setup.min_keycode, KEYSYM_DOWN, active_group)?,
            page_up: require(&reply, setup.min_keycode, KEYSYM_PAGE_UP, active_group)?,
            page_down: require(&reply, setup.min_keycode, KEYSYM_PAGE_DOWN, active_group)?,
            chars,
        })
    }

    /// Returns `(keycode, needs_shift)` for an ASCII printable, or `None`
    /// if the active layout doesn't expose this character at level 0 or 1.
    pub(crate) fn lookup_char(&self, ch: char) -> Option<(Keycode, bool)> {
        self.chars.get(&ch).copied()
    }
}

fn active_group(conn: &RustConnection) -> Result<usize> {
    let reply = conn.xkb_get_state(xkb::ID::USE_CORE_KBD.into())?.reply()?;
    Ok(usize::from(u8::from(reply.group)))
}

/// Find a keysym in the keymap and report which level (0 = unshifted,
/// 1 = shifted) it lives at, falling back to other groups if needed.
fn lookup(
    reply: &GetKeyboardMappingReply,
    min_keycode: Keycode,
    keysym: u32,
    active_group: usize,
) -> Option<(Keycode, bool)> {
    let per = reply.keysyms_per_keycode as usize;
    let active_start = active_group.saturating_mul(2);

    for (index, group) in reply.keysyms.chunks(per).enumerate() {
        if let Some(level) = level_in_window(group, keysym, active_start) {
            return Some((min_keycode + index as u8, level == 1));
        }
    }
    if active_start != 0 {
        for (index, group) in reply.keysyms.chunks(per).enumerate() {
            if let Some(level) = level_in_window(group, keysym, 0) {
                return Some((min_keycode + index as u8, level == 1));
            }
        }
    }
    // Last-resort: any level in any group. Treat odd-indexed levels as
    // requiring shift (level 1, 3, …); even-indexed as unshifted.
    for (index, group) in reply.keysyms.chunks(per).enumerate() {
        for (offset, &k) in group.iter().enumerate() {
            if k == keysym {
                return Some((min_keycode + index as u8, offset % 2 == 1));
            }
        }
    }
    None
}

fn level_in_window(group: &[u32], keysym: u32, start: usize) -> Option<usize> {
    if group.get(start) == Some(&keysym) {
        return Some(0);
    }
    if group.get(start + 1) == Some(&keysym) {
        return Some(1);
    }
    None
}

fn require(
    reply: &GetKeyboardMappingReply,
    min_keycode: Keycode,
    keysym: u32,
    active_group: usize,
) -> Result<Keycode> {
    lookup(reply, min_keycode, keysym, active_group)
        .map(|(kc, _)| kc)
        .ok_or_else(|| {
            io::Error::other(format!("could not resolve X11 keysym 0x{keysym:x}")).into()
        })
}
