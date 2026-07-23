//! Real-display specs for viewport behavior that used to be asserted through
//! GPUI test-context internals.

mod support;

use std::time::{Duration, Instant};

use lst_x11_harness::{clipboard::write_clipboard_text, Selection, WheelDir};
use support::{secs, EditorTestExt, TestResult};

fn row_covers_char(record: &lst_x11_harness::StateTraceRecord, line: usize, ch: usize) -> bool {
    record
        .viewport
        .rows
        .iter()
        .any(|row| row.logical_line == line && row.line_start_char <= ch && ch <= row.display_end_char)
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn overlays_do_not_resize_text_viewport() -> TestResult {
    support::run_x11_test("viewport-overlay-size", |session| {
        let (mut editor, _path) = session.open("scratch")?;
        let baseline = editor.read_state()?;
        let baseline_size = baseline
            .viewport
            .bounds_size_px
            .expect("initial text viewport should have bounds");

        editor.keys("<C-g>")?;
        let goto = editor.wait_state("goto overlay open", secs(5), |record| record.goto_line_input.is_some())?;
        assert_eq!(goto.viewport.bounds_size_px, Some(baseline_size), "{goto:?}");

        editor.keys("<C-f>")?;
        let find = editor.wait_state("find overlay open", secs(5), |record| record.find.visible)?;
        assert_eq!(find.viewport.bounds_size_px, Some(baseline_size), "{find:?}");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn alt_z_no_wrap_reveals_horizontal_cursor_and_wrap_resets_scroll() -> TestResult {
    support::run_x11_test("viewport-horizontal-wrap-toggle", |session| {
        let path = session.seed_file("wide.txt", &"x".repeat(2_000))?;
        let mut editor = session.open_file("viewport-horizontal-wrap-toggle", &path)?;

        editor.wait_state("wrap status after first paint", secs(5), |record| {
            record.status_bar.contains("Wrap") && !record.status_bar.contains("No Wrap")
        })?;

        editor.send_keys_settle("<A-z>")?;
        editor.wait_state("no-wrap status", secs(5), |record| {
            record.status_bar.contains("No Wrap")
        })?;

        editor.keys("<C-end>")?;
        let scrolled = editor.wait_state("horizontal cursor reveal", secs(5), |record| {
            record.viewport.scroll_left_px > record.viewport.char_width_px * 10.0
                && matches!(record.cursors.as_slice(), [cursor] if cursor.head_col >= 1_900)
        })?;
        assert!(scrolled.viewport.scroll_left_px > 0.0, "{scrolled:?}");

        editor.send_keys_settle("<A-z>")?;
        let wrapped = editor.wait_state("wrap resets horizontal scroll", secs(5), |record| {
            record.status_bar.contains("Wrap")
                && !record.status_bar.contains("No Wrap")
                && record.viewport.scroll_left_px <= 1.0
        })?;
        assert!(wrapped.viewport.scroll_left_px <= 1.0, "{wrapped:?}");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn no_wrap_line_growth_updates_horizontal_extent() -> TestResult {
    support::run_x11_test("viewport-no-wrap-line-growth", |session| {
        let path = session.seed_file("growing-wide.txt", "short")?;
        let mut editor = session.open_file("viewport-no-wrap-line-growth", &path)?;

        editor.send_keys_settle("<A-z>")?;
        editor.wait_state("no-wrap status", secs(5), |record| {
            record.status_bar.contains("No Wrap")
        })?;
        editor.keys("<C-end>")?;
        write_clipboard_text(Selection::Clipboard, &"x".repeat(400))?;
        editor.keys("<C-v>")?;

        let grown = editor.wait_state("grown line end remains horizontally reachable", secs(10), |record| {
            record.viewport.scroll_left_px > record.viewport.char_width_px * 10.0
                && matches!(record.cursors.as_slice(), [cursor] if cursor.head_col >= 405)
        })?;
        assert!(grown.viewport.scroll_left_px > 0.0, "{grown:?}");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn typing_at_wrapped_line_end_keeps_cursor_visible() -> TestResult {
    support::run_x11_test("viewport-wrapped-eof-typing", |session| {
        let path = session.seed_file("long-wrapped.txt", &"a".repeat(30_000))?;
        let mut editor = session.open_file("viewport-wrapped-eof-typing", &path)?;

        editor.keys("<C-end>")?;
        let before = editor.wait_state("wrapped eof visible before typing", secs(10), |record| {
            matches!(record.cursors.as_slice(), [cursor]
                if cursor.head_line == 0
                    && cursor.head_char >= 30_000
                    && row_covers_char(record, cursor.head_line, cursor.head_char))
        })?;
        assert!(
            before.viewport.scroll_top_px > before.viewport.line_height_px,
            "{before:?}"
        );

        editor.keys("x")?;
        let after = editor.wait_state("wrapped eof visible after typing", secs(10), |record| {
            matches!(record.cursors.as_slice(), [cursor]
                if cursor.head_line == 0
                    && cursor.head_char >= 30_001
                    && row_covers_char(record, cursor.head_line, cursor.head_char))
        })?;
        assert!(
            after.viewport.scroll_top_px + after.viewport.line_height_px * 2.0 >= before.viewport.scroll_top_px,
            "typing at wrapped EOF should not jump back toward the top; before={before:?}, after={after:?}"
        );
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn wheel_scroll_animates_between_detents_and_clamps_at_top() -> TestResult {
    support::run_x11_test("viewport-wheel-scroll", |session| {
        let body = (0..400).map(|i| format!("line {i:03}")).collect::<Vec<_>>().join("\n");
        let path = session.seed_file("tall.txt", &body)?;
        let mut editor = session.open_file("viewport-wheel-scroll", &path)?;
        let baseline = editor.wait_state("first paint", secs(10), |record| record.viewport.line_height_px > 0.0)?;
        let detent = baseline.viewport.line_height_px * 3.0;

        // Park the pointer over the buffer; wheel events land wherever it sits.
        editor.click_center()?;
        editor.wait_state("cursor placed by click", secs(5), |record| {
            record.viewport.scroll_top_px <= 0.5
        })?;
        editor.drain_state_records()?;

        editor.wheel_burst(WheelDir::Down, 1, Duration::ZERO)?;
        let mut observed = Vec::new();
        let deadline = Instant::now() + secs(5);
        while !observed
            .iter()
            .any(|record: &lst_x11_harness::StateTraceRecord| (record.viewport.scroll_top_px - detent).abs() <= 0.5)
        {
            assert!(
                Instant::now() <= deadline,
                "single wheel detent never settled at {detent}px; observed tops: {:?}",
                observed
                    .iter()
                    .map(|record| record.viewport.scroll_top_px)
                    .collect::<Vec<_>>()
            );
            std::thread::sleep(Duration::from_millis(5));
            observed.extend(editor.drain_state_records()?);
        }
        assert!(
            observed
                .iter()
                .any(|record| record.viewport.scroll_top_px > 1.0 && record.viewport.scroll_top_px < detent - 1.0),
            "wheel detent jumped to its target without painting intermediate frames; observed tops: {:?}",
            observed
                .iter()
                .map(|record| record.viewport.scroll_top_px)
                .collect::<Vec<_>>()
        );

        editor.wheel_burst(WheelDir::Down, 3, Duration::from_millis(90))?;
        editor.wait_state("burst accumulates one detent per notch", secs(5), |record| {
            (record.viewport.scroll_top_px - detent * 4.0).abs() <= 0.5
        })?;

        editor.wheel_burst(WheelDir::Up, 8, Duration::from_millis(120))?;
        editor.wait_state("wheel up past the start clamps at the top", secs(5), |record| {
            record.viewport.scroll_top_px <= 0.01
        })?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn light_window_stays_operable_at_responsive_size_and_zoom_extremes() -> TestResult {
    support::run_x11_test("viewport-responsive-light", |session| {
        session.seed_settings(
            "version = 1\n[editor]\nword_wrap = true\n[appearance]\ntheme = 'light'\nzoom_level = 8\n",
        )?;
        let path = session.seed_file("responsive-light.txt", &format!("{}\n", "wrapped text ".repeat(80)))?;
        let mut editor = session.open_file("viewport-responsive-light", &path)?;
        editor.resize(900, 600)?;

        let maximized = editor.wait_state("maximum-zoom light viewport", secs(5), |record| {
            record.theme_name == "Light"
                && record.word_wrap_enabled
                && record.status_bar.contains("Zoom 214%")
                && record
                    .viewport
                    .bounds_size_px
                    .is_some_and(|(width, height)| width > 0.0 && height > 0.0)
        })?;
        assert!(!maximized.viewport.rows.is_empty(), "{maximized:?}");

        editor.keys("<C-0>")?;
        editor.resize(640, 480)?;
        editor.wait_state("narrow 100-percent light viewport", secs(5), |record| {
            record.theme_name == "Light"
                && record.word_wrap_enabled
                && !record.status_bar.contains("Zoom")
                && !record.viewport.rows.is_empty()
        })?;

        editor.keys("<C-f>")?;
        editor.wait_state("find in narrow light window", secs(2), |record| {
            record.find.visible && record.focused_input == "find_query"
        })?;
        editor.keys("<esc><C-,>")?;
        editor.wait_state("settings in narrow light window", secs(2), |record| {
            record.workspace_surface == "settings" && record.focused_input == "settings"
        })?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn dark_window_stays_operable_at_responsive_size_and_zoom_extremes() -> TestResult {
    support::run_x11_test("viewport-responsive-dark", |session| {
        session.seed_settings(
            "version = 1\n[editor]\nword_wrap = false\n[appearance]\ntheme = 'dark'\nzoom_level = 8\n",
        )?;
        let path = session.seed_file("responsive-dark.txt", &"unwrapped".repeat(400))?;
        let mut editor = session.open_file("viewport-responsive-dark", &path)?;
        editor.resize(900, 600)?;

        editor.wait_state("compact dark viewport", secs(5), |record| {
            record.theme_name == "Dark"
                && !record.word_wrap_enabled
                && record.status_bar.contains("Zoom 214%")
                && !record.viewport.rows.is_empty()
        })?;

        editor.keys("<C-0>")?;
        editor.resize(640, 480)?;
        editor.wait_state("narrow 100-percent dark viewport", secs(5), |record| {
            record.theme_name == "Dark"
                && !record.word_wrap_enabled
                && !record.status_bar.contains("Zoom")
                && !record.viewport.rows.is_empty()
        })?;
        editor.keys("<C-S-p>")?;
        editor.wait_state("palette in compact dark window", secs(2), |record| {
            record.workspace_surface == "command_palette" && record.focused_input == "command_palette"
        })?;
        editor.keys("<esc><C-g>")?;
        editor.wait_state("goto in compact dark window", secs(2), |record| {
            record.goto_line_input.is_some() && record.focused_input == "goto_line"
        })?;
        Ok(())
    })
}
