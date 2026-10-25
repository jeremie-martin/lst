//! Real-input contracts for the shared search and replacement fields.

mod support;

use lst_x11_harness::{
    clipboard::{wait_clipboard_text, write_clipboard_text},
    ChordMods, Editor, Key, KeyChord, Selection, StateTraceRecord,
};
use support::{find_input, secs, EditorTestExt, FindChip, FindInput, SupportResult, TestResult};

fn expect_query(editor: &mut Editor<'_>, expected: &str) -> SupportResult<StateTraceRecord> {
    editor.wait_state("search field value", secs(5), |record| {
        record.find.visible && record.find.query == expected
    })
}

fn expect_replacement(editor: &mut Editor<'_>, expected: &str) -> SupportResult<StateTraceRecord> {
    editor.wait_state("replacement field value", secs(5), |record| {
        record.find.visible && record.find.show_replace && record.find.replacement == expected
    })
}

fn set_query(editor: &mut Editor<'_>, text: &str) -> SupportResult<StateTraceRecord> {
    editor.keys("<C-f><C-a>")?;
    write_clipboard_text(Selection::Clipboard, text)?;
    editor.keys("<C-v>")?;
    expect_query(editor, text)
}

fn expect_focus(editor: &mut Editor<'_>, focus: &str) -> SupportResult<StateTraceRecord> {
    editor.wait_state("find field focus", secs(5), |record| record.focused_input == focus)
}

fn geometry(editor: &mut Editor<'_>, input: FindInput) -> SupportResult<StateTraceRecord> {
    editor.wait_state("rendered find field geometry", secs(5), |record| {
        find_input(record, input).is_some_and(|field| field.bounds_px.is_some() && field.text_bounds_px.is_some())
    })
}

fn caret_point(editor: &mut Editor<'_>, input: FindInput, offset: usize) -> SupportResult<(i32, i32)> {
    let record = editor.wait_state("find field caret geometry", secs(5), |record| {
        find_input(record, input).is_some_and(|field| {
            field.selection.start == offset && field.selection.end == offset && field.caret_bounds_px.is_some()
        })
    })?;
    let field = find_input(&record, input).ok_or("field disappeared after caret wait")?;
    let (x, y, _, height) = field.caret_bounds_px.ok_or("caret geometry missing after wait")?;
    Ok(window_point(&record, x, y + height * 0.5))
}

fn window_point(record: &StateTraceRecord, x: f32, y: f32) -> (i32, i32) {
    let scale = if record.viewport.scale_factor > 0.0 {
        record.viewport.scale_factor
    } else {
        1.0
    };
    ((x * scale).round() as i32, (y * scale).round() as i32)
}

fn expect_selection(
    editor: &mut Editor<'_>,
    input: FindInput,
    start: usize,
    end: usize,
) -> SupportResult<StateTraceRecord> {
    editor.wait_state("find field selection", secs(5), |record| {
        find_input(record, input).is_some_and(|field| field.selection.start == start && field.selection.end == end)
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn mouse_switches_search_and_replacement_without_overriding_input_ownership() -> TestResult {
    support::run_x11_test("find-mouse-field-switch", |session| {
        let original = "alpha alpha";
        let path = session.seed_file("mouse-field-switch.txt", original)?;
        let mut editor = session.open_file("find-mouse-field-switch", &path)?;
        editor.keys("<C-h>alpha")?;
        expect_query(&mut editor, "alpha")?;

        // The compatibility helper permits the same behavioral repro against a
        // preserved app whose trace predates field geometry.
        editor.click_find_input_baseline_compatible(FindInput::Replace)?;
        expect_focus(&mut editor, "find_replace")?;
        editor.keys("<C-a>beta<C-a><C-c>")?;
        wait_clipboard_text(Selection::Clipboard, "beta", secs(5))?;
        expect_replacement(&mut editor, "beta")?;

        editor.click_find_input_baseline_compatible(FindInput::Query)?;
        expect_focus(&mut editor, "find_query")?;
        editor.keys("<C-a>gamma")?;
        expect_query(&mut editor, "gamma")?;
        editor.click_find_chip(FindChip::CaseSensitive)?;
        editor.wait_state("case chip still clickable after field switching", secs(5), |record| {
            record.find.case_sensitive
        })?;
        editor.keys("<end>!")?;
        expect_query(&mut editor, "gamma!")?;
        editor.save_then_expect_file(&path, original)?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn newer_field_clicks_supersede_older_focus_requests_in_an_input_burst() -> TestResult {
    support::run_x11_test("find-input-burst-focus", |session| {
        let original = "alpha beta gamma\nhello world\n";
        let path = session.seed_file("input-burst-focus.txt", original)?;
        let mut editor = session.open_file("find-input-burst-focus", &path)?;
        editor.keys("<C-h>alpha")?;
        let record = geometry(&mut editor, FindInput::Query)?;
        let query_bounds = find_input(&record, FindInput::Query)
            .and_then(|field| field.bounds_px)
            .ok_or("query bounds missing")?;
        let replace_bounds = find_input(&record, FindInput::Replace)
            .and_then(|field| field.bounds_px)
            .ok_or("replacement bounds missing")?;
        let query = window_point(
            &record,
            query_bounds.0 + query_bounds.2 * 0.5,
            query_bounds.1 + query_bounds.3 * 0.5,
        );
        let replace = window_point(
            &record,
            replace_bounds.0 + replace_bounds.2 * 0.5,
            replace_bounds.1 + replace_bounds.3 * 0.5,
        );
        let document = record
            .viewport
            .text_to_window_local(0, 1)
            .ok_or("first document row missing")?;
        let before = record.seq;
        editor.input_burst_at(None, &[document, query])?;
        editor.wait_state("newer search click wins within the input burst", secs(5), |record| {
            record.seq > before
                && record.focused_input == "find_query"
                && matches!(record.cursors.as_slice(), [cursor] if cursor.head_line == 0 && cursor.head_col > 0)
        })?;
        editor.keys("q")?;
        expect_query(&mut editor, "alphaq")?;

        let record = editor.read_state()?;
        let document = record
            .viewport
            .text_to_window_local(1, 2)
            .ok_or("second document row missing")?;
        let before = record.seq;
        editor.input_burst_at(None, &[document, replace])?;
        editor.wait_state(
            "newer replacement click wins within the input burst",
            secs(5),
            |record| {
                record.seq > before
                    && record.focused_input == "find_replace"
                    && matches!(record.cursors.as_slice(), [cursor] if cursor.head_line == 1)
            },
        )?;
        editor.keys("r")?;
        expect_replacement(&mut editor, "r")?;

        // A refocus shortcut and a later click are also queued together: the
        // later pointer intent must win, not the older Ctrl+F request. A fresh
        // pointer point keeps this click distinct from the preceding gesture.
        let refocus_click = window_point(
            &record,
            replace_bounds.0 + replace_bounds.2 * 0.75,
            replace_bounds.1 + replace_bounds.3 * 0.5,
        );
        editor.input_burst_at(Some(KeyChord::Ctrl(Key::Char('f'))), &[refocus_click])?;
        editor.keys("s")?;
        editor.wait_state("mouse supersedes the earlier refocus shortcut", secs(5), |record| {
            record.focused_input == "find_replace"
                && record.find.replacement == "rs"
                && record.find.query == "alphaq"
                && !record.active_tab_modified
        })?;
        editor.save_then_expect_file(&path, original)?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn find_panel_padding_occludes_the_editor_scrollbar() -> TestResult {
    support::run_x11_test("find-occludes-scrollbar", |session| {
        let original = format!("needle\n{}", "body\n".repeat(500));
        let path = session.seed_file("find-scrollbar-occlusion.txt", &original)?;
        let mut editor = session.open_file("find-scrollbar-occlusion", &path)?;
        editor.keys("<C-f>needle")?;
        let record = editor.wait_state("find over a scrollable document", secs(5), |record| {
            record.find.query == "needle"
                && record.find.close_bounds_px.is_some()
                && record.viewport.bounds_origin_px.is_some()
                && record.viewport.bounds_size_px.is_some()
        })?;
        let (left, _) = record.viewport.bounds_origin_px.ok_or("viewport origin missing")?;
        let (width, _) = record.viewport.bounds_size_px.ok_or("viewport size missing")?;
        let (_, close_top, _, close_height) = record.find.close_bounds_px.ok_or("find close bounds missing")?;
        let overlap = window_point(&record, left + width - 9.0, close_top + close_height * 0.5);
        let scroll_top = record.viewport.scroll_top_px;
        // The editor's 10px scrollbar and find's 8px right inset overlap here.
        // The visible panel border/padding, not the covered track, owns input.
        editor.click_at(overlap.0, overlap.1)?;
        editor.keys("<end>q")?;
        editor.wait_state(
            "find padding keeps field ownership and editor scroll position",
            secs(5),
            |record| {
                record.find.query == "needleq"
                    && record.focused_input == "find_query"
                    && record.viewport.scroll_top_px == scroll_top
                    && !record.active_tab_modified
            },
        )?;
        editor.save_then_expect_file(&path, &original)?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn collapsing_replacement_rehomes_only_a_focus_owner_that_becomes_hidden() -> TestResult {
    support::run_x11_test("find-collapse-focus", |session| {
        let path = session.seed_file("collapse-focus.txt", "alpha alpha")?;
        let mut editor = session.open_file("find-collapse-focus", &path)?;
        editor.keys("<C-h>alpha<tab>beta")?;
        expect_replacement(&mut editor, "beta")?;
        expect_focus(&mut editor, "find_replace")?;
        editor.click_find_disclosure()?;
        editor.wait_state("collapsed replacement hands focus to search", secs(5), |record| {
            record.find.visible && !record.find.show_replace && record.focused_input == "find_query"
        })?;
        editor.keys("<end>q")?;
        expect_query(&mut editor, "alphaq")?;
        editor.keys("<C-f><C-a>gamma")?;
        expect_query(&mut editor, "gamma")?;

        editor.click_find_disclosure()?;
        editor.wait_state("replacement expanded again", secs(5), |record| record.find.show_replace)?;
        editor.click_at_text(0, 0)?;
        expect_focus(&mut editor, "editor")?;
        editor.click_find_disclosure()?;
        editor.wait_state("hiding an unfocused field keeps editor focus", secs(5), |record| {
            !record.find.show_replace && record.focused_input == "editor"
        })?;
        editor.keys("<end>X")?;
        editor.save_then_expect_file(&path, "alpha alphaX")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn find_traversal_has_no_dead_ends_and_shift_enter_navigates_backward() -> TestResult {
    support::run_x11_test("find-traversal-and-reverse-submit", |session| {
        let path = session.seed_file("traversal.txt", "foo foo foo")?;
        let mut editor = session.open_file("find-traversal", &path)?;
        editor.keys("<C-h>foo")?;
        editor.expect_cursor_heads(&[(0, 0)])?;
        editor.keys("<S-enter>")?;
        editor.expect_cursor_heads(&[(0, 8)])?;
        editor.keys("<enter><tab>")?;
        expect_focus(&mut editor, "find_replace")?;
        editor.expect_cursor_heads(&[(0, 0)])?;
        editor.keys("<S-enter>")?;
        editor.expect_cursor_heads(&[(0, 8)])?;
        editor.keys("<S-tab>")?;
        expect_focus(&mut editor, "find_query")?;
        editor.keys("<S-tab>")?;
        expect_focus(&mut editor, "editor")?;
        editor.keys("<C-f><tab>")?;
        expect_focus(&mut editor, "find_replace")?;
        editor.keys("<tab>")?;
        expect_focus(&mut editor, "editor")?;

        editor.keys("<C-f><esc><C-f>")?;
        editor.wait_state("find-only query focused", secs(5), |record| {
            record.find.visible && !record.find.show_replace && record.focused_input == "find_query"
        })?;
        editor.keys("<tab>")?;
        expect_focus(&mut editor, "editor")?;
        editor.keys("<C-f><S-tab>")?;
        expect_focus(&mut editor, "editor")?;
        editor.save_then_expect_file(&path, "foo foo foo")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn standard_boundary_shortcuts_and_redo_use_local_field_history() -> TestResult {
    support::run_x11_test("find-standard-shortcuts", |session| {
        let path = session.seed_file("field-history.txt", "document")?;
        let mut editor = session.open_file("find-standard-shortcuts", &path)?;
        editor.keys("<end>!<C-h>abc")?;
        expect_query(&mut editor, "abc")?;
        editor.keys("<C-home>X")?;
        expect_query(&mut editor, "Xabc")?;
        editor.keys("<C-end>Y")?;
        expect_query(&mut editor, "XabcY")?;
        editor.keys("<C-S-home>Z")?;
        expect_query(&mut editor, "Z")?;
        editor.keys("<C-z>")?;
        expect_query(&mut editor, "XabcY")?;
        editor.keys("<C-y>")?;
        expect_query(&mut editor, "Z")?;
        editor.keys("<C-z>")?;
        expect_query(&mut editor, "XabcY")?;
        editor.keys("<C-S-z>")?;
        expect_query(&mut editor, "Z")?;
        editor.keys("<home><C-S-end>W")?;
        expect_query(&mut editor, "W")?;

        editor.keys("<tab>uvw<home><C-end>!")?;
        expect_replacement(&mut editor, "uvw!")?;
        editor.keys("<C-S-home>R<C-z>")?;
        expect_replacement(&mut editor, "uvw!")?;
        editor.keys("<C-y>")?;
        expect_replacement(&mut editor, "R")?;
        editor.keys("<C-a><C-x>")?;
        expect_replacement(&mut editor, "")?;
        wait_clipboard_text(Selection::Clipboard, "R", secs(5))?;
        editor.keys("<C-z>")?;
        expect_replacement(&mut editor, "R")?;
        write_clipboard_text(Selection::Clipboard, "paste")?;
        editor.keys("<C-a><C-v>")?;
        expect_replacement(&mut editor, "paste")?;
        editor.keys("<C-z>")?;
        expect_replacement(&mut editor, "R")?;

        editor.keys("<esc>")?;
        editor.save_then_expect_file(&path, "document!")?;
        editor.keys("<C-z>")?;
        editor.save_then_expect_file(&path, "document")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn pasted_line_breaks_are_uniformly_single_line_in_search_and_replacement() -> TestResult {
    support::run_x11_test("find-single-line-paste", |session| {
        let path = session.seed_file("single-line-paste.txt", "needle")?;
        let mut editor = session.open_file("find-single-line-paste", &path)?;
        let pasted = "a\r\nb\rc\nd\u{b}e\u{c}f\u{85}g\u{2028}h\u{2029}i";
        let flattened = "a b c d e f g h i";
        editor.keys("<C-h>")?;
        write_clipboard_text(Selection::Clipboard, pasted)?;
        editor.keys("<C-v>")?;
        expect_query(&mut editor, flattened)?;
        editor.keys("<C-a><C-c>")?;
        wait_clipboard_text(Selection::Clipboard, flattened, secs(5))?;
        editor.keys("needle<tab>")?;
        expect_query(&mut editor, "needle")?;
        write_clipboard_text(Selection::Clipboard, pasted)?;
        editor.keys("<C-v>")?;
        expect_replacement(&mut editor, flattened)?;
        editor.keys("<enter>")?;
        editor.save_then_expect_file(&path, flattened)?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn multiline_selections_never_seed_hidden_line_breaks_into_search() -> TestResult {
    support::run_x11_test("find-single-line-prefill", |session| {
        for (index, separator) in ["\r", "\r\n", "\n", "\u{b}", "\u{c}", "\u{85}", "\u{2028}", "\u{2029}"]
            .iter()
            .enumerate()
        {
            let path = session.seed_file(&format!("prefill-{index}.txt"), &format!("needle{separator}other"))?;
            let mut editor = session.open_file(&format!("find-prefill-{index}"), &path)?;
            editor.keys("<C-f>needle<esc><C-home><C-S-end><C-f>")?;
            expect_query(&mut editor, "needle")?;
            write_clipboard_text(Selection::Clipboard, "before prefill copy")?;
            editor.keys("<C-a><C-c>")?;
            wait_clipboard_text(Selection::Clipboard, "needle", secs(5))?;
        }
        let path = session.seed_file("single-line-prefill.txt", "needle other")?;
        let mut editor = session.open_file("find-single-line-prefill", &path)?;
        editor.keys("<end><C-S-left><C-f>")?;
        expect_query(&mut editor, "other")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn committed_grapheme_joins_cannot_be_partially_deleted_in_a_field() -> TestResult {
    support::run_x11_test("find-committed-grapheme-joins", |session| {
        let path = session.seed_file("grapheme-joins.txt", "unchanged")?;
        let mut editor = session.open_file("find-grapheme-joins", &path)?;
        set_query(&mut editor, "👩💻")?;
        editor.keys("<home><right>")?;
        write_clipboard_text(Selection::Clipboard, "\u{200d}")?;
        editor.keys("<C-v>")?;
        expect_query(&mut editor, "👩‍💻")?;
        editor.keys("<bs>")?;
        expect_query(&mut editor, "")?;

        set_query(&mut editor, "🇦🇧🇨")?;
        editor.keys("<home><right>")?;
        write_clipboard_text(Selection::Clipboard, "🇩")?;
        editor.keys("<C-v>")?;
        expect_query(&mut editor, "🇦🇧🇩🇨")?;
        editor.keys("<bs>")?;
        expect_query(&mut editor, "🇦🇧")?;

        set_query(&mut editor, "e\u{301}x")?;
        editor.keys("<home><right><bs>")?;
        expect_query(&mut editor, "x")?;
        editor.save_then_expect_file(&path, "unchanged")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn field_padding_uses_horizontal_caret_placement_and_click_selection_remains_standard() -> TestResult {
    support::run_x11_test("find-padding-and-click-selection", |session| {
        let text = "alpha beta gamma";
        let path = session.seed_file("field-padding.txt", text)?;
        let mut editor = session.open_file("find-padding", &path)?;
        set_query(&mut editor, text)?;
        editor.keys("<home><right><right><right><right><right><right>")?;
        let (x, _) = caret_point(&mut editor, FindInput::Query, 6)?;
        let record = geometry(&mut editor, FindInput::Query)?;
        let field = find_input(&record, FindInput::Query).ok_or("query geometry missing")?;
        let (_, top, _, height) = field.bounds_px.ok_or("query bounds missing")?;
        let (_, text_top, _, text_height) = field.text_bounds_px.ok_or("query text bounds missing")?;
        for y in [text_top + text_height * 0.5, top + 2.0, top + height - 2.0] {
            let (_, y) = window_point(&record, 0.0, y);
            editor.click_at(x, y)?;
            editor.keys("!")?;
            expect_query(&mut editor, "alpha !beta gamma")?;
            editor.keys("<C-z>")?;
            expect_query(&mut editor, text)?;
        }

        editor.keys("<home><right><right><right><right><right><right><right><right>")?;
        let target = caret_point(&mut editor, FindInput::Query, 8)?;
        editor.click_at_with_modifiers(target.0, target.1, ChordMods::default(), 2)?;
        expect_selection(&mut editor, FindInput::Query, 6, 10)?;
        editor.keys("<C-c>")?;
        wait_clipboard_text(Selection::Clipboard, "beta", secs(5))?;
        editor.click_at_with_modifiers(target.0, target.1, ChordMods::default(), 3)?;
        expect_selection(&mut editor, FindInput::Query, 0, text.len())?;
        editor.keys("<C-c>")?;
        wait_clipboard_text(Selection::Clipboard, text, secs(5))?;
        editor.keys("<home>")?;
        let start = caret_point(&mut editor, FindInput::Query, 0)?;
        // Move the click origin so this is a fresh Shift+click, not a fourth
        // click in the preceding word/all-selection gesture.
        editor.click_at(start.0, start.1)?;
        editor.click_at_with_modifiers(target.0, target.1, ChordMods::SHIFT, 1)?;
        expect_selection(&mut editor, FindInput::Query, 0, 8)?;
        editor.keys("<C-c>")?;
        wait_clipboard_text(Selection::Clipboard, "alpha be", secs(5))?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn selection_drag_continues_beyond_both_field_edges() -> TestResult {
    support::run_x11_test("find-drag-outside-edges", |session| {
        let text = "alpha beta gamma";
        let path = session.seed_file("field-drag.txt", text)?;
        let mut editor = session.open_file("find-drag-outside", &path)?;
        set_query(&mut editor, text)?;
        editor.keys("<home><right><right><right><right><right><right><right><right>")?;
        let from = caret_point(&mut editor, FindInput::Query, 8)?;
        let record = geometry(&mut editor, FindInput::Query)?;
        let (left, top, width, height) = find_input(&record, FindInput::Query)
            .and_then(|field| field.bounds_px)
            .ok_or("query bounds missing")?;
        let outside_left = window_point(&record, left - 24.0, top + height * 0.5);
        editor.drag_at(from, outside_left, |editor| {
            expect_selection(editor, FindInput::Query, 0, 8)
        })?;
        editor.keys("<C-c>")?;
        wait_clipboard_text(Selection::Clipboard, "alpha be", secs(5))?;

        editor.keys("<home><right><right><right><right><right><right>")?;
        let from = caret_point(&mut editor, FindInput::Query, 6)?;
        let outside_right = window_point(&record, left + width + 24.0, top + height * 0.5);
        editor.drag_at(from, outside_right, |editor| {
            expect_selection(editor, FindInput::Query, 6, text.len())
        })?;
        editor.keys("<C-c>")?;
        wait_clipboard_text(Selection::Clipboard, "beta gamma", secs(5))?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn long_field_drag_autoscrolls_while_held_and_stops_after_release_or_hide() -> TestResult {
    support::run_x11_test("find-drag-autoscroll-lifecycle", |session| {
        let path = session.seed_file("field-autoscroll.txt", "unchanged")?;
        let mut editor = session.open_file("find-drag-autoscroll", &path)?;
        let text = "0123456789".repeat(40);
        set_query(&mut editor, &text)?;
        editor.keys("<home>")?;
        let from = caret_point(&mut editor, FindInput::Query, 0)?;
        let record = geometry(&mut editor, FindInput::Query)?;
        let (left, top, width, height) = find_input(&record, FindInput::Query)
            .and_then(|field| field.bounds_px)
            .ok_or("query bounds missing")?;
        let outside_right = window_point(&record, left + width + 24.0, top + height * 0.5);
        let outside_left = window_point(&record, left - 24.0, top + height * 0.5);
        editor.drag_at(from, outside_right, |editor| {
            editor.wait_state("held drag reaches long query end", secs(10), |record| {
                find_input(record, FindInput::Query).is_some_and(|field| {
                    field.selection.start == 0 && field.selection.end == text.len() && field.scroll_left_px > 0.0
                })
            })
        })?;
        write_clipboard_text(Selection::Clipboard, "before autoscroll selection copy")?;
        editor.keys("<C-c>")?;
        wait_clipboard_text(Selection::Clipboard, &text, secs(5))?;

        editor.keys("<end>")?;
        let from = caret_point(&mut editor, FindInput::Query, text.len())?;
        editor.drag_at(from, outside_left, |editor| {
            editor.wait_state("held drag reaches long query start", secs(10), |record| {
                find_input(record, FindInput::Query).is_some_and(|field| {
                    field.selection.start == 0 && field.selection.end == text.len() && field.scroll_left_px == 0.0
                })
            })
        })?;
        write_clipboard_text(Selection::Clipboard, "before autoscroll selection copy")?;
        editor.keys("<C-c>")?;
        wait_clipboard_text(Selection::Clipboard, &text, secs(5))?;

        editor.keys("<home>")?;
        let from = caret_point(&mut editor, FindInput::Query, 0)?;
        editor.drag_at(from, outside_right, |editor| {
            editor.wait_state("partially autoscrolled selection before release", secs(10), |record| {
                find_input(record, FindInput::Query)
                    .is_some_and(|field| field.selection.end > 80 && field.selection.end < 300)
            })
        })?;
        editor.wait_quiet(std::time::Duration::from_millis(75), secs(5))?;
        let released = editor.read_state()?;
        let selection = find_input(&released, FindInput::Query)
            .ok_or("query missing after release")?
            .selection;
        if selection.start != 0 || selection.end >= text.len() || selection.end <= 80 {
            return Err(format!("released drag did not stop at a partial selection: {released:?}").into());
        }
        editor.move_pointer_at(outside_left.0, outside_left.1)?;
        editor.keys("<C-c>")?;
        wait_clipboard_text(Selection::Clipboard, &text[..selection.end], secs(5))?;

        editor.keys("<home>")?;
        let from = caret_point(&mut editor, FindInput::Query, 0)?;
        editor.drag_at(from, outside_right, |editor| {
            editor.wait_state("autoscroll active before hiding find", secs(10), |record| {
                find_input(record, FindInput::Query)
                    .is_some_and(|field| field.selection.end > 60 && field.selection.end < text.len())
            })?;
            editor.keys("<esc>")?;
            editor.wait_state("find hidden during drag", secs(5), |record| {
                !record.find.visible && record.focused_input == "editor"
            })
        })?;
        editor.keys("<C-f>")?;
        expect_focus(&mut editor, "find_query")?;
        editor.click_find_chip(FindChip::CaseSensitive)?;
        editor.wait_state(
            "reopened find has no stale drag consuming chip clicks",
            secs(5),
            |record| record.find.case_sensitive,
        )?;
        editor.save_then_expect_file(&path, "unchanged")?;
        Ok(())
    })
}
