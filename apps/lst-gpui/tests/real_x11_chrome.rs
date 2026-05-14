//! Real-display specs for visible editor chrome. These tests assert on UI
//! state that a user can see: status text, theme label, and line-number text.

mod support;

use lst_x11_harness::{Key, KeyChord, StateTraceRecord};

use support::{secs, EditorTestExt, TestResult};

fn visible_gutter(record: &StateTraceRecord, line: usize) -> String {
    record
        .viewport
        .rows
        .iter()
        .find(|row| row.logical_line == line)
        .and_then(|row| row.gutter_text.as_deref())
        .unwrap_or("")
        .trim()
        .to_string()
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn line_number_modes_render_absolute_relative_and_hybrid_text() -> TestResult {
    support::run_x11_test("chrome-line-number-modes", |session| {
        let path = session.seed_file("line-numbers.txt", "one\ntwo\nthree\nfour\n")?;
        let mut editor = session.open_file("chrome-line-number-modes", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<down>")?;
        editor.expect_cursor_heads(&[(1, 0)])?;

        let absolute = editor.wait_state("absolute gutter text", secs(2), |record| {
            visible_gutter(record, 0) == "1"
                && visible_gutter(record, 1) == "2"
                && visible_gutter(record, 2) == "3"
        })?;
        assert_eq!(visible_gutter(&absolute, 1), "2", "{absolute:?}");

        editor.keys("<A-l>")?;
        let relative = editor.wait_state("relative gutter text", secs(2), |record| {
            record.status_message == "Line numbers: Relative"
                && visible_gutter(record, 0) == "1"
                && visible_gutter(record, 1) == "0"
                && visible_gutter(record, 2) == "1"
        })?;
        assert_eq!(visible_gutter(&relative, 1), "0", "{relative:?}");

        editor.keys("<A-l>")?;
        let hybrid = editor.wait_state("hybrid gutter text", secs(2), |record| {
            record.status_message == "Line numbers: Hybrid"
                && visible_gutter(record, 0) == "1"
                && visible_gutter(record, 1) == "2"
                && visible_gutter(record, 2) == "1"
        })?;
        assert_eq!(visible_gutter(&hybrid, 1), "2", "{hybrid:?}");

        editor.keys("<A-l>")?;
        editor.wait_state("absolute gutter restored", secs(2), |record| {
            record.status_message == "Line numbers: Absolute"
                && visible_gutter(record, 0) == "1"
                && visible_gutter(record, 1) == "2"
                && visible_gutter(record, 2) == "3"
        })?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn theme_button_cycles_visible_theme_label() -> TestResult {
    support::run_x11_test("chrome-theme-cycle", |session| {
        let (mut editor, _path) = session.open("scratch")?;

        editor.wait_state("initial theme", secs(2), |record| {
            record.theme_name == "Dark" && record.theme_button_bounds_px.is_some()
        })?;

        editor.click_theme_button()?;
        editor.wait_state("light theme", secs(2), |record| {
            record.theme_name == "Light"
        })?;

        editor.click_theme_button()?;
        editor.wait_state("dark theme restored", secs(2), |record| {
            record.theme_name == "Dark"
        })?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn zoom_shortcuts_update_visible_zoom_status_and_reset() -> TestResult {
    support::run_x11_test("chrome-zoom-shortcuts", |session| {
        let (mut editor, _path) = session.open("scratch")?;

        editor.press(KeyChord::Ctrl(Key::Char('=')))?;
        editor.wait_state("zoom in", secs(2), |record| {
            record.status_bar.contains("Zoom 110%")
        })?;

        editor.press(KeyChord::Ctrl(Key::Char('=')))?;
        editor.wait_state("zoom in again", secs(2), |record| {
            record.status_bar.contains("Zoom 121%")
        })?;

        editor.press(KeyChord::Ctrl(Key::Char('-')))?;
        editor.wait_state("zoom out", secs(2), |record| {
            record.status_bar.contains("Zoom 110%")
        })?;

        editor.press(KeyChord::Ctrl(Key::Char('0')))?;
        editor.wait_state("zoom reset", secs(2), |record| {
            !record.status_bar.contains("Zoom")
        })?;
        Ok(())
    })
}
