use std::thread;
use std::time::{Duration, Instant};

use x11rb::connection::Connection as _;
use x11rb::protocol::xproto::{self, ConnectionExt as _, Keycode, Window};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;
use x11rb::NONE;

use crate::x11::keycodes::Keycodes;
use crate::x11::window::WindowInfo;
use crate::Result;

/// Pause between pointer motion and a click. The X server processes motion
/// events asynchronously, and clicking before the motion has been delivered
/// to the target window can route the click to the previous pointer location.
pub(crate) const POINTER_SETTLE: Duration = Duration::from_millis(50);
const BUTTON_HOLD: Duration = Duration::from_millis(5);
const MULTI_CLICK_INTERVAL: Duration = Duration::from_millis(25);
const KEY_HOLD: Duration = Duration::from_millis(20);
pub(crate) const KEY_PHASE_SETTLE: Duration = Duration::from_millis(20);

pub(crate) const BUTTON_LEFT: u8 = 1;
pub(crate) const BUTTON_MIDDLE: u8 = 2;
pub(crate) const BUTTON_WHEEL_UP: u8 = 4;
pub(crate) const BUTTON_WHEEL_DOWN: u8 = 5;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ModifierState {
    pub(crate) ctrl: bool,
    pub(crate) alt: bool,
    pub(crate) shift: bool,
    pub(crate) platform: bool,
}

impl ModifierState {
    fn any(self) -> bool {
        self.ctrl || self.alt || self.shift || self.platform
    }
}

pub(crate) fn move_pointer_to_window_point(
    conn: &RustConnection,
    root: Window,
    window: &WindowInfo,
    local_x: i32,
    local_y: i32,
) -> Result<()> {
    let translated = conn
        .translate_coordinates(window.id, root, clamp_i16(local_x), clamp_i16(local_y))?
        .reply()?;
    let x = translated.dst_x;
    let y = translated.dst_y;
    conn.warp_pointer(NONE, root, 0, 0, 0, 0, x, y)?;
    conn.flush()?;
    Ok(())
}

pub(crate) fn move_pointer_to_window_center(
    conn: &RustConnection,
    root: Window,
    window: &WindowInfo,
) -> Result<()> {
    move_pointer_to_window_point(
        conn,
        root,
        window,
        i32::from(window.width) / 2,
        i32::from(window.height) / 2,
    )
}

pub(crate) fn click_button(conn: &RustConnection, root: Window, button: u8) -> Result<()> {
    button_press(conn, root, button)?;
    conn.flush()?;
    thread::sleep(BUTTON_HOLD);
    button_release(conn, root, button)?;
    conn.flush()?;
    Ok(())
}

pub(crate) fn button_press(conn: &RustConnection, root: Window, button: u8) -> Result<()> {
    let (x, y) = pointer_root_position(conn, root)?;
    conn.xtest_fake_input(xproto::BUTTON_PRESS_EVENT, button, 0, root, x, y, 0)?;
    Ok(())
}

pub(crate) fn button_release(conn: &RustConnection, root: Window, button: u8) -> Result<()> {
    let (x, y) = pointer_root_position(conn, root)?;
    conn.xtest_fake_input(xproto::BUTTON_RELEASE_EVENT, button, 0, root, x, y, 0)?;
    Ok(())
}

/// Click `button` `count` times in quick succession. GPUI's input adapter
/// detects double / triple / quadruple clicks by comparing X event
/// timestamps; X servers stamp at queue time with millisecond resolution,
/// so a microsecond-tight burst can hand back identical timestamps and
/// fail the click-count promotion. Sleep briefly between pairs — well
/// inside the typical click-interval threshold (~200ms) but long enough for
/// the application to observe distinct click phases reliably under load.
pub(crate) fn multi_click_button(
    conn: &RustConnection,
    root: Window,
    button: u8,
    count: usize,
) -> Result<()> {
    for index in 0..count {
        if index > 0 {
            conn.flush()?;
            thread::sleep(MULTI_CLICK_INTERVAL);
        }
        button_press(conn, root, button)?;
        conn.flush()?;
        thread::sleep(BUTTON_HOLD);
        button_release(conn, root, button)?;
    }
    conn.flush()?;
    Ok(())
}

pub(crate) fn key_press(conn: &RustConnection, root: Window, code: Keycode) -> Result<()> {
    conn.xtest_fake_input(xproto::KEY_PRESS_EVENT, code, 0, root, 0, 0, 0)?;
    Ok(())
}

pub(crate) fn key_release(conn: &RustConnection, root: Window, code: Keycode) -> Result<()> {
    conn.xtest_fake_input(xproto::KEY_RELEASE_EVENT, code, 0, root, 0, 0, 0)?;
    Ok(())
}

/// Press `code` with optional `Ctrl`/`Alt`/`Shift` modifiers held. Modifiers
/// are pressed before the key and released after, so the X server sees a
/// well-formed chord regardless of what the host's actual keyboard state is.
pub(crate) fn chord(
    conn: &RustConnection,
    root: Window,
    kc: &Keycodes,
    code: Keycode,
    ctrl: bool,
    alt: bool,
    shift: bool,
) -> Result<()> {
    chord_with_modifiers(
        conn,
        root,
        kc,
        code,
        ModifierState {
            ctrl,
            alt,
            shift,
            platform: false,
        },
    )
}

pub(crate) fn chord_with_modifiers(
    conn: &RustConnection,
    root: Window,
    kc: &Keycodes,
    code: Keycode,
    modifiers: ModifierState,
) -> Result<()> {
    let has_modifiers = modifiers.any();
    if modifiers.platform {
        conn.xtest_grab_control(true)?.check()?;
    }
    if has_modifiers {
        press_modifiers_with_platform(
            conn,
            root,
            kc,
            modifiers.ctrl,
            modifiers.alt,
            modifiers.shift,
            modifiers.platform,
        )?;
        conn.flush()?;
        thread::sleep(KEY_PHASE_SETTLE);
    }
    tap_key(conn, root, code)?;
    if has_modifiers {
        conn.flush()?;
        thread::sleep(KEY_PHASE_SETTLE);
        release_modifiers_with_platform(
            conn,
            root,
            kc,
            modifiers.ctrl,
            modifiers.alt,
            modifiers.shift,
            modifiers.platform,
        )?;
    }
    conn.flush()?;
    if modifiers.platform {
        conn.xtest_grab_control(false)?.check()?;
    }
    Ok(())
}

/// Press the requested modifier keys without flushing. Pair with
/// [`release_modifiers`] to keep modifiers held across multiple key taps
/// (chord-hold), or modifier-bearing mouse clicks.
pub(crate) fn press_modifiers(
    conn: &RustConnection,
    root: Window,
    kc: &Keycodes,
    ctrl: bool,
    alt: bool,
    shift: bool,
) -> Result<()> {
    press_modifiers_with_platform(conn, root, kc, ctrl, alt, shift, false)
}

pub(crate) fn press_modifiers_with_platform(
    conn: &RustConnection,
    root: Window,
    kc: &Keycodes,
    ctrl: bool,
    alt: bool,
    shift: bool,
    platform: bool,
) -> Result<()> {
    if ctrl {
        key_press(conn, root, kc.control_l)?;
    }
    if alt {
        key_press(conn, root, kc.alt_l)?;
    }
    if platform {
        key_press(conn, root, kc.super_l)?;
    }
    if shift {
        key_press(conn, root, kc.shift_l)?;
    }
    Ok(())
}

/// Release the requested modifier keys in reverse order. Pair with
/// [`press_modifiers`].
pub(crate) fn release_modifiers(
    conn: &RustConnection,
    root: Window,
    kc: &Keycodes,
    ctrl: bool,
    alt: bool,
    shift: bool,
) -> Result<()> {
    release_modifiers_with_platform(conn, root, kc, ctrl, alt, shift, false)
}

pub(crate) fn release_modifiers_with_platform(
    conn: &RustConnection,
    root: Window,
    kc: &Keycodes,
    ctrl: bool,
    alt: bool,
    shift: bool,
    platform: bool,
) -> Result<()> {
    if shift {
        key_release(conn, root, kc.shift_l)?;
    }
    if platform {
        key_release(conn, root, kc.super_l)?;
    }
    if alt {
        key_release(conn, root, kc.alt_l)?;
    }
    if ctrl {
        key_release(conn, root, kc.control_l)?;
    }
    Ok(())
}

/// Press and release a single keycode without flushing. Used inside
/// chord-hold spans where the caller wants to control the surrounding
/// modifier state explicitly.
pub(crate) fn tap_key(conn: &RustConnection, root: Window, code: Keycode) -> Result<()> {
    key_press(conn, root, code)?;
    conn.flush()?;
    thread::sleep(KEY_HOLD);
    key_release(conn, root, code)?;
    Ok(())
}

pub(crate) fn wheel_burst(
    conn: &RustConnection,
    root: Window,
    button: u8,
    count: usize,
    total: Duration,
) -> Result<()> {
    let start = Instant::now();
    for index in 0..count {
        let (x, y) = pointer_root_position(conn, root)?;
        conn.xtest_fake_input(xproto::BUTTON_PRESS_EVENT, button, 0, root, x, y, 0)?;
        conn.xtest_fake_input(xproto::BUTTON_RELEASE_EVENT, button, 0, root, x, y, 0)?;
        conn.flush()?;
        if !total.is_zero() && count > 0 {
            let target = start + total.mul_f64((index + 1) as f64 / count as f64);
            let now = Instant::now();
            if target > now {
                thread::sleep(target - now);
            }
        }
    }
    Ok(())
}

fn clamp_i16(value: i32) -> i16 {
    value.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
}

fn pointer_root_position(conn: &RustConnection, root: Window) -> Result<(i16, i16)> {
    let pointer = conn.query_pointer(root)?.reply()?;
    Ok((pointer.root_x, pointer.root_y))
}
