//! Real-display specs for recent-files panel behavior. These cover user-visible
//! recent workflows through the production app and state trace instead of GPUI
//! test-context snapshots.

mod support;

use std::path::Path;

use support::{secs, EditorTestExt, TestResult};

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn recent_panel_path_query_opens_matching_file() -> TestResult {
    support::run_x11_test("recent-path-query", |session| {
        let alpha = session.seed_file("alpha-note.txt", "alpha\n")?;
        let target = session.seed_file("needle-name.txt", "target\n")?;
        session.seed_recent_files(&[alpha, target.clone()])?;
        let target_text = path_text(&target);

        let (mut editor, _scratchpad) = session.open("recent-path-query")?;
        editor.keys("<C-r>")?;
        editor.wait_state("recent query focus", secs(5), |record| {
            record.recent_panel_open && record.focused_input == "recent_query"
        })?;

        editor.keys("needle")?;
        editor.wait_state("recent path filter selected target", secs(5), |record| {
            record.recent_panel_open
                && record.recent_panel_query.as_deref() == Some("needle")
                && record.recent_panel_selected_path.as_deref() == Some(target_text.as_str())
        })?;

        editor.keys("<enter>")?;
        editor.wait_state("recent target opened", secs(5), |record| {
            !record.recent_panel_open
                && record.active_tab_path.as_deref() == Some(target_text.as_str())
        })?;
        editor.keys("X")?;
        editor.save_then_expect_file(&target, "Xtarget\n")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn recent_panel_keyboard_selection_opens_selected_file() -> TestResult {
    support::run_x11_test("recent-keyboard-selection", |session| {
        let one = session.seed_file("one.txt", "one\n")?;
        let two = session.seed_file("two.txt", "two\n")?;
        let three = session.seed_file("three.txt", "three\n")?;
        session.seed_recent_files(&[one.clone(), two.clone(), three])?;
        let one_text = path_text(&one);
        let two_text = path_text(&two);

        let (mut editor, _scratchpad) = session.open("recent-keyboard-selection")?;
        editor.keys("<C-r>")?;
        editor.wait_state("recent initial selection", secs(5), |record| {
            record.recent_panel_open
                && record.focused_input == "recent_query"
                && record.recent_panel_selected_path.as_deref() == Some(one_text.as_str())
        })?;

        editor.keys("<tab>")?;
        editor.wait_state("recent second selection", secs(5), |record| {
            record.recent_panel_open
                && record.recent_panel_selected_path.as_deref() == Some(two_text.as_str())
        })?;

        editor.keys("<enter>")?;
        editor.wait_state("recent second file opened", secs(5), |record| {
            !record.recent_panel_open
                && record.active_tab_path.as_deref() == Some(two_text.as_str())
        })?;
        editor.keys("X")?;
        editor.save_then_expect_file(&two, "Xtwo\n")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn recent_panel_content_query_opens_file_matching_body() -> TestResult {
    support::run_x11_test("recent-content-query", |session| {
        let target = session.seed_file("plain-name.txt", "alpha\nneedle in the body\nomega\n")?;
        session.seed_recent_files(std::slice::from_ref(&target))?;
        let target_text = path_text(&target);

        let (mut editor, _scratchpad) = session.open("recent-content-query")?;
        editor.keys("<C-r>")?;
        editor.wait_state("recent query focus", secs(5), |record| {
            record.recent_panel_open && record.focused_input == "recent_query"
        })?;

        editor.keys("needle")?;
        editor.wait_state("recent content search pending", secs(5), |record| {
            record.recent_panel_open
                && record.recent_panel_query.as_deref() == Some("needle")
                && record.recent_panel_content_search_pending
        })?;
        editor.wait_state(
            "recent content filter selected target",
            secs(10),
            |record| {
                record.recent_panel_open
                    && record.recent_panel_query.as_deref() == Some("needle")
                    && !record.recent_panel_content_search_pending
                    && record.recent_panel_selected_path.as_deref() == Some(target_text.as_str())
            },
        )?;

        editor.keys("<enter>")?;
        editor.wait_state("recent content match opened", secs(5), |record| {
            !record.recent_panel_open
                && record.active_tab_path.as_deref() == Some(target_text.as_str())
        })?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn recent_panel_empty_states_are_visible() -> TestResult {
    support::run_x11_test("recent-empty-states", |session| {
        let (mut editor, _scratchpad) = session.open("recent-empty-history")?;
        editor.keys("<C-r>")?;
        editor.wait_state("recent empty history", secs(5), |record| {
            record.recent_panel_open
                && record.recent_panel_empty_message.as_deref() == Some("No recent files")
        })?;
        editor.keys("<escape>")?;
        editor.wait_state("recent closed", secs(5), |record| !record.recent_panel_open)?;

        drop(editor);

        let file = session.seed_file("history.txt", "history body\n")?;
        session.seed_recent_files(&[file])?;
        let (mut editor, _scratchpad) = session.open("recent-empty-query")?;
        editor.keys("<C-r>missing")?;
        editor.wait_state("recent query miss", secs(5), |record| {
            record.recent_panel_open
                && record.recent_panel_query.as_deref() == Some("missing")
                && record.recent_panel_empty_message.as_deref()
                    == Some("No matches for \"missing\"")
        })?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn opening_missing_recent_file_prunes_it_from_the_panel() -> TestResult {
    support::run_x11_test("recent-missing-prune", |session| {
        let missing = session.root().join("missing.txt");
        session.seed_recent_files(std::slice::from_ref(&missing))?;
        let missing_text = path_text(&missing);

        let (mut editor, _scratchpad) = session.open("recent-missing-prune")?;
        editor.keys("<C-r>")?;
        editor.wait_state("missing recent selected", secs(5), |record| {
            record.recent_panel_open
                && record.recent_panel_selected_path.as_deref() == Some(missing_text.as_str())
        })?;

        editor.keys("<enter>")?;
        editor.wait_state("missing recent pruned", secs(5), |record| {
            record.recent_panel_open
                && record.recent_panel_selected_path.is_none()
                && record.recent_panel_empty_message.as_deref() == Some("No recent files")
        })?;
        Ok(())
    })
}
