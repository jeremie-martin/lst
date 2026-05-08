//! Under-review executable specs for the `Insert` key toggling overtype mode.
//!
//! Pinned contract:
//!
//! - Pressing `Insert` toggles overtype mode. The status bar shows the
//!   substring `"OVR"` while overtype is active and does not show it
//!   otherwise.
//! - In overtype mode, typing a printable character replaces the character
//!   to the right of the caret instead of inserting one. Buffer length is
//!   preserved when there is a character to overwrite.
//! - When the caret is at end-of-line (no character to the right), overtype
//!   falls back to insertion so users do not get stuck unable to extend a
//!   line.
//! - Overtype persists across cursor motion until the user toggles it off
//!   with another `Insert` press.
//!
//! Specs run under the `x11-tdd` profile. Promote to `real_x11_overtype.rs`
//! once green.
//!
//!     cargo nextest run --profile x11-tdd -p lst-gpui --test real_x11_overtype_tdd --run-ignored only

mod support;

use support::{secs, EditorTestExt, TestResult};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn insert_key_toggles_overtype_visible_in_status_bar() -> TestResult {
    support::run_x11_test("overtype-toggle-status", |session| {
        let (mut editor, _path) = session.open("scratch")?;

        // Status bar is OVR-free before the toggle.
        let baseline = editor.read_state()?;
        assert!(
            !baseline.status_bar.contains("OVR"),
            "baseline status bar should not advertise overtype: {:?}",
            baseline.status_bar
        );

        editor.keys("<insert>")?;
        editor.wait_state("overtype on", secs(2), |record| {
            record.status_bar.contains("OVR")
        })?;

        editor.keys("<insert>")?;
        editor.wait_state("overtype off", secs(2), |record| {
            !record.status_bar.contains("OVR")
        })?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn overtype_replaces_char_under_caret_instead_of_inserting() -> TestResult {
    support::run_x11_test("overtype-replace", |session| {
        let path = session.seed_file("overtype-replace.txt", "abcdef")?;
        let mut editor = session.open_file("overtype-replace", &path)?;

        // Caret at column 2 (between 'b' and 'c'). With overtype on, typing
        // 'X' replaces 'c' rather than inserting before it. Buffer length
        // is preserved.
        editor.place_cursor_at_document_start()?;
        editor.keys("<right><right>")?;
        editor.expect_cursor_heads(&[(0, 2)])?;

        editor.keys("<insert>X")?;
        editor.save_then_expect_file(&path, "abXdef")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn overtype_at_end_of_line_falls_back_to_insert() -> TestResult {
    support::run_x11_test("overtype-eol", |session| {
        let path = session.seed_file("overtype-eol.txt", "abc")?;
        let mut editor = session.open_file("overtype-eol", &path)?;

        // Caret at end of line. There is no character to overwrite, so
        // typing 'X' must insert and grow the line — otherwise users get
        // stuck unable to add new content.
        editor.place_cursor_at_document_start()?;
        editor.keys("<end>")?;
        editor.expect_cursor_heads(&[(0, 3)])?;

        editor.keys("<insert>X")?;
        editor.save_then_expect_file(&path, "abcX")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn overtype_persists_across_motion_until_toggled_off() -> TestResult {
    support::run_x11_test("overtype-persists", |session| {
        let path = session.seed_file("overtype-persists.txt", "abcdef")?;
        let mut editor = session.open_file("overtype-persists", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<insert>")?;
        editor.wait_state("overtype on", secs(2), |record| {
            record.status_bar.contains("OVR")
        })?;

        // Trace:
        //   start "abcdef", caret (0,0)
        //   X            → "Xbcdef", caret (0,1)  (overtype 'a' with 'X')
        //   <right>      → caret (0,2)
        //   <right>      → caret (0,3)
        //   Y            → "XbcYef", caret (0,4)  (overtype 'd' with 'Y')
        editor.keys("X<right><right>Y")?;
        editor.save_then_expect_file(&path, "XbcYef")?;

        // Toggle off; subsequent typing inserts again.
        editor.keys("<insert>")?;
        editor.wait_state("overtype off", secs(2), |record| {
            !record.status_bar.contains("OVR")
        })?;
        editor.keys("Z")?;
        editor.save_then_expect_file(&path, "XbcYZef")?;
        Ok(())
    })
}
