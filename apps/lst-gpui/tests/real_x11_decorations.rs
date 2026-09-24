//! Real-display specs for passive editor decorations: bracket matching and
//! guides, whitespace and control markers, rulers, and highlights of the
//! identifier under the caret or the selected text.

mod support;

use support::{secs, EditorTestExt, TestResult};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn guide_decorations_are_disabled_by_default() -> TestResult {
    support::run_x11_test("polish-guides-default-off", |session| {
        let path = session.seed_file("default-guides.rs", "fn main() {\n    let value = (1 + 2);\n}\n")?;
        let mut editor = session.open_file("polish-guides-default-off", &path)?;

        let state = editor.wait_state("default guide settings", secs(5), |record| {
            record.viewport.structural_pair_count >= 3
                && record.viewport.guide_count == 0
                && record.editor_polish.bracket_pair_guides == "off"
                && record.editor_polish.bracket_pair_horizontal_guides == "off"
                && !record.editor_polish.indent_guides
                && !record.editor_polish.highlight_active_indent_guide
        })?;
        assert_eq!(state.viewport.guide_count, 0);
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn enclosing_brackets_are_decorated_and_the_jump_command_uses_the_same_pairs() -> TestResult {
    support::run_x11_test("polish-bracket-match", |session| {
        session.seed_settings(
            "version = 1\n[editor]\nbracket_pair_guides = 'active'\nbracket_pair_horizontal_guides = 'active'\nindent_guides = true\nhighlight_active_indent_guide = true\n",
        )?;
        let path = session.seed_file("brackets.rs", "fn main() {\n    let value = (1 + 2);\n}\n")?;
        let mut editor = session.open_file("polish-bracket-match", &path)?;

        editor.click_at_text(1, 19)?;
        let decorated = editor.wait_state("enclosing bracket decoration", secs(2), |record| {
            record.viewport.structural_pair_count >= 3
                && record.viewport.bracket_matches.len() == 2
                && record.viewport.guide_count > 0
        })?;
        assert_eq!(decorated.editor_polish.match_brackets, "always");

        editor.click_at_text(1, 16)?;
        editor.keys("<C-S-\\>")?;
        editor.expect_cursor_heads(&[(1, 22)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn injected_brackets_stop_matching_after_selection_is_quoted() -> TestResult {
    support::run_x11_test("polish-injected-bracket-edit", |session| {
        let path = session.seed_file("injected.md", "```rust\nfn fenced() { let value = (1 + 2); }\n```\n")?;
        let mut editor = session.open_file("polish-injected-bracket-edit", &path)?;

        let initial = editor.wait_state("injected brackets parsed", secs(5), |record| {
            record.viewport.structural_pair_count == 3
        })?;
        assert_eq!(initial.viewport.structural_pair_count, 3);

        editor.click_at_text(1, 26)?;
        editor.keys("<S-right><S-right><S-right><S-right><S-right><S-right><S-right>\"")?;
        editor.wait_state("quoted injected brackets excluded", secs(5), |record| {
            record.viewport.structural_pair_count == 2
        })?;
        editor.save_then_expect_file(&path, "```rust\nfn fenced() { let value = \"(1 + 2)\"; }\n```\n")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn configured_guides_rulers_whitespace_and_control_markers_reach_the_real_viewport() -> TestResult {
    support::run_x11_test("polish-viewport-markers", |session| {
        session.seed_settings(
            "version = 1\n[editor]\nbracket_pair_guides = 'all'\nbracket_pair_horizontal_guides = 'all'\nrender_whitespace = 'all'\nrender_control_characters = true\nrulers = [4, 8]\n",
        )?;
        let path = session.seed_file("markers.rs", "fn main() {\n\tlet value = 1;  \u{1}\n}\n")?;
        let mut editor = session.open_file("polish-viewport-markers", &path)?;

        let painted = editor.wait_state("polish markers painted", secs(5), |record| {
            record.editor_polish.rulers == [4, 8]
                && record.viewport.guide_count > 0
                && record.viewport.whitespace_marker_count >= 4
                && record.viewport.control_marker_count >= 1
        })?;
        assert_eq!(painted.editor_polish.render_whitespace, "all");
        assert!(painted.editor_polish.render_control_characters);
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

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn explicit_selection_highlights_exact_text_but_not_the_selections_themselves() -> TestResult {
    support::run_x11_test("chrome-selection-match-highlights", |session| {
        let path = session.seed_file("selection-match-highlights.txt", "alpha alphabet alpha\nalpha beta\n")?;
        let mut editor = session.open_file("chrome-selection-match-highlights", &path)?;
        editor.place_cursor_at_document_start()?;

        editor.keys("<S-right><S-right><S-right><S-right><S-right>")?;
        editor.wait_state("selected text matches exclude active selection", secs(5), |record| {
            record.viewport.occurrence_highlights.is_empty()
                && record
                    .viewport
                    .selection_match_highlights
                    .iter()
                    .map(|range| (range.start, range.end))
                    .collect::<Vec<_>>()
                    == vec![(6, 11), (15, 20), (21, 26)]
        })?;

        editor.keys("<C-f>")?;
        editor.wait_state(
            "find suppresses duplicate selected-text decoration",
            secs(2),
            |record| {
                record.focused_input == "find_query"
                    && record.find.query == "alpha"
                    && record.find.match_count == 4
                    && record.viewport.selection_match_highlights.is_empty()
            },
        )?;
        editor.keys("<esc>")?;
        editor.wait_state("closing find restores selected-text decoration", secs(2), |record| {
            record.focused_input == "editor" && record.viewport.selection_match_highlights.len() == 3
        })?;

        editor.keys("<S-left>")?;
        editor.wait_state("partial selection matches substrings", secs(2), |record| {
            record
                .viewport
                .selection_match_highlights
                .iter()
                .map(|range| (range.start, range.end))
                .collect::<Vec<_>>()
                == vec![(6, 10), (15, 19), (21, 25)]
        })?;

        editor.keys("<home><right><right><right><right><right><S-right>")?;
        editor.wait_state("whitespace-only selection has no text matches", secs(2), |record| {
            record.cursors[0].anchor_col == 5
                && record.cursors[0].head_col == 6
                && record.viewport.selection_match_highlights.is_empty()
        })?;

        editor.keys("<home><S-down>")?;
        editor.wait_state("multiline selection has no text matches", secs(2), |record| {
            record.cursors[0].anchor_line == 0
                && record.cursors[0].head_line == 1
                && record.viewport.selection_match_highlights.is_empty()
        })?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn selection_match_limit_and_multi_cursor_agreement_are_explicit() -> TestResult {
    support::run_x11_test("chrome-selection-match-boundaries", |session| {
        let long = "x".repeat(201);
        let path = session.seed_file(
            "selection-match-boundaries.txt",
            &format!("{long}\n{long}\nfoo foo\nbar foo\n"),
        )?;
        let mut editor = session.open_file("chrome-selection-match-boundaries", &path)?;
        editor.place_cursor_at_document_start()?;

        editor.keys("<S-end>")?;
        editor.wait_state("selection over match limit is ignored", secs(5), |record| {
            record.cursors[0].head_col == 201 && record.viewport.selection_match_highlights.is_empty()
        })?;

        editor.keys("<S-left>")?;
        editor.wait_state("selection at match limit is highlighted", secs(2), |record| {
            record.cursors[0].head_col == 200
                && record
                    .viewport
                    .selection_match_highlights
                    .iter()
                    .any(|range| range.start == 202 && range.end == 402)
        })?;

        editor.keys("<C-g>3:1<enter><S-right><S-right><S-right><C-A-down>")?;
        editor.wait_state("different selected texts disable shared matches", secs(2), |record| {
            record.cursors.len() == 2
                && record.cursors[0].anchor_line == 2
                && record.cursors[1].anchor_line == 3
                && record.viewport.selection_match_highlights.is_empty()
        })?;
        Ok(())
    })
}
