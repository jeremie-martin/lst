//! Text geometry and syntax as displayed by the production application.
mod support;

use lst_x11_harness::{clipboard::write_clipboard_text, Selection, StateTraceRecord};
use std::time::{Duration, Instant};
use support::{secs, EditorTestExt, TestResult};

fn wrap_columns(state: &StateTraceRecord) -> Option<usize> {
    state
        .status_bar
        .split("Wrap ")
        .nth(1)?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn held_document_drag_keeps_its_speed_after_input_activity_and_stops_on_release() -> TestResult {
    support::run_x11_test("presentation-drag-speed", |session| {
        session.seed_settings("version = 1\n[editor]\ncursor_blink = false\n")?;
        let path = session.seed_file("drag.txt", &"long document line\n".repeat(3000))?;
        let mut editor = session.open_file("presentation-drag-speed", &path)?;
        editor.resize(900, 600)?;
        editor.keys("<C-home>")?;
        let state = editor.expect_cursor_heads(&[(0, 0)])?;
        let view = &state.viewport;
        let from = view.text_to_window_local(0, 0).ok_or("first row missing")?;
        let (_, top) = view.bounds_origin_px.ok_or("viewport origin missing")?;
        let (_, height) = view.bounds_size_px.ok_or("viewport size missing")?;
        let to = (from.0, ((top + height - 2.0) * view.scale_factor).round() as i32);
        editor.drag_at(from, to, |editor| {
            let mut rates = Vec::new();
            for pause in [200, 700] {
                std::thread::sleep(Duration::from_millis(pause));
                let before = editor.read_state()?.viewport.scroll_top_px;
                let started = Instant::now();
                std::thread::sleep(Duration::from_millis(600));
                let after = editor.read_state()?.viewport.scroll_top_px;
                rates.push((after - before) / started.elapsed().as_secs_f32());
            }
            assert!(
                rates.iter().all(|rate| *rate > 500.0),
                "drag must keep advancing: {rates:?}"
            );
            assert!(
                (0.7..1.3).contains(&(rates[1] / rates[0])),
                "post-input refresh must not change drag speed: {rates:?}"
            );
            Ok(())
        })?;
        editor.wait_quiet(Duration::from_millis(100), secs(5))?;
        let released = editor.read_state()?;
        std::thread::sleep(Duration::from_millis(300));
        let stopped = editor.read_state()?;
        assert_eq!(released.viewport.scroll_top_px, stopped.viewport.scroll_top_px);
        assert_eq!(released.cursors[0].head_char, stopped.cursors[0].head_char);
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn clicks_choose_nearest_grapheme_boundaries_in_cached_and_shaped_text() -> TestResult {
    support::run_x11_test("presentation-hit-testing", |session| {
        let path = session.seed_file("hit.txt", "ab\néx\ne\u{301}x\nz")?;
        let mut editor = session.open_file("presentation-hit-testing", &path)?;
        for (line, cell_x, expected) in [
            (0, 0.2, 0),
            (0, 0.8, 1),
            (0, 1.8, 2),
            (1, 0.8, 1),
            (1, 1.8, 2),
            (2, 0.2, 0),
            (2, 0.8, 2),
            (2, 1.8, 3),
        ] {
            let state = editor.read_state()?;
            let viewport = &state.viewport;
            let row = viewport.first_row_for_line(line).ok_or("text row missing")?;
            editor.click_at(
                ((viewport.code_origin_x_px + cell_x * viewport.char_width_px) * viewport.scale_factor).round() as i32,
                ((row.top_px + viewport.line_height_px * 0.5) * viewport.scale_factor).round() as i32,
            )?;
            editor.expect_cursor_heads(&[(line, expected)])?;
            // Separate clicks must not become a double-click selection.
            std::thread::sleep(Duration::from_millis(510));
        }
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn status_wrap_columns_follow_resize_without_another_input() -> TestResult {
    support::run_x11_test("presentation-wrap-status", |session| {
        session.seed_settings("version = 1\n[editor]\ncursor_blink = false\n")?;
        let path = session.seed_file("width.txt", &"x".repeat(3000))?;
        let mut editor = session.open_file("presentation-wrap-status", &path)?;
        editor.keys("<C-home>")?;
        for width in [900, 640, 1000] {
            editor.resize(width, 600)?;
            editor.wait_state("rendered status matches the current wrapped row", secs(5), |state| {
                let Some(first) = state.viewport.first_row_for_line(0) else {
                    return false;
                };
                first.line_start_char == 0 && wrap_columns(state) == Some(first.display_end_char)
            })?;
        }
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn wide_graphemes_wrap_and_navigate_by_the_same_display_rows() -> TestResult {
    support::run_x11_test("presentation-wide-wrap", |session| {
        session.seed_settings("version = 1\n[editor]\nfont_family = 'JetBrains Mono'\ncursor_blink = false\n")?;
        let path = session.seed_file("wide.txt", &"界".repeat(300))?;
        let screenshot = session.artifacts().join("wide-wrap.ppm");
        let mut editor = session.open_file("presentation-wide-wrap", &path)?;
        editor.resize(640, 480)?;
        editor.keys("<C-home>")?;
        let state = editor.wait_state("wide text wraps at its cell width", secs(5), |state| {
            state
                .viewport
                .first_row_for_line(0)
                .zip(wrap_columns(state))
                .is_some_and(|(row, cols)| row.line_start_char == 0 && row.display_end_char == cols / 2)
        })?;
        let first_end = state.viewport.first_row_for_line(0).unwrap().display_end_char;
        editor.keys("<down>")?;
        editor.expect_cursor_heads(&[(0, first_end)])?;
        editor.keys("<up>")?;
        editor.expect_cursor_heads(&[(0, 0)])?;
        editor.screenshot()?.write_ppm(screenshot)?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn returning_to_a_tab_reveals_its_caret_after_other_tab_resize_and_zoom() -> TestResult {
    support::run_x11_test("presentation-tab-reveal", |session| {
        session.seed_settings("version = 1\n[editor]\ncursor_blink = false\n")?;
        let a = session.seed_file("long.txt", &"abcdefghij ".repeat(3000))?;
        let b = session.seed_file("other.txt", "other")?;
        let screenshot = session.artifacts().join("tab-reveal.ppm");
        let mut editor = session.open_files("presentation-tab-reveal", &[a, b])?;
        editor.resize(1000, 700)?;
        editor.keys("<C-end><C-tab>")?;
        editor.wait_state("second tab active", secs(5), |state| state.active_tab_index == 1)?;
        editor.resize(640, 480)?;
        editor.keys("<C-=><C-=><C-=><C-S-tab>")?;
        editor.wait_state("returned caret is inside the new viewport", secs(10), |state| {
            if state.active_tab_index != 0 {
                return false;
            }
            let Some(cursor) = state.cursors.first() else {
                return false;
            };
            let Some((_, top)) = state.viewport.bounds_origin_px else {
                return false;
            };
            let Some((_, height)) = state.viewport.bounds_size_px else {
                return false;
            };
            state.viewport.rows.iter().any(|row| {
                row.line_start_char <= cursor.head_char
                    && cursor.head_char <= row.display_end_char
                    && row.top_px >= top - 0.5
                    && row.top_px + state.viewport.line_height_px <= top + height + 0.5
            })
        })?;
        editor.screenshot()?.write_ppm(screenshot)?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn injected_comment_recolors_distant_text_and_undo_restores_its_pixels() -> TestResult {
    support::run_x11_test("presentation-injected-colors", |session| {
        session.seed_settings("version = 1\n[editor]\ncursor_blink = false\n")?;
        let source = "```rust\nfn f() {\n    let a = 1;\n    let b = 2;\n    let c = 3;\n    let d = 4; */\n}\n```\n";
        let path = session.seed_file("colors.md", source)?;
        let screenshot = session.artifacts().join("injected-colors.ppm");
        let mut editor = session.open_file("presentation-injected-colors", &path)?;
        editor.keys("<C-home><down><down>")?;
        let state = editor.expect_cursor_heads(&[(2, 0)])?;
        let view = &state.viewport;
        let row = view.first_row_for_line(4).ok_or("target row missing")?;
        let x = (view.code_origin_x_px * view.scale_factor).round() as usize;
        let y = (row.top_px * view.scale_factor).round() as usize;
        let width = (18.0 * view.char_width_px * view.scale_factor).floor() as usize;
        let height = (view.line_height_px * view.scale_factor).floor() as usize;
        let crop = |shot: &lst_x11_harness::Screenshot| {
            (y..y + height)
                .flat_map(|row| {
                    let start = (row * usize::from(shot.width) + x) * 3;
                    shot.rgb_pixels[start..start + width * 3].iter().copied()
                })
                .collect::<Vec<_>>()
        };
        let before = crop(&editor.screenshot()?);
        write_clipboard_text(Selection::Clipboard, "/* ")?;
        editor.keys("<C-v>")?;
        editor.expect_cursor_heads(&[(2, 3)])?;
        let during = editor.screenshot()?;
        during.write_ppm(screenshot)?;
        assert!(before != crop(&during), "distant code must acquire comment colors");
        editor.keys("<C-z>")?;
        editor.expect_cursor_heads(&[(2, 0)])?;
        assert!(
            before == crop(&editor.screenshot()?),
            "undo restores the original colors"
        );
        editor.save_then_expect_file(&path, source)?;
        Ok(())
    })
}
