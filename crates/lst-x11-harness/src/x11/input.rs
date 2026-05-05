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

pub(crate) const BUTTON_LEFT: u8 = 1;
pub(crate) const BUTTON_MIDDLE: u8 = 2;
pub(crate) const BUTTON_WHEEL_UP: u8 = 4;
pub(crate) const BUTTON_WHEEL_DOWN: u8 = 5;

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
/// fail the click-count promotion. Sleep a single millisecond between
/// pairs — well inside the typical click-interval threshold (~200ms) but
/// long enough to guarantee distinct timestamps on every reasonable
/// server clock.
pub(crate) fn multi_click_button(
    conn: &RustConnection,
    root: Window,
    button: u8,
    count: usize,
) -> Result<()> {
    for index in 0..count {
        if index > 0 {
            conn.flush()?;
            thread::sleep(Duration::from_millis(1));
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
    press_modifiers(conn, root, kc, ctrl, alt, shift)?;
    tap_key(conn, root, code)?;
    release_modifiers(conn, root, kc, ctrl, alt, shift)?;
    conn.flush()?;
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
    if ctrl {
        key_press(conn, root, kc.control_l)?;
    }
    if alt {
        key_press(conn, root, kc.alt_l)?;
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
    if shift {
        key_release(conn, root, kc.shift_l)?;
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
