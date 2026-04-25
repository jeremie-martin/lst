use crate::ui::{
    input_keybindings,
    scrollbar::{vertical_scrollbar_layout, VerticalScrollbarLayout},
    theme::syntax as theme_syntax,
};
use gpui::{
    point, px, Bounds, ClipboardItem, Entity, EntityInputHandler, Keystroke, Modifiers,
    MouseButton, TestAppContext, VisualContext as _, VisualTestContext,
};
#[cfg(feature = "internal-invariants")]
use lst_editor::{EditorModel, EditorTab, TabId};
use std::{
    process,
    sync::atomic::{AtomicUsize, Ordering},
};

#[cfg(feature = "internal-invariants")]
use crate::syntax::SyntaxHighlightJobKey;
use crate::syntax::{
    compute_syntax_highlights, syntax_mode_for_language, SyntaxLanguage, SyntaxMode,
};
#[cfg(feature = "internal-invariants")]
use crate::viewport::PaintedRow;
use crate::*;

static NEXT_TEST_DIR: AtomicUsize = AtomicUsize::new(0);

fn has_binding<A: gpui::Action + 'static>(keystroke: &str) -> bool {
    let typed = [Keystroke::parse(keystroke).expect("valid test keystroke")];
    editor_keybindings().iter().any(|binding| {
        binding.match_keystrokes(&typed) == Some(false) && binding.action().as_any().is::<A>()
    })
}

fn has_binding_in_context<A: gpui::Action + 'static>(keystroke: &str, context: &str) -> bool {
    let typed = [Keystroke::parse(keystroke).expect("valid test keystroke")];
    editor_keybindings().iter().any(|binding| {
        binding.match_keystrokes(&typed) == Some(false)
            && binding.action().as_any().is::<A>()
            && binding
                .predicate()
                .as_ref()
                .map(ToString::to_string)
                .as_deref()
                == Some(context)
    })
}

fn has_binding_context_containing<A: gpui::Action + 'static>(
    keystroke: &str,
    expected: &[&str],
) -> bool {
    let typed = [Keystroke::parse(keystroke).expect("valid test keystroke")];
    editor_keybindings().iter().any(|binding| {
        binding.match_keystrokes(&typed) == Some(false)
            && binding.action().as_any().is::<A>()
            && binding
                .predicate()
                .as_ref()
                .map(ToString::to_string)
                .is_some_and(|predicate| expected.iter().all(|part| predicate.contains(part)))
    })
}

fn temp_dir(label: &str) -> PathBuf {
    let id = NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("lst-gpui-app-tests-{label}-{}-{id}", process::id()));
    std::fs::create_dir(&dir).expect("create test temp dir");
    dir
}

fn new_test_app(
    cx: &mut TestAppContext,
    mut launch: LaunchArgs,
) -> (Entity<LstGpuiApp>, &mut VisualTestContext) {
    if launch.files.is_empty() && launch.scratchpad_dir.is_none() {
        launch.scratchpad_dir = Some(temp_dir("scratchpads"));
    }
    cx.update(|cx| {
        cx.bind_keys(editor_keybindings());
        cx.bind_keys(input_keybindings());
    });
    let (view, cx) = cx.add_window_view(|_, cx| LstGpuiApp::new(cx, launch));
    cx.update(|window, cx| {
        window.focus(&view.read(cx).focus_handle);
        window.activate_window();
    });
    cx.run_until_parked();
    (view, cx)
}

fn app_snapshot(view: &Entity<LstGpuiApp>, cx: &mut VisualTestContext) -> AppSnapshot {
    view.update(cx, |app, cx| app.snapshot(cx))
}

fn assert_tab_views_match_model(snapshot: &AppSnapshot) {
    assert_eq!(snapshot.tab_view_ids, snapshot.model.tab_ids);
}

fn is_timestamped_scratchpad_name(name: &str) -> bool {
    if name.len() != "YYYY-MM-DD_HH-MM-SS.md".len() {
        return false;
    }
    name.char_indices().all(|(ix, ch)| match ix {
        4 | 7 | 13 | 16 => ch == '-',
        10 => ch == '_',
        19 => ch == '.',
        20 => ch == 'm',
        21 => ch == 'd',
        _ => ch.is_ascii_digit(),
    })
}

fn active_viewport_size(view: &Entity<LstGpuiApp>, cx: &mut VisualTestContext) -> (i32, i32) {
    view.update(cx, |app, _cx| {
        let bounds = app
            .active_view()
            .geometry
            .borrow()
            .bounds
            .expect("viewport should have rendered bounds");
        (
            (bounds.size.width / px(1.0)).round() as i32,
            (bounds.size.height / px(1.0)).round() as i32,
        )
    })
}

fn refresh_and_flush_reveal(view: &Entity<LstGpuiApp>, cx: &mut VisualTestContext, label: &str) {
    cx.refresh()
        .unwrap_or_else(|_| panic!("refresh {label} before queued reveal"));
    cx.run_until_parked();
    cx.update_window_entity(view, |app, window, cx| {
        app.flush_pending_reveal_for_test(window, cx);
    });
    cx.run_until_parked();
    cx.refresh()
        .unwrap_or_else(|_| panic!("refresh {label} after queued reveal"));
    cx.run_until_parked();
}

fn active_cursor_viewport_state(
    view: &Entity<LstGpuiApp>,
    cx: &mut VisualTestContext,
) -> (f32, f32, f32, usize, f32, usize) {
    view.update(cx, |app, _cx| {
        let active_view = app.active_view();
        let bounds = active_view
            .geometry
            .borrow()
            .bounds
            .expect("viewport should have rendered bounds");
        let cache = active_view.cache.borrow();
        let layout = cache
            .wrap_layout
            .as_ref()
            .expect("wrap layout should have been prepared");
        let cursor_row = crate::viewport::visual_row_for_char(app.active_tab(), &layout.layout)
            .expect("cursor should map to a visual row");
        let scroll_top = crate::viewport::scroll_top_for(&active_view.scroll);
        let max_offset = active_view.scroll.max_offset().height.max(px(0.0));
        let row_height = app.ui_px(crate::ui::theme::metrics::ROW_HEIGHT);

        (
            scroll_top / px(1.0),
            bounds.size.height / px(1.0),
            row_height / px(1.0),
            cursor_row,
            max_offset / px(1.0),
            layout.layout.total_rows,
        )
    })
}

fn active_editor_scrollbar_layout(
    view: &Entity<LstGpuiApp>,
    cx: &mut VisualTestContext,
) -> Option<VerticalScrollbarLayout> {
    view.update(cx, |app, _cx| {
        let active_view = app.active_view();
        let bounds = active_view
            .geometry
            .borrow()
            .bounds
            .expect("viewport should have rendered bounds");
        vertical_scrollbar_layout(
            Bounds::new(
                point(
                    bounds.right() - app.ui_px(crate::ui::theme::metrics::SCROLLBAR_TRACK_WIDTH),
                    bounds.top(),
                ),
                gpui::size(
                    app.ui_px(crate::ui::theme::metrics::SCROLLBAR_TRACK_WIDTH),
                    bounds.size.height,
                ),
            ),
            crate::viewport::scroll_top_for(&active_view.scroll),
            active_view.scroll.max_offset().height.max(px(0.0)),
            app.ui_scale(),
        )
    })
}

#[cfg(feature = "internal-invariants")]
fn tab_from_path(path: PathBuf, text: &str) -> EditorTab {
    EditorTab::from_path(TabId::from_raw(1), path, text)
}

#[test]
fn launch_args_accept_window_title() {
    let args = crate::launch::parse_launch_args_from(["--title", "lst-window", "/tmp/example.rs"])
        .expect("args should parse");

    assert_eq!(args.window_title.as_deref(), Some("lst-window"));
    assert_eq!(args.files, [PathBuf::from("/tmp/example.rs")]);
}

#[test]
fn primary_font_family_is_tx02() {
    assert_eq!(crate::ui::theme::typography::PRIMARY_FONT_FAMILY, "TX-02");
}

#[test]
fn launch_args_accept_scratchpad_dir() {
    let args = crate::launch::parse_launch_args_from([
        "--scratchpad-dir",
        "/tmp/lst-notes",
        "--scratchpad-dir=/tmp/lst-other-notes",
    ])
    .expect("args should parse");

    assert_eq!(
        args.scratchpad_dir,
        Some(PathBuf::from("/tmp/lst-other-notes"))
    );
    assert!(args.files.is_empty());
}

#[test]
fn launch_args_require_title_value() {
    let error = crate::launch::parse_launch_args_from(["--title"]).expect_err("missing title");

    assert!(matches!(
        error,
        crate::launch::LaunchArgError::Message(message) if message == "missing value for --title"
    ));
}

#[test]
fn launch_args_require_scratchpad_dir_value() {
    let error = crate::launch::parse_launch_args_from(["--scratchpad-dir"])
        .expect_err("missing scratchpad dir");

    assert!(matches!(
        error,
        crate::launch::LaunchArgError::Message(message)
            if message == "missing value for --scratchpad-dir"
    ));
}

#[test]
fn launch_model_loads_real_files_before_gpui_wiring() {
    let dir = temp_dir("launch-model");
    let ok = dir.join("ok.txt");
    let missing = dir.join("missing.txt");
    std::fs::write(&ok, "loaded").expect("write launch fixture");

    let model = initial_model_from_launch(LaunchArgs {
        files: vec![ok.clone(), missing.clone()],
        ..LaunchArgs::default()
    });
    let snapshot = model.snapshot();

    assert_eq!(snapshot.text, "loaded");
    assert_eq!(snapshot.active_path, Some(ok));
    assert!(snapshot
        .status
        .contains(&format!("Failed to open {}", missing.display())));

    std::fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[test]
fn launch_model_without_files_creates_timestamped_scratchpad() {
    let dir = temp_dir("launch-scratchpad");

    let model = initial_model_from_launch(LaunchArgs {
        scratchpad_dir: Some(dir.clone()),
        ..LaunchArgs::default()
    });
    let snapshot = model.snapshot();
    let path = snapshot
        .active_path
        .as_ref()
        .expect("scratchpad should be path backed");

    assert_eq!(snapshot.tab_count, 1);
    assert_eq!(snapshot.tab_scratchpad, [true]);
    assert!(path.starts_with(&dir));
    assert!(is_timestamped_scratchpad_name(
        path.file_name().and_then(|name| name.to_str()).unwrap()
    ));
    assert_eq!(std::fs::read_to_string(path).expect("read scratchpad"), "");

    std::fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[gpui::test]
fn app_input_handler_updates_real_editor_model(cx: &mut TestAppContext) {
    let (view, cx) = new_test_app(cx, LaunchArgs::default());

    cx.update_window_entity(&view, |app, window, cx| {
        app.replace_text_in_range(None, "hello", window, cx);
    });
    cx.run_until_parked();

    let snapshot = app_snapshot(&view, cx);
    assert_eq!(snapshot.model.text, "hello");
    assert_eq!(snapshot.model.status, "Ready.");
    assert_tab_views_match_model(&snapshot);
}

#[gpui::test]
fn typing_at_wrapped_line_end_keeps_cursor_visible(cx: &mut TestAppContext) {
    let dir = temp_dir("wrapped-reveal");
    let path = dir.join("long.txt");
    std::fs::write(&path, "a".repeat(30_000)).expect("write wrapped reveal fixture");
    let (view, cx) = new_test_app(
        cx,
        LaunchArgs {
            files: vec![path],
            ..LaunchArgs::default()
        },
    );
    refresh_and_flush_reveal(&view, cx, "initial wrapped buffer");

    view.update(cx, |app, cx| {
        app.update_model(cx, true, |model| {
            let end = model.active_tab().len_chars();
            model.move_to_char(end, false, None);
        });
    });
    refresh_and_flush_reveal(&view, cx, "cursor move to wrapped EOF");
    let before = active_cursor_viewport_state(&view, cx);
    assert!(
        before.0 > before.2,
        "fixture should scroll before typing; scroll_top={}, row_height={}, cursor_row={}, max_offset={}, total_rows={}",
        before.0,
        before.2,
        before.3,
        before.4,
        before.5
    );

    cx.update_window_entity(&view, |app, window, cx| {
        app.replace_text_in_range(None, "x", window, cx);
    });
    refresh_and_flush_reveal(&view, cx, "typing at wrapped EOF");

    let after = active_cursor_viewport_state(&view, cx);
    let caret_top = after.2 * after.3 as f32;
    let caret_bottom = caret_top + after.2;
    assert!(
        after.0 <= caret_top + 1.0 && caret_bottom <= after.0 + after.1 + 1.0,
        "cursor visual row should remain visible after typing; scroll_top={}, viewport_height={}, row_height={}, cursor_row={}",
        after.0,
        after.1,
        after.2,
        after.3
    );
    assert!(
        after.0 >= before.0 - after.2,
        "typing at wrapped EOF should not reset the viewport toward the top; before={}, after={}",
        before.0,
        after.0
    );

    std::fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[gpui::test]
fn app_actions_copy_and_paste_through_gpui_clipboard(cx: &mut TestAppContext) {
    let dir = temp_dir("clipboard");
    let path = dir.join("note.txt");
    std::fs::write(&path, "hello world").expect("write clipboard fixture");
    let (view, cx) = new_test_app(
        cx,
        LaunchArgs {
            files: vec![path],
            ..LaunchArgs::default()
        },
    );

    cx.dispatch_action(SelectAll);
    cx.dispatch_action(CopySelection);
    let copied = cx
        .read_from_clipboard()
        .and_then(|item| item.text())
        .expect("clipboard should contain copied text");
    assert_eq!(copied, "hello world");

    cx.write_to_clipboard(ClipboardItem::new_string("replacement".to_string()));
    cx.dispatch_action(PasteClipboard);

    let snapshot = app_snapshot(&view, cx);
    assert_eq!(snapshot.model.text, "replacement");
    assert_eq!(snapshot.model.status, "Pasted 1 line(s).");
    assert_tab_views_match_model(&snapshot);

    std::fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[gpui::test]
fn middle_click_pastes_gpui_primary_selection(cx: &mut TestAppContext) {
    let dir = temp_dir("primary-paste");
    let (view, cx) = new_test_app(
        cx,
        LaunchArgs {
            scratchpad_dir: Some(dir.clone()),
            ..LaunchArgs::default()
        },
    );
    cx.update(|_, cx| cx.write_to_primary(ClipboardItem::new_string("primary text".to_string())));
    cx.refresh().expect("render editor before mouse paste");
    cx.run_until_parked();
    let paste_position = view.update(cx, |app, _cx| {
        let bounds = app
            .active_view()
            .geometry
            .borrow()
            .bounds
            .expect("viewport should have rendered bounds");
        point(bounds.left() + px(80.0), bounds.top() + px(8.0))
    });

    cx.simulate_mouse_down(paste_position, MouseButton::Middle, Modifiers::default());

    let snapshot = app_snapshot(&view, cx);
    assert_eq!(snapshot.model.text, "primary text");
    assert_eq!(snapshot.model.status, "Pasted 1 line(s).");
    assert_tab_views_match_model(&snapshot);

    std::fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[gpui::test]
fn mouse_selection_updates_gpui_primary_selection(cx: &mut TestAppContext) {
    let dir = temp_dir("mouse-primary");
    let path = dir.join("note.txt");
    std::fs::write(&path, "hello").expect("write mouse selection fixture");
    let (view, cx) = new_test_app(
        cx,
        LaunchArgs {
            files: vec![path],
            ..LaunchArgs::default()
        },
    );
    cx.refresh().expect("render editor before mouse selection");
    cx.run_until_parked();
    let (start, end) = view.update(cx, |app, _cx| {
        let bounds = app
            .active_view()
            .geometry
            .borrow()
            .bounds
            .expect("viewport should have rendered bounds");
        let x = bounds.left() + code_origin_pad(app.model.show_gutter(), app.ui_scale()) + px(1.0);
        let y = bounds.top() + px(8.0);
        (point(x, y), point(x + px(80.0), y))
    });

    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(end, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());

    assert_eq!(
        cx.update(|_, cx| cx.read_from_primary().and_then(|item| item.text())),
        Some("hello".to_string())
    );

    std::fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[gpui::test]
fn editor_scrollbar_drag_scrolls_without_text_selection(cx: &mut TestAppContext) {
    let dir = temp_dir("scrollbar-drag");
    let path = dir.join("long.txt");
    let text = (0..400)
        .map(|line| format!("line {line}\n"))
        .collect::<String>();
    std::fs::write(&path, text).expect("write scrollbar drag fixture");
    let (view, cx) = new_test_app(
        cx,
        LaunchArgs {
            files: vec![path],
            ..LaunchArgs::default()
        },
    );
    cx.refresh()
        .expect("render long editor before scrollbar drag");
    cx.run_until_parked();

    let layout = active_editor_scrollbar_layout(&view, cx)
        .expect("long editor should expose a scrollbar layout");
    let x = layout.thumb_bounds.left() + layout.thumb_bounds.size.width / 2.0;
    let start = point(x, layout.thumb_bounds.top() + px(2.0));
    let end = point(x, layout.track_bounds.bottom() - px(4.0));

    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(end, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());

    let (scroll_top, selection, active_text_drag, active_scrollbar_drag) =
        view.update(cx, |app, _cx| {
            (
                crate::viewport::scroll_top_for(&app.active_view().scroll),
                app.model.active_tab().selected_range(),
                app.selection_drag.is_some(),
                app.editor_scrollbar_drag.is_some(),
            )
        });
    assert!(
        scroll_top > px(0.0),
        "dragging the scrollbar thumb should move the editor scroll position"
    );
    assert_eq!(selection, 0..0);
    assert!(!active_text_drag);
    assert!(!active_scrollbar_drag);

    std::fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[gpui::test]
fn editor_scrollbar_track_click_pages_without_text_selection(cx: &mut TestAppContext) {
    let dir = temp_dir("scrollbar-track");
    let path = dir.join("long.txt");
    let text = (0..400)
        .map(|line| format!("line {line}\n"))
        .collect::<String>();
    std::fs::write(&path, text).expect("write scrollbar track fixture");
    let (view, cx) = new_test_app(
        cx,
        LaunchArgs {
            files: vec![path],
            ..LaunchArgs::default()
        },
    );
    cx.refresh()
        .expect("render long editor before scrollbar track click");
    cx.run_until_parked();

    let layout = active_editor_scrollbar_layout(&view, cx)
        .expect("long editor should expose a scrollbar layout");
    let click = point(
        layout.thumb_bounds.left() + layout.thumb_bounds.size.width / 2.0,
        (layout.thumb_bounds.bottom() + px(20.0)).min(layout.track_bounds.bottom() - px(1.0)),
    );

    cx.simulate_mouse_down(click, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_up(click, MouseButton::Left, Modifiers::default());

    let (scroll_top, selection, active_text_drag) = view.update(cx, |app, _cx| {
        (
            crate::viewport::scroll_top_for(&app.active_view().scroll),
            app.model.active_tab().selected_range(),
            app.selection_drag.is_some(),
        )
    });
    assert!(
        scroll_top > px(0.0),
        "clicking below the scrollbar thumb should page the editor down"
    );
    assert_eq!(selection, 0..0);
    assert!(!active_text_drag);

    std::fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[gpui::test]
fn editor_scrollbar_is_absent_without_overflow(cx: &mut TestAppContext) {
    let dir = temp_dir("scrollbar-short");
    let path = dir.join("short.txt");
    std::fs::write(&path, "short\n").expect("write short scrollbar fixture");
    let (view, cx) = new_test_app(
        cx,
        LaunchArgs {
            files: vec![path],
            ..LaunchArgs::default()
        },
    );
    cx.refresh()
        .expect("render short editor before scrollbar absence check");
    cx.run_until_parked();

    assert!(active_editor_scrollbar_layout(&view, cx).is_none());

    std::fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[gpui::test]
fn app_find_input_flow_is_observable_at_app_boundary(cx: &mut TestAppContext) {
    let dir = temp_dir("find");
    let path = dir.join("note.txt");
    std::fs::write(&path, "one two one").expect("write find fixture");
    let (view, cx) = new_test_app(
        cx,
        LaunchArgs {
            files: vec![path],
            ..LaunchArgs::default()
        },
    );

    cx.dispatch_action(FindOpen);
    cx.refresh().expect("refresh after focus request");
    cx.run_until_parked();
    cx.simulate_input("one");

    let snapshot = app_snapshot(&view, cx);
    assert!(snapshot.model.find_visible);
    assert_eq!(snapshot.model.find_query, "one");
    assert_eq!(snapshot.find_query_input, "one");
    assert_eq!(snapshot.model.find_matches, 2);
    assert_eq!(snapshot.model.find_current, Some(0));
    assert_eq!(snapshot.model.find_active_match, Some(0..3));
    assert_eq!(snapshot.model.selection, 0..0);
    assert_tab_views_match_model(&snapshot);

    cx.simulate_keystrokes("escape");
    let snapshot = app_snapshot(&view, cx);
    assert!(!snapshot.model.find_visible);
    assert_eq!(snapshot.pending_focus, None);

    std::fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[gpui::test]
fn app_find_open_syncs_selected_text_into_input(cx: &mut TestAppContext) {
    let (view, cx) = new_test_app(cx, LaunchArgs::default());

    cx.update_window_entity(&view, |app, window, cx| {
        app.replace_text_in_range(None, "one two one", window, cx);
    });
    view.update(cx, |app, cx| {
        app.update_model(cx, true, |model| {
            model.set_selection(0..3, false);
        });
    });

    cx.dispatch_action(FindOpen);
    cx.refresh().expect("refresh after focus request");
    cx.run_until_parked();

    let snapshot = app_snapshot(&view, cx);
    assert!(snapshot.model.find_visible);
    assert_eq!(snapshot.model.find_query, "one");
    assert_eq!(snapshot.find_query_input, "one");
    assert_eq!(snapshot.model.find_matches, 2);
    assert_tab_views_match_model(&snapshot);
}

#[gpui::test]
fn find_input_navigation_does_not_extend_document_selection_without_matches(
    cx: &mut TestAppContext,
) {
    let (view, cx) = new_test_app(cx, LaunchArgs::default());

    cx.update_window_entity(&view, |app, window, cx| {
        app.replace_text_in_range(None, "alpha\nbeta\ngamma", window, cx);
    });
    view.update(cx, |app, cx| {
        app.update_model(cx, true, |model| {
            model.move_to_char("alpha\n".chars().count(), false, None);
        });
    });

    cx.dispatch_action(FindOpen);
    cx.refresh().expect("refresh after find focus request");
    cx.run_until_parked();
    cx.simulate_input("zzz");
    cx.simulate_keystrokes("ctrl-down");

    let snapshot = app_snapshot(&view, cx);
    let expected_cursor = "alpha\n".chars().count();
    assert_eq!(snapshot.model.find_matches, 0);
    assert_eq!(snapshot.model.find_current, None);
    assert_eq!(snapshot.model.find_active_match, None);
    assert_eq!(snapshot.model.cursor, expected_cursor);
    assert_eq!(snapshot.model.selection, expected_cursor..expected_cursor);
}

#[gpui::test]
fn hover_after_find_with_stale_drag_state_does_not_select_text(cx: &mut TestAppContext) {
    let (view, cx) = new_test_app(cx, LaunchArgs::default());

    cx.update_window_entity(&view, |app, window, cx| {
        app.replace_text_in_range(None, "alpha\nbeta\ngamma", window, cx);
    });
    let expected_cursor = "alpha\n".chars().count();
    view.update(cx, |app, cx| {
        app.update_model(cx, true, |model| {
            model.move_to_char(expected_cursor, false, None);
        });
    });

    cx.dispatch_action(FindOpen);
    cx.refresh().expect("refresh after find focus request");
    cx.run_until_parked();
    cx.simulate_input("zzz");
    cx.refresh().expect("render editor before stale drag hover");
    cx.run_until_parked();

    let hover_position = view.update(cx, |app, _cx| {
        let bounds = app
            .active_view()
            .geometry
            .borrow()
            .bounds
            .expect("viewport should have rendered bounds");
        let x = bounds.left() + code_origin_pad(app.model.show_gutter(), app.ui_scale()) + px(1.0);
        let y = bounds.top() + px(8.0);
        point(x, y)
    });
    view.update(cx, |app, _cx| {
        app.force_stale_drag_selection_for_test(hover_position);
    });

    cx.simulate_mouse_move(hover_position, None, Modifiers::default());

    let snapshot = app_snapshot(&view, cx);
    assert_eq!(snapshot.model.find_matches, 0);
    assert_eq!(snapshot.model.cursor, expected_cursor);
    assert_eq!(snapshot.model.selection, expected_cursor..expected_cursor);
    view.update(cx, |app, _cx| {
        assert!(!app.has_active_drag_selection_for_test());
    });
}

#[gpui::test]
fn find_overlay_click_keeps_text_input_out_of_editor(cx: &mut TestAppContext) {
    let (view, cx) = new_test_app(cx, LaunchArgs::default());

    cx.update_window_entity(&view, |app, window, cx| {
        app.replace_text_in_range(None, "alpha beta alpha", window, cx);
    });
    cx.dispatch_action(FindOpen);
    cx.refresh().expect("render find overlay");
    cx.run_until_parked();

    cx.simulate_click(point(px(2230.0), px(169.0)), Modifiers::default());
    cx.refresh().expect("refresh after overlay click");
    cx.run_until_parked();
    cx.simulate_input("alpha");

    let snapshot = app_snapshot(&view, cx);
    assert_eq!(snapshot.model.text, "alpha beta alpha");
    assert_eq!(snapshot.model.find_query, "alpha");
    assert_eq!(snapshot.find_query_input, "alpha");
    assert_eq!(snapshot.model.find_matches, 2);
}

#[gpui::test]
fn app_goto_input_syncs_open_submit_and_close(cx: &mut TestAppContext) {
    let (view, cx) = new_test_app(cx, LaunchArgs::default());

    cx.update_window_entity(&view, |app, window, cx| {
        app.replace_text_in_range(None, "alpha\nbeta\ngamma", window, cx);
    });

    cx.dispatch_action(GotoLineOpen);
    cx.refresh().expect("refresh after goto focus request");
    cx.run_until_parked();
    assert_eq!(app_snapshot(&view, cx).goto_line_input, "");

    cx.simulate_input("2");
    let snapshot = app_snapshot(&view, cx);
    assert_eq!(snapshot.model.goto_line.as_deref(), Some("2"));
    assert_eq!(snapshot.goto_line_input, "2");

    cx.simulate_keystrokes("enter");
    let snapshot = app_snapshot(&view, cx);
    assert_eq!(snapshot.model.cursor, "alpha\n".chars().count());
    assert_eq!(snapshot.model.goto_line, None);
    assert_eq!(snapshot.goto_line_input, "");
    assert_tab_views_match_model(&snapshot);
}

#[gpui::test]
fn app_goto_input_accepts_line_and_column(cx: &mut TestAppContext) {
    let (view, cx) = new_test_app(cx, LaunchArgs::default());

    cx.update_window_entity(&view, |app, window, cx| {
        app.replace_text_in_range(None, "alpha\nbeta\ngamma", window, cx);
    });

    cx.dispatch_action(GotoLineOpen);
    cx.refresh().expect("refresh after goto focus request");
    cx.run_until_parked();

    cx.simulate_input("2:3");
    let snapshot = app_snapshot(&view, cx);
    assert_eq!(snapshot.model.goto_line.as_deref(), Some("2:3"));
    assert_eq!(snapshot.goto_line_input, "2:3");

    cx.simulate_keystrokes("enter");
    let snapshot = app_snapshot(&view, cx);
    assert_eq!(snapshot.model.cursor_position.line, 1);
    assert_eq!(snapshot.model.cursor_position.column, 2);
    assert_eq!(snapshot.model.goto_line, None);
    assert_eq!(snapshot.goto_line_input, "");
    assert_tab_views_match_model(&snapshot);
}

#[gpui::test]
fn rendered_wrapped_rows_fill_viewport_width_except_remainder(cx: &mut TestAppContext) {
    let (view, cx) = new_test_app(cx, LaunchArgs::default());

    let text = "a".repeat(560);
    let text_len = text.chars().count();
    cx.update_window_entity(&view, |app, window, cx| {
        app.replace_text_in_range(None, &text, window, cx);
    });
    cx.refresh().expect("render long line");
    cx.run_until_parked();

    cx.update_window_entity(&view, |app, window, _cx| {
        let geometry = app.active_view().geometry.borrow();
        let bounds = geometry.bounds.expect("viewport bounds");
        let rows = geometry.rows.clone();
        drop(geometry);

        let char_width = crate::viewport::code_char_width(window, app.ui_scale());
        let wrap_columns = app.active_wrap_columns(window);
        let content_width = bounds.size.width
            - crate::viewport::code_origin_pad(app.model.show_gutter(), app.ui_scale());
        let row_lengths: Vec<usize> = rows
            .iter()
            .map(|row| row.display_end_char.saturating_sub(row.line_start_char))
            .collect();

        assert!(
            row_lengths.len() > 1,
            "fixture should wrap into multiple visual rows"
        );
        assert_eq!(row_lengths.iter().sum::<usize>(), text_len);

        for row_length in row_lengths.iter().take(row_lengths.len().saturating_sub(1)) {
            assert_eq!(
                *row_length, wrap_columns,
                "full wrapped rows should consume the computed wrap width"
            );
        }

        let char_width_px = char_width / px(1.0);
        for row in rows.iter().take(rows.len().saturating_sub(1)) {
            let code_width = row
                .code_line
                .as_ref()
                .expect("wrapped row should have shaped code")
                .width;
            let slack = (content_width - code_width) / px(1.0);
            assert!(
                (-1.0..=char_width_px + 1.0).contains(&slack),
                "full wrapped row should end within one character cell of the viewport edge; slack={slack}, char_width={char_width_px}"
            );
        }

        let last_width = rows
            .last()
            .and_then(|row| row.code_line.as_ref())
            .expect("last wrapped row should have shaped code")
            .width;
        assert!(
            last_width < content_width,
            "the final visual row should be the shorter remainder"
        );
    });
}

#[gpui::test]
fn code_font_is_effectively_monospace_for_basic_ascii(cx: &mut TestAppContext) {
    let (view, cx) = new_test_app(cx, LaunchArgs::default());

    cx.update_window_entity(&view, |app, window, _cx| {
        let font = crate::ui::theme::typography::primary_font();
        let font_size = app.ui_px(crate::ui::theme::metrics::CODE_FONT_SIZE);
        let style_for = |text: &str| {
            [gpui::TextRun {
                len: text.len(),
                font: font.clone(),
                color: gpui::rgb(crate::ui::theme::role::TEXT).into(),
                background_color: None,
                underline: None,
                strikethrough: None,
            }]
        };
        let width_per_char = |text: &str| {
            let text = text.to_string();
            let shaped = window
                .text_system()
                .shape_line(gpui::SharedString::from(text.clone()), font_size, &style_for(&text), None);
            shaped.width / text.chars().count() as f32 / px(1.0)
        };

        let zero = width_per_char("0000000000000000");
        let a = width_per_char("aaaaaaaaaaaaaaaa");
        let m = width_per_char("mmmmmmmmmmmmmmmm");
        let space = width_per_char("                ");
        let diff = zero.max(a).max(m).max(space) - zero.min(a).min(m).min(space);

        assert!(
            diff <= 0.25,
            "code font must be monospace-like for wrapping; zero={zero}, a={a}, m={m}, space={space}, diff={diff}"
        );
    });
}

#[gpui::test]
fn exact_wrap_multiples_fill_every_visual_row(cx: &mut TestAppContext) {
    let (view, cx) = new_test_app(cx, LaunchArgs::default());

    cx.refresh().expect("initial render");
    cx.run_until_parked();

    cx.update_window_entity(&view, |app, window, cx| {
        let wrap_columns = app.active_wrap_columns(window);
        let text = "a".repeat(wrap_columns * 4);
        app.replace_text_in_range(None, &text, window, cx);
    });
    cx.refresh().expect("render exact wrap multiple");
    cx.run_until_parked();

    cx.update_window_entity(&view, |app, window, _cx| {
        let geometry = app.active_view().geometry.borrow();
        let bounds = geometry.bounds.expect("viewport bounds");
        let rows = geometry.rows.clone();
        drop(geometry);

        let wrap_columns = app.active_wrap_columns(window);
        let char_width = crate::viewport::code_char_width(window, app.ui_scale()) / px(1.0);
        let content_width = bounds.size.width
            - crate::viewport::code_origin_pad(app.model.show_gutter(), app.ui_scale());
        let row_lengths: Vec<usize> = rows
            .iter()
            .map(|row| row.display_end_char.saturating_sub(row.line_start_char))
            .collect();

        assert_eq!(row_lengths, vec![wrap_columns; 4]);
        for row in &rows {
            let code_width = row
                .code_line
                .as_ref()
                .expect("wrapped row should have shaped code")
                .width;
            let slack = (content_width - code_width) / px(1.0);
            assert!(
                (-1.0..=char_width + 1.0).contains(&slack),
                "exact wrap multiple should fill the viewport on every row; slack={slack}, char_width={char_width}"
            );
        }
    });
}

#[gpui::test]
fn status_details_include_wrap_columns_after_render(cx: &mut TestAppContext) {
    let (view, cx) = new_test_app(cx, LaunchArgs::default());

    cx.update_window_entity(&view, |app, window, cx| {
        app.replace_text_in_range(None, &"a".repeat(2_000), window, cx);
    });
    cx.refresh().expect("render wrapped content");
    cx.run_until_parked();

    let details = view.update(cx, |app, _cx| app.status_details());
    assert!(details.contains("Wrap "));
    assert!(details.contains(" cols"));
}

#[gpui::test]
fn status_details_ignore_wrap_layouts_that_have_not_been_painted(cx: &mut TestAppContext) {
    let (view, cx) = new_test_app(cx, LaunchArgs::default());

    view.update(cx, |app, _cx| {
        let revision = app.active_tab().revision();
        let lines = app.model.active_tab_lines();
        app.active_view().cache.borrow_mut().wrap_layout =
            Some(crate::viewport::CachedWrapLayout {
                revision,
                layout: lst_editor::wrap::build_wrap_layout(lines.as_ref(), 37, true),
            });
    });

    let details = view.update(cx, |app, _cx| app.status_details());
    assert!(details.contains("Wrap"));
    assert!(!details.contains("37 cols"));
}

#[gpui::test]
fn find_and_goto_overlays_do_not_resize_viewport(cx: &mut TestAppContext) {
    let (view, cx) = new_test_app(cx, LaunchArgs::default());
    cx.refresh().expect("initial render");
    cx.run_until_parked();
    let before = active_viewport_size(&view, cx);

    cx.dispatch_action(GotoLineOpen);
    cx.refresh().expect("render goto overlay");
    cx.run_until_parked();
    assert_eq!(active_viewport_size(&view, cx), before);

    cx.dispatch_action(FindOpen);
    cx.refresh().expect("render stacked overlays");
    cx.run_until_parked();
    assert_eq!(active_viewport_size(&view, cx), before);
}

#[gpui::test]
fn dirty_close_decision_can_cancel_or_discard_without_dialog(cx: &mut TestAppContext) {
    let (view, cx) = new_test_app(cx, LaunchArgs::default());
    cx.update_window_entity(&view, |app, window, cx| {
        app.replace_text_in_range(None, "unsaved", window, cx);
    });
    let tab_id = app_snapshot(&view, cx).model.active_tab_id;

    view.update(cx, |app, cx| {
        app.apply_unsaved_close_decision(tab_id, crate::runtime::UnsavedCloseDecision::Cancel, cx);
    });
    let snapshot = app_snapshot(&view, cx);
    assert_eq!(snapshot.model.text, "unsaved");
    assert!(snapshot.model.tab_modified[0]);
    assert_eq!(snapshot.model.status, "Close cancelled.");

    view.update(cx, |app, cx| {
        app.apply_unsaved_close_decision(tab_id, crate::runtime::UnsavedCloseDecision::Discard, cx);
    });
    let snapshot = app_snapshot(&view, cx);
    assert_eq!(snapshot.model.tab_count, 1);
    assert_eq!(snapshot.model.text, "");
    assert!(!snapshot.model.tab_modified[0]);
}

#[gpui::test]
fn dirty_close_save_writes_exact_tab_then_closes(cx: &mut TestAppContext) {
    let dir = temp_dir("close-save");
    let path = dir.join("note.txt");
    std::fs::write(&path, "old").expect("write close-save fixture");
    let (view, cx) = new_test_app(
        cx,
        LaunchArgs {
            files: vec![path.clone()],
            ..LaunchArgs::default()
        },
    );
    cx.update_window_entity(&view, |app, window, cx| {
        app.replace_text_in_range(None, "new ", window, cx);
    });
    let tab_id = app_snapshot(&view, cx).model.active_tab_id;

    view.update(cx, |app, cx| {
        app.apply_unsaved_close_decision(tab_id, crate::runtime::UnsavedCloseDecision::Save, cx);
    });
    cx.run_until_parked();

    assert_eq!(
        std::fs::read_to_string(&path).expect("read saved close file"),
        "new old"
    );
    let snapshot = app_snapshot(&view, cx);
    assert_eq!(snapshot.model.tab_count, 1);
    assert_eq!(snapshot.model.active_path, None);
    assert_eq!(snapshot.model.text, "");

    std::fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[gpui::test]
fn clean_external_file_change_reloads_without_prompt(cx: &mut TestAppContext) {
    let dir = temp_dir("clean-reload");
    let path = dir.join("note.txt");
    std::fs::write(&path, "old").expect("write clean reload fixture");
    let (view, cx) = new_test_app(
        cx,
        LaunchArgs {
            files: vec![path.clone()],
            ..LaunchArgs::default()
        },
    );
    std::fs::write(&path, "new content").expect("write external clean reload");

    view.update(cx, |app, cx| {
        app.check_external_file_changes(cx);
    });

    let snapshot = app_snapshot(&view, cx);
    assert_eq!(snapshot.model.text, "new content");
    assert!(!snapshot.model.tab_modified[0]);
    assert_eq!(
        snapshot.model.status,
        format!("Reloaded {}.", path.display())
    );

    std::fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[gpui::test]
fn app_tab_actions_keep_model_and_tab_views_aligned(cx: &mut TestAppContext) {
    let (view, cx) = new_test_app(cx, LaunchArgs::default());

    cx.dispatch_action(NewTab);
    cx.dispatch_action(NewTab);
    cx.dispatch_action(CloseActiveTab);

    let snapshot = app_snapshot(&view, cx);
    assert_eq!(snapshot.model.tab_count, 2);
    assert_eq!(snapshot.model.active, 1);
    assert!(snapshot
        .model
        .tab_scratchpad
        .iter()
        .all(|is_scratchpad| *is_scratchpad));
    assert!(snapshot
        .model
        .tab_titles
        .iter()
        .all(|title| title.ends_with(".md")));
    assert_tab_views_match_model(&snapshot);
}

#[gpui::test]
fn new_tab_creates_a_real_scratchpad_file(cx: &mut TestAppContext) {
    let dir = temp_dir("new-scratchpad");
    let (view, cx) = new_test_app(
        cx,
        LaunchArgs {
            scratchpad_dir: Some(dir.clone()),
            ..LaunchArgs::default()
        },
    );

    cx.dispatch_action(NewTab);

    let snapshot = app_snapshot(&view, cx);
    let active_path = snapshot
        .model
        .active_path
        .as_ref()
        .expect("active scratchpad should have a path");
    assert_eq!(snapshot.model.tab_count, 2);
    assert_eq!(snapshot.model.tab_scratchpad, [true, true]);
    assert!(active_path.starts_with(&dir));
    assert_eq!(
        std::fs::read_dir(&dir)
            .expect("read scratchpad dir")
            .count(),
        2
    );

    std::fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[gpui::test]
fn closing_empty_scratchpad_removes_its_file(cx: &mut TestAppContext) {
    let dir = temp_dir("close-empty-scratchpad");
    let (view, cx) = new_test_app(
        cx,
        LaunchArgs {
            scratchpad_dir: Some(dir.clone()),
            ..LaunchArgs::default()
        },
    );
    cx.dispatch_action(NewTab);
    let empty_path = app_snapshot(&view, cx)
        .model
        .active_path
        .expect("new scratchpad should have a path");

    cx.dispatch_action(CloseActiveTab);

    let snapshot = app_snapshot(&view, cx);
    assert_eq!(snapshot.model.tab_count, 1);
    assert!(!empty_path.exists());
    assert_eq!(
        std::fs::read_dir(&dir)
            .expect("read scratchpad dir")
            .count(),
        1
    );

    std::fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[gpui::test]
fn closing_only_empty_scratchpad_removes_its_file(cx: &mut TestAppContext) {
    let dir = temp_dir("close-only-empty-scratchpad");
    let (view, cx) = new_test_app(
        cx,
        LaunchArgs {
            scratchpad_dir: Some(dir.clone()),
            ..LaunchArgs::default()
        },
    );
    let path = app_snapshot(&view, cx)
        .model
        .active_path
        .expect("scratchpad should have a path");
    assert!(path.exists());

    cx.dispatch_action(CloseActiveTab);

    assert!(!path.exists());
    assert_eq!(
        std::fs::read_dir(&dir)
            .expect("read scratchpad dir")
            .count(),
        0
    );

    std::fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[gpui::test]
fn dirty_quit_cancel_keeps_unsaved_edits_without_clipboard_write(cx: &mut TestAppContext) {
    let dir = temp_dir("quit-cancel");
    let path = dir.join("note.txt");
    std::fs::write(&path, "old").expect("write quit-cancel fixture");
    let (view, cx) = new_test_app(
        cx,
        LaunchArgs {
            files: vec![path.clone()],
            ..LaunchArgs::default()
        },
    );
    cx.write_to_clipboard(ClipboardItem::new_string("before".to_string()));
    cx.update(|_, cx| cx.write_to_primary(ClipboardItem::new_string("before".to_string())));
    cx.update_window_entity(&view, |app, window, cx| {
        app.replace_text_in_range(None, "new ", window, cx);
    });
    let tab_id = app_snapshot(&view, cx).model.active_tab_id;

    view.update(cx, |app, cx| {
        app.apply_unsaved_quit_decision(tab_id, crate::runtime::UnsavedCloseDecision::Cancel, cx);
    });

    let snapshot = app_snapshot(&view, cx);
    assert_eq!(snapshot.model.text, "new old");
    assert!(snapshot.model.tab_modified[0]);
    assert_eq!(snapshot.model.status, "Close cancelled.");
    assert_eq!(
        std::fs::read_to_string(&path).expect("read quit-cancel file"),
        "old"
    );
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some("before".to_string())
    );
    assert_eq!(
        cx.update(|_, cx| cx.read_from_primary().and_then(|item| item.text())),
        Some("before".to_string())
    );

    std::fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[gpui::test]
fn dirty_quit_save_writes_before_copying_clipboards(cx: &mut TestAppContext) {
    let dir = temp_dir("quit-save");
    let path = dir.join("note.txt");
    std::fs::write(&path, "old").expect("write quit-save fixture");
    let (view, cx) = new_test_app(
        cx,
        LaunchArgs {
            files: vec![path.clone()],
            ..LaunchArgs::default()
        },
    );
    cx.update_window_entity(&view, |app, window, cx| {
        app.replace_text_in_range(None, "new ", window, cx);
    });
    let tab_id = app_snapshot(&view, cx).model.active_tab_id;

    view.update(cx, |app, cx| {
        app.apply_unsaved_quit_decision(tab_id, crate::runtime::UnsavedCloseDecision::Save, cx);
    });
    cx.run_until_parked();

    assert_eq!(
        std::fs::read_to_string(&path).expect("read quit-save file"),
        "new old"
    );
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some("new old".to_string())
    );
    assert_eq!(
        cx.update(|_, cx| cx.read_from_primary().and_then(|item| item.text())),
        Some("new old".to_string())
    );

    std::fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[gpui::test]
fn dirty_scratchpad_quit_save_writes_before_copying_clipboards(cx: &mut TestAppContext) {
    let dir = temp_dir("quit-save-scratchpad");
    let (view, cx) = new_test_app(
        cx,
        LaunchArgs {
            scratchpad_dir: Some(dir.clone()),
            ..LaunchArgs::default()
        },
    );
    cx.update_window_entity(&view, |app, window, cx| {
        app.replace_text_in_range(None, "scratch text", window, cx);
    });
    let snapshot = app_snapshot(&view, cx);
    let tab_id = snapshot.model.active_tab_id;
    let path = snapshot
        .model
        .active_path
        .expect("scratchpad should be path backed");

    view.update(cx, |app, cx| {
        app.apply_unsaved_quit_decision(tab_id, crate::runtime::UnsavedCloseDecision::Save, cx);
    });
    cx.run_until_parked();

    assert_eq!(
        std::fs::read_to_string(&path).expect("read saved scratchpad"),
        "scratch text"
    );
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some("scratch text".to_string())
    );
    assert_eq!(
        cx.update(|_, cx| cx.read_from_primary().and_then(|item| item.text())),
        Some("scratch text".to_string())
    );

    std::fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[gpui::test]
fn quit_copies_active_text_to_clipboard_and_primary(cx: &mut TestAppContext) {
    let dir = temp_dir("quit-copy");
    let path = dir.join("note.txt");
    std::fs::write(&path, "quit text").expect("write quit-copy fixture");
    let (view, cx) = new_test_app(
        cx,
        LaunchArgs {
            files: vec![path],
            ..LaunchArgs::default()
        },
    );

    view.update(cx, |app, cx| {
        app.request_quit(cx);
    });
    cx.run_until_parked();

    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some("quit text".to_string())
    );
    assert_eq!(
        cx.update(|_, cx| cx.read_from_primary().and_then(|item| item.text())),
        Some("quit text".to_string())
    );

    std::fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[test]
fn utf16_range_conversion_handles_surrogate_pairs() {
    let buffer = Rope::from_str("a🙂b");

    assert_eq!(char_range_to_utf16_range(&buffer, &(1..2)), 1..3);
    assert_eq!(utf16_range_to_char_range_in_text("a🙂b", &(1..3)), 1..2);
}

#[cfg(feature = "internal-invariants")]
#[test]
fn autosave_revision_requires_a_unique_matching_tab() {
    let path = PathBuf::from("/tmp/example.rs");
    let tab = tab_from_path(path.clone(), "fn main() {}\n");

    assert!(autosave_revision_is_current(
        &[tab],
        TabId::from_raw(1),
        &path,
        0
    ));

    let mut stale_tab = tab_from_path(path.clone(), "fn main() {}\n");
    stale_tab.replace_char_range(0..0, "// ");
    assert!(!autosave_revision_is_current(
        &[stale_tab],
        TabId::from_raw(1),
        &path,
        0
    ));

    let first = tab_from_path(path.clone(), "one\n");
    let second = tab_from_path(path.clone(), "two\n");
    assert!(!autosave_revision_is_current(
        &[first, second],
        TabId::from_raw(1),
        &path,
        0
    ));
}

#[test]
fn rust_highlighting_keeps_multiline_comment_context() {
    let lines = compute_syntax_highlights(
        SyntaxLanguage::Rust,
        "/* first line\nsecond line */\nlet x = 1;\n",
    );

    assert!(lines[0]
        .iter()
        .any(|span| span.color == theme_syntax::COMMENT));
    assert!(lines[1]
        .iter()
        .any(|span| span.color == theme_syntax::COMMENT));
    assert!(lines[2]
        .iter()
        .all(|span| span.color != theme_syntax::COMMENT));
}

#[test]
fn syntax_mode_maps_core_extensions() {
    let cases = [
        ("example.rs", SyntaxLanguage::Rust),
        ("example.py", SyntaxLanguage::Python),
        ("example.pyw", SyntaxLanguage::Python),
        ("example.js", SyntaxLanguage::JavaScript),
        ("example.mjs", SyntaxLanguage::JavaScript),
        ("example.cjs", SyntaxLanguage::JavaScript),
        ("example.jsx", SyntaxLanguage::Jsx),
        ("example.ts", SyntaxLanguage::TypeScript),
        ("example.tsx", SyntaxLanguage::Tsx),
        ("example.json", SyntaxLanguage::Json),
        ("example.toml", SyntaxLanguage::Toml),
        ("example.yaml", SyntaxLanguage::Yaml),
        ("example.yml", SyntaxLanguage::Yaml),
        ("example.md", SyntaxLanguage::Markdown),
        ("example.markdown", SyntaxLanguage::Markdown),
        ("example.html", SyntaxLanguage::Html),
        ("example.htm", SyntaxLanguage::Html),
        ("example.css", SyntaxLanguage::Css),
    ];

    for (path, language) in cases {
        let detected = lst_editor::language::detect(Some(&PathBuf::from(path)), None);
        assert_eq!(
            syntax_mode_for_language(detected),
            SyntaxMode::TreeSitter(language)
        );
    }
    let detected = lst_editor::language::detect(Some(&PathBuf::from("example.txt")), None);
    assert_eq!(syntax_mode_for_language(detected), SyntaxMode::Plain);
}

#[test]
fn broad_highlighting_produces_spans_for_representative_languages() {
    let cases = [
        (
            SyntaxLanguage::Python,
            "value = \"\"\"first\nsecond\"\"\"\nprint(value)\n",
        ),
        (
            SyntaxLanguage::JavaScript,
            "/* first\nsecond */\nconst value = `template ${1}`;\n",
        ),
        (
            SyntaxLanguage::Jsx,
            "const element = <div className=\"editor\">{value}</div>;\n",
        ),
        (
            SyntaxLanguage::TypeScript,
            "interface Item { name: string }\nconst item: Item = { name: \"lst\" };\n",
        ),
        (
            SyntaxLanguage::Tsx,
            "const element: JSX.Element = <div className=\"editor\">{value}</div>;\n",
        ),
        (
            SyntaxLanguage::Json,
            "{\n  \"name\": \"lst\",\n  \"enabled\": true\n}\n",
        ),
        (SyntaxLanguage::Toml, "[package]\nname = \"lst\"\n"),
        (SyntaxLanguage::Yaml, "name: lst\nenabled: true\n"),
        (
            SyntaxLanguage::Markdown,
            "# Title\n\n```rust\nfn main() {}\n```\n",
        ),
        (
            SyntaxLanguage::Html,
            "<style>.editor { color: red; }</style>\n",
        ),
        (SyntaxLanguage::Css, ".editor { color: red; }\n"),
    ];

    for (language, source) in cases {
        let lines = compute_syntax_highlights(language, source);
        assert!(
            lines.iter().flatten().next().is_some(),
            "{language:?} should produce at least one syntax span"
        );
    }
}

#[test]
fn python_highlighting_keeps_multiline_string_context() {
    let lines = compute_syntax_highlights(
        SyntaxLanguage::Python,
        "value = \"\"\"first\nsecond\"\"\"\nprint(value)\n",
    );

    assert!(lines[0]
        .iter()
        .any(|span| span.color == theme_syntax::STRING));
    assert!(lines[1]
        .iter()
        .any(|span| span.color == theme_syntax::STRING));
}

#[test]
fn javascript_highlighting_keeps_multiline_comment_context() {
    let lines = compute_syntax_highlights(
        SyntaxLanguage::JavaScript,
        "/* first\nsecond */\nconst value = 1;\n",
    );

    assert!(lines[0]
        .iter()
        .any(|span| span.color == theme_syntax::COMMENT));
    assert!(lines[1]
        .iter()
        .any(|span| span.color == theme_syntax::COMMENT));
    assert!(lines[2]
        .iter()
        .all(|span| span.color != theme_syntax::COMMENT));
}

#[cfg(feature = "internal-invariants")]
#[test]
fn syntax_highlight_result_requires_matching_active_revision_and_language() {
    let rust_tab = tab_from_path(PathBuf::from("/tmp/example.rs"), "fn main() {}\n");
    let rust_tab_id = rust_tab.id();
    let rust_view = EditorTabView::new(&rust_tab);
    let rust_cache = rust_view.cache.clone();
    let rust_model = EditorModel::from_tab(rust_tab, "Ready.".to_string());
    let mut rust_store = TabViewStore::default();
    rust_store.views.insert(rust_tab_id, rust_view);
    let rust_key = SyntaxHighlightJobKey {
        language: SyntaxLanguage::Rust,
        revision: 0,
    };
    assert!(syntax_highlight_result_is_current(
        &rust_model,
        &rust_store,
        rust_tab_id,
        &rust_cache,
        rust_key
    ));

    let mut stale_tab = tab_from_path(PathBuf::from("/tmp/example.rs"), "fn main() {}\n");
    let stale_tab_id = stale_tab.id();
    let stale_view = EditorTabView::new(&stale_tab);
    let stale_cache = stale_view.cache.clone();
    stale_tab.replace_char_range(0..0, "// ");
    let stale_model = EditorModel::from_tab(stale_tab, "Ready.".to_string());
    let mut stale_store = TabViewStore::default();
    stale_store.views.insert(stale_tab_id, stale_view);
    assert!(!syntax_highlight_result_is_current(
        &stale_model,
        &stale_store,
        stale_tab_id,
        &stale_cache,
        rust_key
    ));

    let python_tab = tab_from_path(PathBuf::from("/tmp/example.py"), "print('lst')\n");
    let python_tab_id = python_tab.id();
    let python_view = EditorTabView::new(&python_tab);
    let python_cache = python_view.cache.clone();
    let python_model = EditorModel::from_tab(python_tab, "Ready.".to_string());
    let mut python_store = TabViewStore::default();
    python_store.views.insert(python_tab_id, python_view);
    assert!(!syntax_highlight_result_is_current(
        &python_model,
        &python_store,
        python_tab_id,
        &python_cache,
        rust_key
    ));
}

#[cfg(feature = "internal-invariants")]
#[test]
fn drag_autoscroll_delta_only_activates_at_viewport_edges() {
    let bounds = Bounds::new(point(px(0.0), px(100.0)), gpui::size(px(100.0), px(200.0)));

    assert!(drag_autoscroll_delta(point(px(50.0), px(90.0)), bounds, 1.0).is_some());
    assert!(drag_autoscroll_delta(point(px(50.0), px(310.0)), bounds, 1.0).is_some());
    assert!(drag_autoscroll_delta(point(px(50.0), px(200.0)), bounds, 1.0).is_none());
}

#[gpui::test]
fn zoom_actions_update_window_scale(cx: &mut TestAppContext) {
    let (view, cx) = new_test_app(cx, LaunchArgs::default());
    let default_rem_size = cx.update_window_entity(&view, |_app, window, _cx| window.rem_size());

    cx.dispatch_action(ZoomIn);
    cx.run_until_parked();
    let (zoomed_level, zoomed_rem_size) = cx.update_window_entity(&view, |app, window, _cx| {
        (app.zoom_level, window.rem_size())
    });
    assert_eq!(zoomed_level, 1);
    assert!(zoomed_rem_size > default_rem_size);

    cx.dispatch_action(ZoomReset);
    cx.run_until_parked();
    let (reset_level, reset_rem_size) = cx.update_window_entity(&view, |app, window, _cx| {
        (app.zoom_level, window.rem_size())
    });
    assert_eq!(reset_level, 0);
    assert_eq!(reset_rem_size, default_rem_size);
}

#[test]
fn ctrl_arrow_aliases_expand_vertical_selection() {
    assert!(has_binding::<SelectUp>("ctrl-up"));
    assert!(has_binding::<SelectDown>("ctrl-down"));
}

#[test]
fn standard_movement_keybindings_are_registered() {
    assert!(has_binding::<MoveWordLeft>("ctrl-left"));
    assert!(has_binding::<MoveWordRight>("ctrl-right"));
    assert!(has_binding::<SelectWordLeft>("ctrl-shift-left"));
    assert!(has_binding::<SelectWordRight>("ctrl-shift-right"));
    assert!(has_binding::<MoveSubwordLeft>("alt-left"));
    assert!(has_binding::<MoveSubwordRight>("alt-right"));
    assert!(has_binding::<SelectSubwordLeft>("alt-shift-left"));
    assert!(has_binding::<SelectSubwordRight>("alt-shift-right"));
    assert!(!has_binding::<MoveWordLeft>("alt-left"));
    assert!(!has_binding::<MoveWordRight>("alt-right"));
    assert!(!has_binding::<SelectWordLeft>("alt-shift-left"));
    assert!(!has_binding::<SelectWordRight>("alt-shift-right"));
    assert!(has_binding::<MovePageUp>("pageup"));
    assert!(has_binding::<MovePageDown>("pagedown"));
    assert!(has_binding::<SelectPageUp>("shift-pageup"));
    assert!(has_binding::<SelectPageDown>("shift-pagedown"));
    assert!(has_binding::<MoveDocumentStart>("ctrl-home"));
    assert!(has_binding::<MoveDocumentEnd>("ctrl-end"));
    assert!(has_binding::<SelectDocumentStart>("ctrl-shift-home"));
    assert!(has_binding::<SelectDocumentEnd>("ctrl-shift-end"));
    assert!(has_binding::<MoveSmartHome>("home"));
    assert!(has_binding::<MoveLineStart>("cmd-left"));
    assert!(!has_binding::<MoveLineStart>("home"));
    assert!(has_binding::<MoveLineEnd>("cmd-right"));
    assert!(has_binding::<SelectSmartHome>("shift-home"));
    assert!(has_binding::<SelectLineStart>("cmd-shift-left"));
    assert!(!has_binding::<SelectLineStart>("shift-home"));
    assert!(has_binding::<SelectLineEnd>("cmd-shift-right"));
    assert!(has_binding::<DeleteWordBackward>("ctrl-backspace"));
    assert!(has_binding::<DeleteWordBackward>("alt-backspace"));
    assert!(has_binding::<DeleteWordForward>("ctrl-delete"));
    assert!(has_binding::<DeleteWordForward>("alt-delete"));
}

#[test]
fn editor_keybindings_are_suppressed_while_inline_inputs_are_focused() {
    let editor_without_inline = &["Editor", "!", "InlineInput"];
    assert!(has_binding_context_containing::<MoveLeft>(
        "left",
        editor_without_inline
    ));
    assert!(has_binding_context_containing::<SelectDown>(
        "ctrl-down",
        editor_without_inline
    ));
    assert!(has_binding_context_containing::<CopySelection>(
        "ctrl-c",
        editor_without_inline
    ));
}

#[test]
fn zoom_keybindings_are_registered() {
    assert!(has_binding_in_context::<ZoomIn>("ctrl-=", "Workspace"));
    assert!(has_binding_in_context::<ZoomIn>("ctrl-+", "Workspace"));
    assert!(has_binding_in_context::<ZoomOut>("ctrl--", "Workspace"));
    assert!(has_binding_in_context::<ZoomReset>("ctrl-0", "Workspace"));
}

#[test]
fn find_shortcuts_stay_available_from_workspace_context() {
    assert!(has_binding_in_context::<FindOpen>("ctrl-f", "Workspace"));
    assert!(has_binding_in_context::<FindOpenReplace>(
        "ctrl-h",
        "Workspace"
    ));
    assert!(has_binding_in_context::<FindNext>("f3", "Workspace"));
    assert!(has_binding_in_context::<FindPrev>("shift-f3", "Workspace"));
    assert!(has_binding_in_context::<GotoLineOpen>(
        "ctrl-g",
        "Workspace"
    ));
}

#[cfg(feature = "internal-invariants")]
#[test]
fn wrapped_row_boundaries_assign_cursor_to_one_row() {
    let first = PaintedRow {
        row_top: px(0.0),
        line_start_char: 0,
        display_end_char: 5,
        logical_end_char: 5,
        cursor_end_inclusive: false,
        code_line: None,
        gutter_line: None,
    };
    let second = PaintedRow {
        row_top: px(0.0),
        line_start_char: 5,
        display_end_char: 10,
        logical_end_char: 10,
        cursor_end_inclusive: true,
        code_line: None,
        gutter_line: None,
    };

    assert!(!row_contains_cursor(&first, 5));
    assert!(row_contains_cursor(&second, 5));
}

#[cfg(feature = "internal-invariants")]
#[test]
fn eof_cursor_is_allowed_on_last_empty_row() {
    let row = PaintedRow {
        row_top: px(0.0),
        line_start_char: 0,
        display_end_char: 0,
        logical_end_char: 0,
        cursor_end_inclusive: true,
        code_line: None,
        gutter_line: None,
    };

    assert!(row_contains_cursor(&row, 0));
}
