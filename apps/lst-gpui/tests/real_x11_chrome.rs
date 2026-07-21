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
            visible_gutter(record, 0) == "1" && visible_gutter(record, 1) == "2" && visible_gutter(record, 2) == "3"
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
fn configured_theme_is_visible_without_a_status_bar_toggle() -> TestResult {
    support::run_x11_test("chrome-configured-theme", |session| {
        session.seed_settings("version = 1\n[appearance]\ntheme = \"dark\"\n")?;
        let (mut editor, _path) = session.open("scratch")?;

        let configured = editor.wait_state("configured theme", secs(2), |record| {
            record.theme_name == "Dark" && record.theme_button_bounds_px.is_none()
        })?;
        assert_eq!(configured.theme_name, "Dark", "{configured:?}");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn zoom_shortcuts_update_visible_zoom_status_and_reset() -> TestResult {
    support::run_x11_test("chrome-zoom-shortcuts", |session| {
        let (mut editor, _path) = session.open("scratch")?;

        editor.press(KeyChord::Ctrl(Key::Char('=')))?;
        editor.wait_state("zoom in", secs(2), |record| record.status_bar.contains("Zoom 110%"))?;

        editor.press(KeyChord::Ctrl(Key::Char('=')))?;
        editor.wait_state("zoom in again", secs(2), |record| {
            record.status_bar.contains("Zoom 121%")
        })?;

        editor.press(KeyChord::Ctrl(Key::Char('-')))?;
        editor.wait_state("zoom out", secs(2), |record| record.status_bar.contains("Zoom 110%"))?;

        editor.press(KeyChord::Ctrl(Key::Char('0')))?;
        editor.wait_state("zoom reset", secs(2), |record| !record.status_bar.contains("Zoom"))?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn gutter_width_is_stable_through_999_lines_and_tracks_digit_transitions() -> TestResult {
    support::run_x11_test("chrome-dynamic-gutter", |session| {
        let contents = (0..999)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        let path = session.seed_file("dynamic-gutter.txt", &contents)?;
        let mut editor = session.open_file("chrome-dynamic-gutter", &path)?;

        let three_digits = editor.wait_state("three digit gutter", secs(5), |record| {
            record.line_count == 999 && record.viewport.gutter_width_px > 0.0
        })?;
        editor.keys("<C-end><enter>")?;
        let four_digits = editor.wait_state("four digit gutter", secs(5), |record| {
            record.line_count == 1_000 && record.viewport.gutter_width_px > three_digits.viewport.gutter_width_px
        })?;
        let growth = four_digits.viewport.gutter_width_px - three_digits.viewport.gutter_width_px;
        assert!(
            (growth - four_digits.viewport.char_width_px).abs() < 0.25,
            "one digit should add one measured glyph width: before={three_digits:?}, after={four_digits:?}"
        );

        editor.keys("<bs>")?;
        let shrunk = editor.wait_state("three digit gutter restored", secs(5), |record| {
            record.line_count == 999
                && (record.viewport.gutter_width_px - three_digits.viewport.gutter_width_px).abs() < 0.25
        })?;
        assert_eq!(shrunk.line_count, 999, "{shrunk:?}");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn cursor_identifier_highlights_exact_visible_whole_word_occurrences() -> TestResult {
    support::run_x11_test("chrome-identifier-highlights", |session| {
        let path = session.seed_file(
            "identifier-highlights.txt",
            "alpha  beta alpha\nalphabet alpha ALPHA\ncafe\u{301}_count cafe\u{301}_count cafe\n",
        )?;
        let mut editor = session.open_file("chrome-identifier-highlights", &path)?;
        editor.place_cursor_at_document_start()?;

        let alpha = editor.wait_state("alpha occurrences", secs(5), |record| {
            record
                .viewport
                .occurrence_highlights
                .iter()
                .map(|range| (range.start, range.end))
                .collect::<Vec<_>>()
                == vec![(0, 5), (12, 17), (27, 32)]
        })?;
        assert_eq!(alpha.viewport.occurrence_highlights.len(), 3, "{alpha:?}");

        editor.keys("<right><right><right><right><right>")?;
        editor.wait_state("word trailing edge remains highlighted", secs(2), |record| {
            record.cursors[0].head_col == 5
                && record
                    .viewport
                    .occurrence_highlights
                    .iter()
                    .map(|range| (range.start, range.end))
                    .collect::<Vec<_>>()
                    == vec![(0, 5), (12, 17), (27, 32)]
        })?;

        editor.keys("<right>")?;
        editor.wait_state("separator interior has no passive highlight", secs(2), |record| {
            record.cursors[0].head_col == 6 && record.viewport.occurrence_highlights.is_empty()
        })?;

        editor.keys("<right>")?;
        let beta = editor.wait_state("beta occurrence", secs(2), |record| {
            record.cursors[0].head_col == 7
                && record
                    .viewport
                    .occurrence_highlights
                    .iter()
                    .map(|range| (range.start, range.end))
                    .collect::<Vec<_>>()
                    == vec![(7, 11)]
        })?;
        assert_eq!(beta.viewport.occurrence_highlights.len(), 1, "{beta:?}");

        editor.keys("<C-g>3:1<enter>")?;
        let decomposed = editor.wait_state("decomposed identifier occurrences", secs(2), |record| {
            record
                .viewport
                .occurrence_highlights
                .iter()
                .map(|range| (range.start, range.end))
                .collect::<Vec<_>>()
                == vec![(39, 50), (51, 62)]
        })?;
        assert_eq!(decomposed.viewport.occurrence_highlights.len(), 2, "{decomposed:?}");

        editor.keys("<C-end>alpha")?;
        editor.wait_state("typing does not retrigger passive highlights", secs(2), |record| {
            record.cursors[0].head_col == 5 && record.viewport.occurrence_highlights.is_empty()
        })?;

        editor.keys("<C-f><esc>")?;
        editor.wait_state(
            "returning editor focus retriggers passive highlights",
            secs(2),
            |record| {
                record.cursors[0].head_col == 5
                    && record
                        .viewport
                        .occurrence_highlights
                        .iter()
                        .map(|range| (range.start, range.end))
                        .collect::<Vec<_>>()
                        == vec![(0, 5), (12, 17), (27, 32), (68, 73)]
            },
        )?;

        editor.keys("x<bs>")?;
        editor.wait_state("later editing clears focus-triggered highlights", secs(2), |record| {
            record.cursors[0].head_col == 5 && record.viewport.occurrence_highlights.is_empty()
        })?;

        editor.keys("<left>")?;
        let after_motion = editor.wait_state("explicit motion retriggers passive highlights", secs(2), |record| {
            record.cursors[0].head_col == 4
                && record
                    .viewport
                    .occurrence_highlights
                    .iter()
                    .map(|range| (range.start, range.end))
                    .collect::<Vec<_>>()
                    == vec![(0, 5), (12, 17), (27, 32), (68, 73)]
        })?;
        assert_eq!(after_motion.viewport.occurrence_highlights.len(), 4, "{after_motion:?}");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn identifier_highlights_are_bounded_to_the_horizontal_viewport() -> TestResult {
    support::run_x11_test("chrome-bounded-identifier-highlights", |session| {
        const REPEATED_OCCURRENCES: usize = 2_000;
        session.seed_settings("version = 1\n[editor]\nword_wrap = false\n")?;
        let mut contents = "alpha ".repeat(REPEATED_OCCURRENCES);
        contents.push_str("alpha");
        let final_start = contents.chars().count() - "alpha".len();
        let path = session.seed_file("bounded-identifier-highlights.txt", &contents)?;
        let mut editor = session.open_file("chrome-bounded-identifier-highlights", &path)?;
        editor.place_cursor_at_document_start()?;

        let left = editor.wait_state("left viewport occurrences", secs(5), |record| {
            !record.viewport.occurrence_highlights.is_empty()
                && record.viewport.occurrence_highlights.len() < REPEATED_OCCURRENCES
                && record
                    .viewport
                    .occurrence_highlights
                    .last()
                    .is_some_and(|range| range.end < final_start)
        })?;
        assert!(
            left.viewport.occurrence_highlights.len() < REPEATED_OCCURRENCES,
            "{left:?}"
        );

        editor.keys("<C-end>")?;
        let right = editor.wait_state("right viewport occurrences", secs(5), |record| {
            record.viewport.scroll_left_px > record.viewport.char_width_px * 20.0
                && record
                    .viewport
                    .occurrence_highlights
                    .first()
                    .is_some_and(|range| range.start > 0)
                && record
                    .viewport
                    .occurrence_highlights
                    .last()
                    .is_some_and(|range| range.start == final_start && range.end == final_start + 5)
        })?;
        assert!(
            right.viewport.occurrence_highlights.len() < REPEATED_OCCURRENCES,
            "{right:?}"
        );
        Ok(())
    })
}
