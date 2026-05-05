use std::thread;
use std::time::{Duration, Instant};

use x11rb::connection::Connection as _;
use x11rb::protocol::xproto::{self, Keycode, Window};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;

use crate::x11::keycodes::Keycodes;
use crate::x11::window::WindowInfo;
use crate::Result;

/// Pause between pointer motion and a click. The X server processes motion
/// events asynchronously, and clicking before the motion has been delivered
/// to the target window can route the click to the previous pointer location.
pub(crate) const POINTER_SETTLE: Duration = Duration::from_millis(50);

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
    let x = clamp_i16(i32::from(window.root_x) + local_x);
    let y = clamp_i16(i32::from(window.root_y) + local_y);
    conn.xtest_fake_input(xproto::MOTION_NOTIFY_EVENT, 0, 0, root, x, y, 0)?;
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
    conn.xtest_fake_input(xproto::BUTTON_PRESS_EVENT, button, 0, root, 0, 0, 0)?;
    conn.xtest_fake_input(xproto::BUTTON_RELEASE_EVENT, button, 0, root, 0, 0, 0)?;
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

/// Press `code` with optional `Ctrl` and `Shift` modifiers held. Modifiers
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
    if ctrl {
        key_press(conn, root, kc.control_l)?;
    }
    if alt {
        key_press(conn, root, kc.alt_l)?;
    }
    if shift {
        key_press(conn, root, kc.shift_l)?;
    }
    key_press(conn, root, code)?;
    key_release(conn, root, code)?;
    if shift {
        key_release(conn, root, kc.shift_l)?;
    }
    if alt {
        key_release(conn, root, kc.alt_l)?;
    }
    if ctrl {
        key_release(conn, root, kc.control_l)?;
    }
    conn.flush()?;
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
        conn.xtest_fake_input(xproto::BUTTON_PRESS_EVENT, button, 0, root, 0, 0, 0)?;
        conn.xtest_fake_input(xproto::BUTTON_RELEASE_EVENT, button, 0, root, 0, 0, 0)?;
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
