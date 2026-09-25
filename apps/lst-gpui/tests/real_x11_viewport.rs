//! Real-display specs for viewport behavior that used to be asserted through
//! GPUI test-context internals.

mod support;

use std::time::{Duration, Instant};

use lst_x11_harness::{clipboard::write_clipboard_text, Editor, Selection, WheelDir};
use support::{secs, EditorTestExt, SupportResult, TestResult};

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
        editor.wait_state("horizontal cursor reveal", secs(5), |record| {
            record.viewport.scroll_left_px > record.viewport.char_width_px * 10.0
                && matches!(record.cursors.as_slice(), [cursor] if cursor.head_col >= 1_900)
        })?;

        editor.send_keys_settle("<A-z>")?;
        editor.wait_state("wrap resets horizontal scroll", secs(5), |record| {
            record.status_bar.contains("Wrap")
                && !record.status_bar.contains("No Wrap")
                && record.viewport.scroll_left_px <= 1.0
        })?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn no_wrap_paste_scrolls_horizontally_to_the_grown_line_end() -> TestResult {
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

        editor.wait_state("grown line end remains horizontally reachable", secs(10), |record| {
            record.viewport.scroll_left_px > record.viewport.char_width_px * 10.0
                && matches!(record.cursors.as_slice(), [cursor] if cursor.head_col >= 405)
        })?;
        Ok(())
    })
}

/// Page right through the horizontal scrollbar track until the thumb covers
/// the click point and the offset stops changing, and return that offset:
/// the no-wrap horizontal extent.
fn scroll_to_right_end(editor: &mut Editor) -> SupportResult<f32> {
    let state = editor.read_state()?;
    let (x, y) = state.viewport.bounds_origin_px.ok_or("viewport origin")?;
    let (width, height) = state.viewport.bounds_size_px.ok_or("viewport size")?;
    let scale = state.viewport.scale_factor;
    let mut previous = None;
    for _ in 0..40 {
        editor.click_at((x + width - 11.0 * scale) as i32, (y + height - 5.0 * scale) as i32)?;
        editor.wait_quiet(Duration::from_millis(75), secs(5))?;
        let current = editor.read_state()?.viewport.scroll_left_px;
        if previous == Some(current) {
            return Ok(current);
        }
        previous = Some(current);
    }
    Err(format!("horizontal scrolling never settled at an end; last offset {previous:?}").into())
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn no_wrap_horizontal_extent_tracks_a_growing_and_shrinking_longest_line() -> TestResult {
    support::run_x11_test("viewport-no-wrap-longest-line", |session| {
        let second = "b".repeat(600);
        let path = session.seed_file("widest.txt", &format!("{}\n{second}", "a".repeat(400)))?;
        let mut editor = session.open_file("viewport-no-wrap-longest-line", &path)?;
        editor.send_keys_settle("<A-z>")?;
        editor.wait_state("no-wrap status", secs(5), |record| {
            record.status_bar.contains("No Wrap")
        })?;
        let original = scroll_to_right_end(&mut editor)?;
        let viewport = editor.read_state()?.viewport;
        let char_width = viewport.char_width_px;
        let (width, _) = viewport.bounds_size_px.ok_or("viewport size")?;
        // At the end, the 600-character line's last column is in view.
        let longest = 600.0 * char_width;
        assert!(
            original < longest && original + width >= longest,
            "extent {original}, viewport width {width}, longest line {longest}px"
        );

        editor.keys("<C-home><end>")?;
        write_clipboard_text(Selection::Clipboard, &"a".repeat(400))?;
        editor.keys("<C-v>xxxxxxxxxx")?;
        editor.expect_cursor_heads(&[(0, 810)])?;
        let grown = scroll_to_right_end(&mut editor)?;
        assert!(
            (grown - original - 210.0 * char_width).abs() < 2.0,
            "{original} -> {grown}, character width {char_width}"
        );

        editor.keys("<C-g>1:401<enter><enter>")?;
        editor.expect_cursor_heads(&[(1, 0)])?;
        let split = scroll_to_right_end(&mut editor)?;
        assert!(
            (split - original).abs() < 2.0,
            "split extent {split}, original {original}"
        );
        editor.keys("<backspace>")?;
        editor.expect_cursor_heads(&[(0, 400)])?;
        let joined = scroll_to_right_end(&mut editor)?;
        assert!((joined - grown).abs() < 2.0, "joined extent {joined}, grown {grown}");

        editor.keys("<C-home><S-end>z")?;
        editor.expect_cursor_heads(&[(0, 1)])?;
        let shrunk = scroll_to_right_end(&mut editor)?;
        assert!((shrunk - original).abs() < 2.0, "{original} -> {grown} -> {shrunk}");
        editor.save_then_expect_file(&path, &format!("z\n{second}"))?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn large_paste_reveals_the_cursor_in_its_first_frame() -> TestResult {
    support::run_x11_test("viewport-large-paste-first-frame", |session| {
        let path = session.seed_file("paste-target.txt", "")?;
        let mut editor = session.open_file("viewport-large-paste-first-frame", &path)?;
        editor.wait_text_viewport(secs(10))?;

        let pasted = (0..2_000).map(|line| format!("pasted line {line}\n")).collect::<String>();
        write_clipboard_text(Selection::Clipboard, &pasted)?;
        editor.keys("<C-v>")?;

        // The scroll extent grows with the paste; revealing against the
        // previous frame's extent left one frame at the old position.
        let first = editor.wait_transient_state("first frame showing the paste", secs(10), |record| {
            record.line_count > 2_000
        })?;
        assert!(
            matches!(first.cursors.as_slice(), [cursor] if row_covers_char(&first, cursor.head_line, cursor.head_char)),
            "cursor {:?} not painted in the first frame (scroll_top {})",
            first.cursors,
            first.viewport.scroll_top_px
        );
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
fn window_stays_operable_across_wrap_modes_sizes_and_zoom_extremes() -> TestResult {
    support::run_x11_test("viewport-responsive", |session| {
        session.seed_settings(
            "version = 1\n[editor]\nword_wrap = true\n[appearance]\ntheme = 'light'\nzoom_level = 8\n",
        )?;
        let path = session.seed_file("responsive.txt", &format!("{}\n", "wrapped text ".repeat(80)))?;
        let mut editor = session.open_file("viewport-responsive", &path)?;
        editor.resize(900, 600)?;

        let wraps = |record: &lst_x11_harness::StateTraceRecord| {
            record.viewport.rows.iter().filter(|row| row.logical_line == 0).count() > 1
        };
        editor.wait_state("maximum-zoom wrapped light viewport", secs(5), |record| {
            record.theme_name == "Light"
                && record.word_wrap_enabled
                && record.status_bar.contains("Zoom 214%")
                && wraps(record)
        })?;

        editor.send_keys_settle("<A-z>")?;
        editor.wait_state("maximum-zoom unwrapped viewport", secs(5), |record| {
            !record.word_wrap_enabled && !record.viewport.rows.is_empty() && !wraps(record)
        })?;

        editor.keys(&"<C-->".repeat(12))?;
        editor.resize(640, 480)?;
        editor.wait_state("minimum-zoom narrow viewport", secs(5), |record| {
            record.status_bar.contains("Zoom 68%")
                && record
                    .viewport
                    .bounds_size_px
                    .is_some_and(|(width, height)| width <= 640.0 && height < 480.0)
                && !record.viewport.rows.is_empty()
        })?;

        editor.keys("<C-f>")?;
        editor.wait_state("find in narrow window", secs(2), |record| {
            record.find.visible && record.focused_input == "find_query"
        })?;
        editor.keys("<esc><C-S-p>")?;
        editor.wait_state("palette in narrow window", secs(2), |record| {
            record.workspace_surface == "command_palette" && record.focused_input == "command_palette"
        })?;
        editor.keys("<esc><C-g>")?;
        editor.wait_state("goto in narrow window", secs(2), |record| {
            record.goto_line_input.is_some() && record.focused_input == "goto_line"
        })?;
        editor.keys("<esc><C-,>")?;
        editor.wait_state("settings in narrow window", secs(2), |record| {
            record.workspace_surface == "settings" && record.focused_input == "settings"
        })?;
        Ok(())
    })
}
