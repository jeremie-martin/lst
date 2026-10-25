//! Launch and X11 lifecycle boundaries of the production application.
mod support;

use std::{
    ffi::{OsStr, OsString},
    fs,
    os::unix::ffi::OsStringExt,
    path::Path,
    process::Command,
    thread,
    time::Duration,
};
use support::{secs, EditorTestExt, TestResult};
use x11rb::{
    connection::Connection as _,
    protocol::{
        xproto::{self, ConnectionExt as _},
        xtest::ConnectionExt as _,
    },
};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn non_utf8_file_paths_open_and_save_without_changing_their_bytes() -> TestResult {
    support::run_x11_test("runtime-path-bytes", |session| {
        let path = session.root().join(OsString::from_vec(b"notes-\xff.txt".to_vec()));
        fs::write(&path, "original")?;
        let mut editor = session.open_file("runtime-path-bytes", &path)?;
        editor.keys("<C-end>!")?;
        editor.save_then_expect_file(&path, "original!")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn launch_titles_publish_utf8_and_a_valid_legacy_fallback() -> TestResult {
    support::run_x11_test("runtime-title", |session| {
        let path = session.seed_file("title.txt", "text")?;
        let editor = session.open_file("日本語–café", &path)?;
        let (conn, _) = x11rb::connect(None)?;
        let name = conn.intern_atom(false, b"_NET_WM_NAME")?.reply()?.atom;
        let utf8 = conn.intern_atom(false, b"UTF8_STRING")?.reply()?.atom;
        let value = conn
            .get_property(false, editor.window_id(), name, utf8, 0, 4096)?
            .reply()?;
        let title = std::str::from_utf8(&value.value)?;
        assert!(title.contains("日本語–café"), "unexpected EWMH title: {title:?}");
        let legacy = conn
            .get_property(
                false,
                editor.window_id(),
                xproto::AtomEnum::WM_NAME,
                xproto::AtomEnum::STRING,
                0,
                4096,
            )?
            .reply()?;
        let expected: Vec<u8> = title.chars().map(|c| u8::try_from(c as u32).unwrap_or(b'?')).collect();
        assert_eq!(legacy.value, expected);
        Ok(())
    })
}

fn frames(path: &Path) -> support::SupportResult<usize> {
    Ok(fs::read_to_string(path)?
        .lines()
        .filter(|line| line.starts_with("frame_wall_ms="))
        .count())
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn unmapped_windows_do_not_render_on_unrelated_input_and_resume_when_shown() -> TestResult {
    support::run_x11_test("runtime-hidden", |session| {
        let path = session.seed_file("hidden.txt", "text")?;
        let trace = session.artifacts().join("frames.log");
        let mut editor = session.open_file_with_env(
            "runtime-hidden",
            &path,
            &[(OsStr::new("LST_BENCH_TRACE_FILE"), trace.as_os_str())],
        )?;
        let (conn, screen) = x11rb::connect(None)?;
        let root = conn.setup().roots[screen].root;
        let shift = conn
            .get_keyboard_mapping(
                conn.setup().min_keycode,
                conn.setup().max_keycode - conn.setup().min_keycode + 1,
            )?
            .reply()?;
        let shift_code = shift
            .keysyms
            .chunks(usize::from(shift.keysyms_per_keycode))
            .position(|symbols| symbols.contains(&0xffe1))
            .ok_or("Shift key missing")? as u8
            + conn.setup().min_keycode;
        conn.unmap_window(editor.window_id())?.check()?;
        conn.flush()?;
        // Exceed GPUI's post-input activity interval and let queued frames finish.
        thread::sleep(Duration::from_millis(1400));
        assert_eq!(
            conn.get_window_attributes(editor.window_id())?.reply()?.map_state,
            xproto::MapState::UNMAPPED,
        );
        let before = frames(&trace)?;
        for _ in 0..4 {
            conn.xtest_fake_input(xproto::KEY_PRESS_EVENT, shift_code, 0, root, 0, 0, 0)?
                .check()?;
            conn.xtest_fake_input(xproto::KEY_RELEASE_EVENT, shift_code, 0, root, 0, 0, 0)?
                .check()?;
            conn.flush()?;
            thread::sleep(Duration::from_millis(150));
        }
        assert_eq!(frames(&trace)?, before, "unmapped window rendered on root input");
        conn.map_window(editor.window_id())?.check()?;
        conn.flush()?;
        editor.click_center()?;
        editor.keys("<C-end>!")?;
        editor.save_then_expect_file(&path, "text!")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip and python-xlib"]
fn a_killed_input_process_cannot_leave_arrows_pressed_in_the_next_editor() -> TestResult {
    support::run_x11_test("runtime-stuck-keys", |session| {
        let path = session.seed_file("keys.txt", "first\nsecond\nthird")?;
        let editor = session.open_file("runtime-stuck-keys-first", &path)?;
        let status = Command::new("python3")
            .args([
                "-c",
                r#"
import os, signal
from Xlib import display, X
from Xlib.ext import xtest
conn = display.Display()
for symbol in [0xff51, 0xff54, 0xffe1]:
    xtest.fake_input(conn, X.KeyPress, conn.keysym_to_keycode(symbol))
conn.sync()
os.kill(os.getpid(), signal.SIGKILL)
"#,
            ])
            .status()?;
        assert!(!status.success());
        let (conn, _) = x11rb::connect(None)?;
        assert!(conn.query_keymap()?.reply()?.keys.iter().any(|bits| *bits != 0));
        drop(editor);
        let mut editor = session.open_file("runtime-stuck-keys-second", &path)?;
        assert!(conn.query_keymap()?.reply()?.keys.iter().all(|bits| *bits == 0));
        editor.keys("<C-home>ok ")?;
        editor.save_then_expect_file(&path, "ok first\nsecond\nthird")?;
        editor.wait_quiet(Duration::from_millis(100), secs(5))?;
        Ok(())
    })
}
