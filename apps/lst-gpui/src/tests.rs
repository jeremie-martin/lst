use crate::ui::{
    input_keybindings,
    scrollbar::{
        horizontal_scrollbar_layout, vertical_scrollbar_layout, HorizontalScrollbarLayout,
        VerticalScrollbarLayout,
    },
    theme::{SyntaxRole, ThemeId},
};
use gpui::{
    point, px, Bounds, ClipboardItem, Entity, EntityInputHandler, Keystroke, Modifiers,
    MouseButton, TestAppContext, VisualContext as _, VisualTestContext,
};
use lst_editor::Selection;
#[cfg(feature = "internal-invariants")]
use lst_editor::{EditorModel, EditorTab, TabId};
#[cfg(feature = "internal-invariants")]
use std::collections::HashMap;
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
    launch: LaunchArgs,
) -> (Entity<LstGpuiApp>, &mut VisualTestContext) {
    let (view, cx, _captured) = new_test_app_capturing(cx, launch);
    (view, cx)
}

fn new_test_app_capturing(
    cx: &mut TestAppContext,
    mut launch: LaunchArgs,
) -> (
    Entity<LstGpuiApp>,
    &mut VisualTestContext,
    crate::runtime::clipboard::CapturingExitClipboard,
) {
    if launch.files.is_empty() && launch.scratchpad_dir.is_none() {
        launch.scratchpad_dir = Some(temp_dir("scratchpads"));
    }
    cx.update(|cx| {
        cx.bind_keys(editor_keybindings());
        cx.bind_keys(input_keybindings());
    });
    let captured = crate::runtime::clipboard::CapturingExitClipboard::default();
    let (view, cx) = cx.add_window_view(|_, cx| LstGpuiApp::new(cx, launch));
    view.update(cx, |app, _| {
        app.exit_clipboard = std::sync::Arc::new(captured.clone());
    });
    cx.update(|window, cx| {
        window.focus(&view.read(cx).focus_handle);
        window.activate_window();
    });
    cx.run_until_parked();
    (view, cx, captured)
}

fn app_snapshot(view: &Entity<LstGpuiApp>, cx: &mut VisualTestContext) -> AppSnapshot {
    view.update(cx, |app, cx| app.snapshot(cx))
}

#[cfg(feature = "internal-invariants")]
fn assert_tab_views_match_model(snapshot: &AppSnapshot) {
    assert_eq!(snapshot.tab_view_ids, snapshot.model.tab_ids);
}

#[cfg(not(feature = "internal-invariants"))]
fn assert_tab_views_match_model(_snapshot: &AppSnapshot) {}

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
            .active_viewport_bounds()
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
        let obs = app
            .observable_cursor_viewport()
            .expect("cursor viewport should be observable after paint");
        (
            obs.scroll_top,
            obs.viewport_height,
            obs.row_height,
            obs.cursor_row,
            obs.max_offset,
            obs.total_rows,
        )
    })
}

fn active_editor_scrollbar_layout(
    view: &Entity<LstGpuiApp>,
    cx: &mut VisualTestContext,
) -> Option<VerticalScrollbarLayout> {
    view.update(cx, |app, _cx| {
        let bounds = app
            .active_viewport_bounds()
            .expect("viewport should have rendered bounds");
        let active_view = app.active_view();
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

fn active_editor_horizontal_scrollbar_layout(
    view: &Entity<LstGpuiApp>,
    cx: &mut VisualTestContext,
) -> Option<HorizontalScrollbarLayout> {
    view.update(cx, |app, _cx| {
        let bounds = app
            .active_viewport_bounds()
            .expect("viewport should have rendered bounds");
        let active_view = app.active_view();
        horizontal_scrollbar_layout(
            Bounds::new(
                point(
                    bounds.left(),
                    bounds.bottom() - app.ui_px(crate::ui::theme::metrics::SCROLLBAR_TRACK_WIDTH),
                ),
                gpui::size(
                    bounds.size.width,
                    app.ui_px(crate::ui::theme::metrics::SCROLLBAR_TRACK_WIDTH),
                ),
            ),
            crate::viewport::scroll_left_for(&active_view.scroll),
            active_view.scroll.max_offset().width.max(px(0.0)),
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
fn built_in_themes_cycle_between_dark_and_light() {
    assert_eq!(ThemeId::Dark.next(), ThemeId::Light);
    assert_eq!(ThemeId::Light.next(), ThemeId::Dark);

    let dark = ThemeId::Dark.theme();
    let light = ThemeId::Light.theme();
    assert_eq!(ThemeId::default(), ThemeId::Dark);
    assert_ne!(dark.role.editor_bg, light.role.editor_bg);
    assert_ne!(dark.role.text, light.role.text);
    assert_ne!(
        dark.syntax.color(SyntaxRole::Keyword),
        light.syntax.color(SyntaxRole::Keyword)
    );
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
fn launch_records_real_files_but_not_blank_startup_scratchpads(cx: &mut TestAppContext) {
    let dir = temp_dir("recent-launch");
    let recent_path = dir.join("recent");
    let file = dir.join("note.txt");
    std::fs::write(&file, "loaded").expect("write recent launch fixture");

    let (view, cx) = new_test_app(
        cx,
        LaunchArgs {
            files: vec![file.clone()],
            recent_files_path: Some(recent_path.clone()),
            ..LaunchArgs::default()
        },
    );
    let snapshot = app_snapshot(&view, cx);
    assert_eq!(
        snapshot.recent_paths,
        [crate::recent::normalize_recent_path(&file)]
    );

    let empty_recent_path = dir.join("empty-recent");
    let (view, cx) = new_test_app(
        cx,
        LaunchArgs {
            scratchpad_dir: Some(dir.join("scratchpads")),
            recent_files_path: Some(empty_recent_path),
            ..LaunchArgs::default()
        },
    );
    let snapshot = app_snapshot(&view, cx);
    assert!(snapshot.recent_paths.is_empty());

    std::fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[gpui::test]
fn test_launch_without_recent_path_does_not_persist_recent_state(cx: &mut TestAppContext) {
    let (view, _cx) = new_test_app(cx, LaunchArgs::default());

    view.update(_cx, |app, _| {
        assert!(!app.recent_files.is_persistent());
    });
}

#[gpui::test]
fn recent_files_view_searches_and_loads_more_paths(cx: &mut TestAppContext) {
    let dir = temp_dir("recent-overlay");
    let recent_path = dir.join("recent");
    let mut recent = crate::recent::RecentFiles::load(Some(recent_path.clone()));
    for index in 0..65 {
        let path = dir.join(format!("file-{index:02}.txt"));
        std::fs::write(&path, format!("body {index}")).expect("write recent fixture");
        recent.record(&path);
    }

    let (view, cx) = new_test_app(
        cx,
        LaunchArgs {
            recent_files_path: Some(recent_path),
            scratchpad_dir: Some(dir.join("scratchpads")),
            ..LaunchArgs::default()
        },
    );

    cx.dispatch_action(ToggleRecentFiles);
    cx.refresh().expect("render recent overlay");
    let snapshot = app_snapshot(&view, cx);
    assert!(snapshot.recent_panel_visible);
    assert_eq!(
        snapshot.recent_visible_paths.len(),
        crate::recent::RECENT_BATCH_SIZE
    );

    view.update(cx, |app, cx| app.load_more_recent_files(cx));
    let snapshot = app_snapshot(&view, cx);
    assert_eq!(snapshot.recent_visible_paths.len(), 65);

    cx.simulate_input("file-64");
    let snapshot = app_snapshot(&view, cx);
    assert_eq!(snapshot.recent_query_input, "file-64");
    assert_eq!(
        snapshot.recent_visible_paths,
        [crate::recent::normalize_recent_path(
            &dir.join("file-64.txt")
        )]
    );

    std::fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[gpui::test]
fn recent_search_keeps_focus_when_find_was_open(cx: &mut TestAppContext) {
    let dir = temp_dir("recent-focus");
    let recent_path = dir.join("recent");
    let file = dir.join("note.txt");
    std::fs::write(&file, "body").expect("write recent focus fixture");
    let mut recent = crate::recent::RecentFiles::load(Some(recent_path.clone()));
    recent.record(&file);

    let (view, cx) = new_test_app(
        cx,
        LaunchArgs {
            recent_files_path: Some(recent_path),
            scratchpad_dir: Some(dir.join("scratchpads")),
            ..LaunchArgs::default()
        },
    );

    cx.dispatch_action(FindOpen);
    cx.refresh().expect("render find overlay");
    cx.dispatch_action(ToggleRecentFiles);
    cx.refresh().expect("render recent view");
    cx.run_until_parked();
    cx.simulate_input("note");

    let snapshot = app_snapshot(&view, cx);
    assert!(snapshot.recent_panel_visible);
    assert_eq!(snapshot.recent_query_input, "note");
    assert_eq!(snapshot.find_query_input, "");

    std::fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[gpui::test]
fn recent_search_matches_file_content(cx: &mut TestAppContext) {
    let dir = temp_dir("recent-content-search");
    let recent_path = dir.join("recent");
    let file = dir.join("plain-name.txt");
    std::fs::write(&file, "alpha\nneedle in the body\nomega")
        .expect("write recent content fixture");
    let mut recent = crate::recent::RecentFiles::load(Some(recent_path.clone()));
    recent.record(&file);

    let (view, cx) = new_test_app(
        cx,
        LaunchArgs {
            recent_files_path: Some(recent_path),
            scratchpad_dir: Some(dir.join("scratchpads")),
            ..LaunchArgs::default()
        },
    );

    cx.dispatch_action(ToggleRecentFiles);
    cx.refresh().expect("render recent view");
    cx.run_until_parked();
    cx.simulate_input("needle");
    cx.run_until_parked();

    let snapshot = app_snapshot(&view, cx);
    assert_eq!(
        snapshot.recent_visible_paths,
        [crate::recent::normalize_recent_path(&file)]
    );

    std::fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[gpui::test]
fn recent_preview_prune_schedules_newly_visible_cards(cx: &mut TestAppContext) {
    let dir = temp_dir("recent-prune-preview");
    let recent_path = dir.join("recent");
    let missing = dir.join("missing.txt");
    let present = dir.join("present.txt");
    std::fs::write(&present, "present preview").expect("write present recent fixture");
    let mut recent = crate::recent::RecentFiles::load(Some(recent_path.clone()));
    recent.record(&present);
    recent.record(&missing);

    let (view, cx) = new_test_app(
        cx,
        LaunchArgs {
            recent_files_path: Some(recent_path),
            scratchpad_dir: Some(dir.join("scratchpads")),
            ..LaunchArgs::default()
        },
    );

    cx.dispatch_action(ToggleRecentFiles);
    cx.run_until_parked();

    view.update(cx, |app, _| {
        let present = crate::recent::normalize_recent_path(&present);
        assert!(matches!(
            app.recent_previews.get(&present),
            Some(RecentPreviewState::Loaded(text)) if text.contains("present preview")
        ));
    });

    std::fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[gpui::test]
fn recent_file_card_click_opens_the_file(cx: &mut TestAppContext) {
    let dir = temp_dir("recent-card-click");
    let recent_path = dir.join("recent");
    let file = dir.join("click-target.txt");
    std::fs::write(&file, "clicked").expect("write recent card click fixture");
    let mut recent = crate::recent::RecentFiles::load(Some(recent_path.clone()));
    recent.record(&file);

    let (view, cx) = new_test_app(
        cx,
        LaunchArgs {
            recent_files_path: Some(recent_path),
            scratchpad_dir: Some(dir.join("scratchpads")),
            ..LaunchArgs::default()
        },
    );
    refresh_and_flush_reveal(&view, cx, "initial editor before recent click");
    let bounds = view
        .update(cx, |app, _| app.active_viewport_bounds())
        .expect("editor viewport bounds before recent click");

    cx.dispatch_action(ToggleRecentFiles);
    cx.refresh().expect("render recent view before card click");
    cx.simulate_click(
        point(bounds.left() + px(36.0), bounds.top() + px(88.0)),
        Modifiers::default(),
    );
    cx.run_until_parked();

    let snapshot = app_snapshot(&view, cx);
    assert!(!snapshot.recent_panel_visible);
    assert_eq!(snapshot.model.active_path, Some(file.clone()));
    assert_eq!(snapshot.model.text, "clicked");

    std::fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[gpui::test]
fn open_recent_view_deselects_tabs_and_toggles_back(cx: &mut TestAppContext) {
    let dir = temp_dir("recent-toggle");
    let recent_path = dir.join("recent");
    let file = dir.join("note.txt");
    std::fs::write(&file, "note").expect("write recent toggle fixture");

    let (view, cx) = new_test_app(
        cx,
        LaunchArgs {
            files: vec![file.clone()],
            recent_files_path: Some(recent_path),
            ..LaunchArgs::default()
        },
    );

    cx.dispatch_action(ToggleRecentFiles);
    let snapshot = app_snapshot(&view, cx);
    assert!(snapshot.recent_panel_visible);
    assert_eq!(snapshot.model.active_path, Some(file.clone()));

    cx.dispatch_action(ToggleRecentFiles);
    let snapshot = app_snapshot(&view, cx);
    assert!(!snapshot.recent_panel_visible);
    assert_eq!(snapshot.model.active_path, Some(file));

    std::fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[gpui::test]
fn opening_recent_file_activates_existing_tab_or_opens_a_new_one(cx: &mut TestAppContext) {
    let dir = temp_dir("recent-open");
    let recent_path = dir.join("recent");
    let one = dir.join("one.txt");
    let two = dir.join("two.txt");
    let closed = dir.join("closed.txt");
    std::fs::write(&one, "one").expect("write one");
    std::fs::write(&two, "two").expect("write two");
    std::fs::write(&closed, "closed").expect("write closed");

    let (view, cx) = new_test_app(
        cx,
        LaunchArgs {
            files: vec![one.clone(), two.clone()],
            recent_files_path: Some(recent_path),
            ..LaunchArgs::default()
        },
    );
    let initial = app_snapshot(&view, cx);
    assert_eq!(initial.model.active_path, Some(one.clone()));

    view.update(cx, |app, cx| {
        app.open_recent_path(crate::recent::normalize_recent_path(&two), cx);
    });
    let snapshot = app_snapshot(&view, cx);
    assert_eq!(snapshot.model.active_path, Some(two.clone()));
    assert_eq!(snapshot.model.tab_count, 2);

    view.update(cx, |app, cx| {
        app.open_recent_path(crate::recent::normalize_recent_path(&closed), cx);
    });
    let snapshot = app_snapshot(&view, cx);
    assert_eq!(snapshot.model.active_path, Some(closed.clone()));
    assert_eq!(snapshot.model.tab_count, 3);

    std::fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[gpui::test]
fn opening_missing_recent_file_prunes_it(cx: &mut TestAppContext) {
    let dir = temp_dir("recent-missing");
    let recent_path = dir.join("recent");
    let missing = dir.join("missing.txt");
    let mut recent = crate::recent::RecentFiles::load(Some(recent_path.clone()));
    recent.record(&missing);

    let (view, cx) = new_test_app(
        cx,
        LaunchArgs {
            recent_files_path: Some(recent_path),
            scratchpad_dir: Some(dir.join("scratchpads")),
            ..LaunchArgs::default()
        },
    );

    view.update(cx, |app, cx| {
        app.open_recent_path(crate::recent::normalize_recent_path(&missing), cx);
    });
    let snapshot = app_snapshot(&view, cx);

    assert!(snapshot.recent_paths.is_empty());
    assert!(snapshot
        .model
        .status
        .contains(&format!("Failed to open {}", missing.display())));

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
            .active_viewport_bounds()
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
            .active_viewport_bounds()
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
fn editor_horizontal_scrollbar_drag_scrolls_without_text_selection(cx: &mut TestAppContext) {
    let dir = temp_dir("h-scrollbar-drag");
    let path = dir.join("wide.txt");
    let text = "x".repeat(2000);
    std::fs::write(&path, text).expect("write wide fixture");
    let (view, cx) = new_test_app(
        cx,
        LaunchArgs {
            files: vec![path],
            ..LaunchArgs::default()
        },
    );
    view.update(cx, |app, cx| {
        app.update_model(cx, true, |model| model.toggle_wrap());
    });
    cx.refresh().expect("first paint after wrap toggle");
    cx.run_until_parked();
    cx.refresh().expect("re-paint with content width");
    cx.run_until_parked();

    let layout = active_editor_horizontal_scrollbar_layout(&view, cx)
        .expect("wide editor should expose a horizontal scrollbar layout");
    let y = layout.thumb_bounds.top() + layout.thumb_bounds.size.height / 2.0;
    let start = point(layout.thumb_bounds.left() + px(2.0), y);
    let end = point(layout.track_bounds.right() - px(4.0), y);

    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(end, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());

    let (scroll_left, selection, active_text_drag, active_h_drag) = view.update(cx, |app, _cx| {
        (
            crate::viewport::scroll_left_for(&app.active_view().scroll),
            app.model.active_tab().selected_range(),
            app.selection_drag.is_some(),
            app.editor_horizontal_scrollbar_drag.is_some(),
        )
    });
    assert!(
        scroll_left > px(0.0),
        "dragging the horizontal thumb should move the editor scroll position"
    );
    assert_eq!(selection, 0..0);
    assert!(!active_text_drag);
    assert!(!active_h_drag);

    std::fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[gpui::test]
fn editor_horizontal_scrollbar_track_click_pages_without_text_selection(cx: &mut TestAppContext) {
    let dir = temp_dir("h-scrollbar-track");
    let path = dir.join("wide.txt");
    let text = "x".repeat(2000);
    std::fs::write(&path, text).expect("write wide fixture");
    let (view, cx) = new_test_app(
        cx,
        LaunchArgs {
            files: vec![path],
            ..LaunchArgs::default()
        },
    );
    view.update(cx, |app, cx| {
        app.update_model(cx, true, |model| model.toggle_wrap());
    });
    cx.refresh().expect("first paint after wrap toggle");
    cx.run_until_parked();
    cx.refresh().expect("re-paint with content width");
    cx.run_until_parked();

    let layout = active_editor_horizontal_scrollbar_layout(&view, cx)
        .expect("wide editor should expose a horizontal scrollbar layout");
    let click = point(
        (layout.thumb_bounds.right() + px(20.0)).min(layout.track_bounds.right() - px(1.0)),
        layout.thumb_bounds.top() + layout.thumb_bounds.size.height / 2.0,
    );

    cx.simulate_mouse_down(click, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_up(click, MouseButton::Left, Modifiers::default());

    let (scroll_left, selection, active_text_drag) = view.update(cx, |app, _cx| {
        (
            crate::viewport::scroll_left_for(&app.active_view().scroll),
            app.model.active_tab().selected_range(),
            app.selection_drag.is_some(),
        )
    });
    assert!(
        scroll_left > px(0.0),
        "clicking right of the horizontal thumb should page the editor right"
    );
    assert_eq!(selection, 0..0);
    assert!(!active_text_drag);

    std::fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[gpui::test]
fn editor_horizontal_scrollbar_is_absent_when_wrap_is_on(cx: &mut TestAppContext) {
    let dir = temp_dir("h-scrollbar-wrap-on");
    let path = dir.join("wide.txt");
    let text = "x".repeat(2000);
    std::fs::write(&path, text).expect("write wide fixture");
    let (view, cx) = new_test_app(
        cx,
        LaunchArgs {
            files: vec![path],
            ..LaunchArgs::default()
        },
    );
    cx.refresh().expect("render wide editor with wrap on");
    cx.run_until_parked();

    assert!(active_editor_horizontal_scrollbar_layout(&view, cx).is_none());

    std::fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[gpui::test]
fn editor_horizontal_scrollbar_is_absent_without_overflow(cx: &mut TestAppContext) {
    let dir = temp_dir("h-scrollbar-short");
    let path = dir.join("short.txt");
    std::fs::write(&path, "short\n").expect("write short fixture");
    let (view, cx) = new_test_app(
        cx,
        LaunchArgs {
            files: vec![path],
            ..LaunchArgs::default()
        },
    );
    view.update(cx, |app, cx| {
        app.update_model(cx, true, |model| model.toggle_wrap());
    });
    cx.refresh().expect("first paint after wrap toggle");
    cx.run_until_parked();
    cx.refresh().expect("re-paint after layout");
    cx.run_until_parked();

    assert!(active_editor_horizontal_scrollbar_layout(&view, cx).is_none());

    std::fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[gpui::test]
fn arrow_right_at_long_line_scrolls_horizontally_to_keep_cursor_in_sidescrolloff(
    cx: &mut TestAppContext,
) {
    let dir = temp_dir("h-scrollbar-reveal");
    let path = dir.join("wide.txt");
    let text = "x".repeat(2000);
    std::fs::write(&path, text).expect("write wide fixture");
    let (view, cx) = new_test_app(
        cx,
        LaunchArgs {
            files: vec![path],
            ..LaunchArgs::default()
        },
    );
    view.update(cx, |app, cx| {
        app.update_model(cx, true, |model| model.toggle_wrap());
    });
    cx.refresh().expect("first paint after wrap toggle");
    cx.run_until_parked();
    cx.refresh().expect("re-paint with content width");
    cx.run_until_parked();

    view.update(cx, |app, cx| {
        app.update_model(cx, true, |model| model.move_to_char(1500, false, None));
    });
    refresh_and_flush_reveal(&view, cx, "horizontal-cursor-reveal");

    let scroll_left = view.update(cx, |app, _| {
        crate::viewport::scroll_left_for(&app.active_view().scroll)
    });
    assert!(
        scroll_left > px(0.0),
        "moving the cursor far right should scroll horizontally to keep it visible"
    );

    std::fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[gpui::test]
fn horizontal_scroll_uses_rendered_glyph_width(cx: &mut TestAppContext) {
    let dir = temp_dir("h-scrollbar-glyphs");
    let path = dir.join("glyphs.txt");
    let text = "\u{1f642}".repeat(320);
    let glyph_count = text.chars().count();
    std::fs::write(&path, &text).expect("write glyph fixture");
    let (view, cx) = new_test_app(
        cx,
        LaunchArgs {
            files: vec![path],
            ..LaunchArgs::default()
        },
    );
    view.update(cx, |app, cx| {
        app.update_model(cx, true, |model| model.toggle_wrap());
    });
    cx.refresh().expect("first paint after wrap toggle");
    cx.run_until_parked();
    cx.refresh().expect("re-paint with rendered tab width");
    cx.run_until_parked();

    let (count_based_overflow, char_width, max_offset) = view.update(cx, |app, _cx| {
        let bounds = app
            .active_viewport_bounds()
            .expect("viewport should have rendered bounds");
        let geometry = app.active_view().geometry.borrow();
        let char_width = geometry.painted_char_width;
        let count_based_width = code_origin_pad(app.model.show_gutter(), app.ui_scale())
            + char_width * (glyph_count as f32 + 2.0);
        (
            (count_based_width - bounds.size.width).max(px(0.0)),
            char_width,
            app.active_view().scroll.max_offset().width.max(px(0.0)),
        )
    });
    assert!(
        max_offset > count_based_overflow + char_width * 100.0,
        "wide-glyph lines should expose horizontal overflow beyond character-count sizing: max_offset={}, count_based_overflow={}, char_width={}",
        max_offset / px(1.0),
        count_based_overflow / px(1.0),
        char_width / px(1.0)
    );

    view.update(cx, |app, cx| {
        app.update_model(cx, true, |model| {
            model.move_to_char(glyph_count, false, None)
        });
    });
    refresh_and_flush_reveal(&view, cx, "horizontal-glyph-reveal");

    let scroll_left = view.update(cx, |app, _| {
        crate::viewport::scroll_left_for(&app.active_view().scroll)
    });
    assert!(
        scroll_left > count_based_overflow + char_width * 100.0,
        "cursor reveal should be able to scroll to rendered glyph positions"
    );

    std::fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[gpui::test]
fn clicking_horizontally_scrolled_text_hits_visible_column(cx: &mut TestAppContext) {
    let dir = temp_dir("h-scroll-hit-test");
    let path = dir.join("wide.txt");
    let text = "x".repeat(2000);
    std::fs::write(&path, text).expect("write wide fixture");
    let (view, cx) = new_test_app(
        cx,
        LaunchArgs {
            files: vec![path],
            ..LaunchArgs::default()
        },
    );
    view.update(cx, |app, cx| {
        app.update_model(cx, true, |model| model.toggle_wrap());
    });
    cx.refresh().expect("first paint after wrap toggle");
    cx.run_until_parked();
    cx.refresh().expect("re-paint with content width");
    cx.run_until_parked();

    view.update(cx, |app, cx| {
        app.update_model(cx, true, |model| model.move_to_char(1500, false, None));
    });
    refresh_and_flush_reveal(&view, cx, "horizontal-hit-test-reveal");

    let (click, first_visible_col) = view.update(cx, |app, _cx| {
        let bounds = app
            .active_viewport_bounds()
            .expect("viewport should have rendered bounds");
        let rows = app.active_painted_rows();
        let row = rows.first().expect("wide editor should paint a row");
        let geometry = app.active_view().geometry.borrow();
        let char_width = geometry.painted_char_width;
        let scroll_left = crate::viewport::scroll_left_for(&app.active_view().scroll);
        assert!(
            scroll_left > char_width * 100.0,
            "precondition: cursor reveal should scroll far enough to expose the bug"
        );
        let first_visible_col = ((scroll_left / px(1.0)) / (char_width / px(1.0))).floor();
        let code_origin_x =
            bounds.left() + code_origin_pad(app.model.show_gutter(), app.ui_scale());
        (
            point(
                code_origin_x + char_width * 10.0,
                row.row_top + app.ui_px(crate::ui::theme::metrics::ROW_HEIGHT) / 2.0,
            ),
            first_visible_col as usize,
        )
    });

    cx.simulate_mouse_down(click, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_up(click, MouseButton::Left, Modifiers::default());

    let snapshot = app_snapshot(&view, cx);
    assert!(
        snapshot.model.cursor >= first_visible_col,
        "click should land in the horizontally visible text, not near the unscrolled line start"
    );

    std::fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[gpui::test]
fn toggling_wrap_back_on_resets_horizontal_scroll(cx: &mut TestAppContext) {
    let dir = temp_dir("h-scroll-reset");
    let path = dir.join("wide.txt");
    let text = "x".repeat(2000);
    std::fs::write(&path, text).expect("write wide fixture");
    let (view, cx) = new_test_app(
        cx,
        LaunchArgs {
            files: vec![path],
            ..LaunchArgs::default()
        },
    );
    view.update(cx, |app, cx| {
        app.update_model(cx, true, |model| model.toggle_wrap());
    });
    cx.refresh().expect("first paint after wrap toggle");
    cx.run_until_parked();
    cx.refresh().expect("re-paint with content width");
    cx.run_until_parked();

    view.update(cx, |app, cx| {
        app.update_model(cx, true, |model| model.move_to_char(1500, false, None));
    });
    refresh_and_flush_reveal(&view, cx, "wrap-reset-reveal");

    let scrolled_left = view.update(cx, |app, _| {
        crate::viewport::scroll_left_for(&app.active_view().scroll)
    });
    assert!(
        scrolled_left > px(0.0),
        "preconditions: cursor reveal should have scrolled right"
    );

    view.update(cx, |app, cx| {
        app.update_model(cx, true, |model| model.toggle_wrap());
    });
    cx.refresh().expect("paint after toggling wrap back on");
    cx.run_until_parked();

    let after_toggle = view.update(cx, |app, _| {
        crate::viewport::scroll_left_for(&app.active_view().scroll)
    });
    assert_eq!(
        after_toggle,
        px(0.0),
        "toggling wrap back on should clear the stale horizontal scroll offset"
    );

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
    assert_eq!(snapshot.model.selection.range(), 0..0);
    assert_tab_views_match_model(&snapshot);

    cx.simulate_keystrokes("escape");
    let snapshot = app_snapshot(&view, cx);
    assert!(!snapshot.model.find_visible);
    assert_eq!(snapshot.focus_target, FocusTarget::Editor);

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
            model.set_selection(Selection::from_range(0..3, false));
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
    assert_eq!(
        snapshot.model.selection.range(),
        expected_cursor..expected_cursor
    );
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
            .active_viewport_bounds()
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
    assert_eq!(
        snapshot.model.selection.range(),
        expected_cursor..expected_cursor
    );
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
        let bounds = app.active_viewport_bounds().expect("viewport bounds");
        let rows = app.active_painted_rows();

        let char_width = crate::viewport::code_char_width(window, app.ui_scale(), app.theme());
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
        let theme = app.theme();
        let style_for = |text: &str| {
            [gpui::TextRun {
                len: text.len(),
                font: font.clone(),
                color: gpui::rgb(theme.role.text).into(),
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
        let bounds = app.active_viewport_bounds().expect("viewport bounds");
        let rows = app.active_painted_rows();

        let wrap_columns = app.active_wrap_columns(window);
        let char_width =
            crate::viewport::code_char_width(window, app.ui_scale(), app.theme()) / px(1.0);
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

// Seeds the wrap-layout cache directly; demoted from the blind-refactor gate.
#[cfg(feature = "internal-invariants")]
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
fn closing_dirty_tab_silently_saves_then_closes(cx: &mut TestAppContext) {
    let dir = temp_dir("close-save");
    let first = dir.join("first.txt");
    let second = dir.join("second.txt");
    std::fs::write(&first, "old").expect("write first close-save fixture");
    std::fs::write(&second, "keep").expect("write second close-save fixture");
    let (view, cx) = new_test_app(
        cx,
        LaunchArgs {
            files: vec![first.clone(), second.clone()],
            ..LaunchArgs::default()
        },
    );
    cx.update_window_entity(&view, |app, window, cx| {
        app.replace_text_in_range(None, "new ", window, cx);
    });

    let snapshot = app_snapshot(&view, cx);
    assert_eq!(snapshot.model.active_path.as_deref(), Some(first.as_path()));
    let active_index = snapshot.model.active;
    view.update(cx, |app, cx| {
        app.request_close_tab_at(active_index, cx);
    });
    cx.run_until_parked();

    assert_eq!(
        std::fs::read_to_string(&first).expect("read silently saved close file"),
        "new old"
    );
    let snapshot = app_snapshot(&view, cx);
    assert_eq!(snapshot.model.tab_count, 1);
    assert_eq!(
        snapshot.model.active_path.as_deref(),
        Some(second.as_path())
    );

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
fn quit_silently_saves_dirty_file_tabs(cx: &mut TestAppContext) {
    let dir = temp_dir("quit-save");
    let path = dir.join("note.txt");
    std::fs::write(&path, "old").expect("write quit-save fixture");
    let (view, cx, captured) = new_test_app_capturing(
        cx,
        LaunchArgs {
            files: vec![path.clone()],
            ..LaunchArgs::default()
        },
    );
    cx.update_window_entity(&view, |app, window, cx| {
        app.replace_text_in_range(None, "new ", window, cx);
    });

    view.update(cx, |app, cx| {
        app.request_quit(cx);
    });
    cx.run_until_parked();

    assert_eq!(
        std::fs::read_to_string(&path).expect("read quit-save file"),
        "new old"
    );
    assert_eq!(
        captured.persisted.lock().unwrap().as_slice(),
        ["new old".to_string()].as_slice(),
    );

    std::fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[gpui::test]
fn quit_silently_truncates_emptied_file_tab(cx: &mut TestAppContext) {
    let dir = temp_dir("quit-truncate");
    let path = dir.join("note.txt");
    std::fs::write(&path, "old contents").expect("write quit-truncate fixture");
    let (view, cx) = new_test_app(
        cx,
        LaunchArgs {
            files: vec![path.clone()],
            ..LaunchArgs::default()
        },
    );
    cx.dispatch_action(SelectAll);
    cx.update_window_entity(&view, |app, window, cx| {
        app.replace_text_in_range(None, "", window, cx);
    });
    assert_eq!(app_snapshot(&view, cx).model.text, "");

    view.update(cx, |app, cx| {
        app.request_quit(cx);
    });
    cx.run_until_parked();

    assert_eq!(
        std::fs::read_to_string(&path).expect("read truncated file"),
        ""
    );

    std::fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[gpui::test]
fn quit_silently_saves_dirty_scratchpad_tabs(cx: &mut TestAppContext) {
    let dir = temp_dir("quit-save-scratchpad");
    let (view, cx, captured) = new_test_app_capturing(
        cx,
        LaunchArgs {
            scratchpad_dir: Some(dir.clone()),
            ..LaunchArgs::default()
        },
    );
    cx.update_window_entity(&view, |app, window, cx| {
        app.replace_text_in_range(None, "scratch text", window, cx);
    });
    let path = app_snapshot(&view, cx)
        .model
        .active_path
        .expect("scratchpad should be path backed");

    view.update(cx, |app, cx| {
        app.request_quit(cx);
    });
    cx.run_until_parked();

    assert_eq!(
        std::fs::read_to_string(&path).expect("read saved scratchpad"),
        "scratch text"
    );
    assert_eq!(
        captured.persisted.lock().unwrap().as_slice(),
        ["scratch text".to_string()].as_slice(),
    );

    std::fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[gpui::test]
fn quit_persists_active_text_to_system_clipboard(cx: &mut TestAppContext) {
    let dir = temp_dir("quit-copy");
    let path = dir.join("note.txt");
    std::fs::write(&path, "quit text").expect("write quit-copy fixture");
    let (view, cx, captured) = new_test_app_capturing(
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
        captured.persisted.lock().unwrap().as_slice(),
        ["quit text".to_string()].as_slice(),
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

    assert!(lines[0].iter().any(|span| span.role == SyntaxRole::Comment));
    assert!(lines[1].iter().any(|span| span.role == SyntaxRole::Comment));
    assert!(lines[2].iter().all(|span| span.role != SyntaxRole::Comment));
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
fn injected_grammars_are_not_root_syntax_modes() {
    assert_eq!(
        SyntaxLanguage::ALL,
        &[
            SyntaxLanguage::Rust,
            SyntaxLanguage::Python,
            SyntaxLanguage::JavaScript,
            SyntaxLanguage::Jsx,
            SyntaxLanguage::TypeScript,
            SyntaxLanguage::Tsx,
            SyntaxLanguage::Json,
            SyntaxLanguage::Toml,
            SyntaxLanguage::Yaml,
            SyntaxLanguage::Markdown,
            SyntaxLanguage::Html,
            SyntaxLanguage::Css,
        ]
    );
    assert_eq!(
        SyntaxLanguage::from_language(lst_editor::Language::Markdown),
        Some(SyntaxLanguage::Markdown)
    );
}

#[test]
fn supported_syntax_languages_have_role_contracts() {
    struct Contract {
        language: SyntaxLanguage,
        source: &'static str,
        roles: &'static [SyntaxRole],
    }

    let contracts = [
        Contract {
            language: SyntaxLanguage::Rust,
            source: "fn main() { let value = \"lst\"; }\n",
            roles: &[
                SyntaxRole::Keyword,
                SyntaxRole::Function,
                SyntaxRole::String,
            ],
        },
        Contract {
            language: SyntaxLanguage::Python,
            source: "def main():\n    value = \"lst\"\n",
            roles: &[
                SyntaxRole::Keyword,
                SyntaxRole::Function,
                SyntaxRole::String,
            ],
        },
        Contract {
            language: SyntaxLanguage::JavaScript,
            source: "function run() { const value = \"lst\"; return value; }\n",
            roles: &[
                SyntaxRole::Keyword,
                SyntaxRole::Function,
                SyntaxRole::String,
            ],
        },
        Contract {
            language: SyntaxLanguage::Jsx,
            source: "const element = <div className=\"editor\">{value}</div>;\n",
            roles: &[SyntaxRole::Tag, SyntaxRole::Property, SyntaxRole::String],
        },
        Contract {
            language: SyntaxLanguage::TypeScript,
            source: "interface Item { name: string }\nconst item: Item = { name: \"lst\" };\n",
            roles: &[SyntaxRole::Keyword, SyntaxRole::Type],
        },
        Contract {
            language: SyntaxLanguage::Tsx,
            source: "const element: JSX.Element = <div className=\"editor\">{value}</div>;\n",
            roles: &[SyntaxRole::Tag, SyntaxRole::Type, SyntaxRole::Property],
        },
        Contract {
            language: SyntaxLanguage::Json,
            source: "{\n  \"name\": \"lst\",\n  \"enabled\": true\n}\n",
            roles: &[SyntaxRole::String, SyntaxRole::Constant],
        },
        Contract {
            language: SyntaxLanguage::Toml,
            source: "[package]\nname = \"lst\"\n",
            roles: &[SyntaxRole::Property, SyntaxRole::String],
        },
        Contract {
            language: SyntaxLanguage::Yaml,
            source: "name: lst\nenabled: true\n",
            roles: &[SyntaxRole::Property, SyntaxRole::Constant],
        },
        Contract {
            language: SyntaxLanguage::Markdown,
            source: "# Title\n",
            roles: &[SyntaxRole::Title],
        },
        Contract {
            language: SyntaxLanguage::Html,
            source: "<a href=\"https://example.test\">link</a>\n",
            roles: &[SyntaxRole::Tag, SyntaxRole::Property, SyntaxRole::String],
        },
        Contract {
            language: SyntaxLanguage::Css,
            source: ".editor::before { content: \"lst\"; }\n",
            roles: &[SyntaxRole::Property, SyntaxRole::String],
        },
    ];

    assert_eq!(contracts.len(), SyntaxLanguage::ALL.len());
    for language in SyntaxLanguage::ALL {
        assert!(
            contracts
                .iter()
                .any(|contract| contract.language == *language),
            "{language:?} needs a syntax highlight contract"
        );
    }

    for contract in contracts {
        let lines = compute_syntax_highlights(contract.language, contract.source);
        let roles: Vec<SyntaxRole> = lines.iter().flatten().map(|span| span.role).collect();
        for role in contract.roles {
            assert!(
                roles.contains(role),
                "{:?} should produce {role:?}; got {roles:?}",
                contract.language
            );
        }
    }
}

#[test]
fn markdown_highlighting_includes_inline_markup() {
    let lines = compute_syntax_highlights(
        SyntaxLanguage::Markdown,
        "**Correctness by construction.** Use `TabSet` and [docs](https://example.test).\n",
    );

    assert!(lines[0].iter().any(|span| span.role == SyntaxRole::Strong));
    assert!(lines[0].iter().any(|span| span.role == SyntaxRole::Literal));
    assert!(lines[0]
        .iter()
        .any(|span| span.role == SyntaxRole::Reference));
}

#[test]
fn markdown_highlighting_includes_fenced_code_injections() {
    let lines = compute_syntax_highlights(SyntaxLanguage::Markdown, "```rust\nfn main() {}\n```\n");
    let roles: Vec<SyntaxRole> = lines.iter().flatten().map(|span| span.role).collect();

    assert!(lines[1].iter().any(|span| span.role == SyntaxRole::Keyword));
    assert!(roles.contains(&SyntaxRole::Literal), "{roles:?}");
}

#[test]
fn python_highlighting_keeps_multiline_string_context() {
    let lines = compute_syntax_highlights(
        SyntaxLanguage::Python,
        "value = \"\"\"first\nsecond\"\"\"\nprint(value)\n",
    );

    assert!(lines[0].iter().any(|span| span.role == SyntaxRole::String));
    assert!(lines[1].iter().any(|span| span.role == SyntaxRole::String));
}

#[test]
fn javascript_highlighting_keeps_multiline_comment_context() {
    let lines = compute_syntax_highlights(
        SyntaxLanguage::JavaScript,
        "/* first\nsecond */\nconst value = 1;\n",
    );

    assert!(lines[0].iter().any(|span| span.role == SyntaxRole::Comment));
    assert!(lines[1].iter().any(|span| span.role == SyntaxRole::Comment));
    assert!(lines[2].iter().all(|span| span.role != SyntaxRole::Comment));
}

#[cfg(feature = "internal-invariants")]
#[test]
fn syntax_highlight_result_requires_matching_active_revision_and_language() {
    let rust_tab = tab_from_path(PathBuf::from("/tmp/example.rs"), "fn main() {}\n");
    let rust_tab_id = rust_tab.id();
    let rust_view = EditorTabView::new(&rust_tab);
    let rust_cache = rust_view.cache.clone();
    let rust_model = EditorModel::from_tab(rust_tab, "Ready.".to_string());
    let mut rust_store: HashMap<TabId, EditorTabView> = HashMap::new();
    rust_store.insert(rust_tab_id, rust_view);
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
    let mut stale_store: HashMap<TabId, EditorTabView> = HashMap::new();
    stale_store.insert(stale_tab_id, stale_view);
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
    let mut python_store: HashMap<TabId, EditorTabView> = HashMap::new();
    python_store.insert(python_tab_id, python_view);
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

#[gpui::test]
fn toggle_theme_changes_only_runtime_theme(cx: &mut TestAppContext) {
    let (view, cx) = new_test_app(cx, LaunchArgs::default());
    let initial = app_snapshot(&view, cx);
    assert_eq!(initial.theme_id, ThemeId::Dark);

    cx.dispatch_action(ToggleTheme);
    cx.run_until_parked();
    let light = app_snapshot(&view, cx);
    assert_eq!(light.theme_id, ThemeId::Light);
    assert_eq!(light.model.text, initial.model.text);
    assert_eq!(light.model.tab_ids, initial.model.tab_ids);
    assert_eq!(light.focus_target, initial.focus_target);

    cx.dispatch_action(ToggleTheme);
    cx.run_until_parked();
    assert_eq!(app_snapshot(&view, cx).theme_id, ThemeId::Dark);
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
