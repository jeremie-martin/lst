//! Real-display specs for opening files into tabs, tab ordering, and
//! active-tab selection.

mod support;

use std::fs;
use std::path::{Path, PathBuf};

use support::{path_text, secs, EditorTestExt, TestResult};

/// `metrics::TAB_MIN_WIDTH` at the default UI scale.
const TAB_MIN_WIDTH_PX: f32 = 96.0;

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn moving_active_tab_keeps_the_same_file_focused() -> TestResult {
    support::run_x11_test("tabs-move-active-keeps-file", |session| {
        let a = session.seed_file("tabs-a.txt", "a")?;
        let b = session.seed_file("tabs-b.txt", "b")?;
        let c = session.seed_file("tabs-c.txt", "c")?;
        let b_path = path_text(&b);
        let mut editor = session.open_files("tabs-move-active-keeps-file", &[a, b, c])?;

        editor.keys("<C-tab>")?;
        editor.wait_state("second tab active", secs(5), |record| {
            record.active_tab_path.as_deref() == Some(&b_path) && record.active_tab_index == 1
        })?;

        editor.keys("<C-S-pagedown>")?;
        editor.wait_state("active tab moved right", secs(5), |record| {
            record.active_tab_path.as_deref() == Some(&b_path) && record.active_tab_index == 2
        })?;

        editor.keys("<C-S-pageup>")?;
        editor.wait_state("active tab moved left", secs(5), |record| {
            record.active_tab_path.as_deref() == Some(&b_path) && record.active_tab_index == 1
        })?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn closing_last_tab_selects_left_neighbor() -> TestResult {
    support::run_x11_test("tabs-close-selects-left-neighbor", |session| {
        let a = session.seed_file("close-tabs-a.txt", "a")?;
        let b = session.seed_file("close-tabs-b.txt", "b")?;
        let c = session.seed_file("close-tabs-c.txt", "c")?;
        let b_path = path_text(&b);
        let c_path = path_text(&c);
        let mut editor = session.open_files("tabs-close-selects-left-neighbor", &[a, b, c])?;

        editor.keys("<C-tab><C-tab>")?;
        editor.wait_state("last tab active", secs(5), |record| {
            record.active_tab_path.as_deref() == Some(&c_path) && record.active_tab_index == 2
        })?;

        editor.keys("<C-w>")?;
        editor.wait_state("left neighbor active", secs(5), |record| {
            record.active_tab_path.as_deref() == Some(&b_path) && record.active_tab_index == 1
        })?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn new_tab_button_remains_usable_when_tabs_overflow() -> TestResult {
    support::run_x11_test("tabs-overflow-new-tab-visible", |session| {
        let files = (0..12)
            .map(|index| {
                session.seed_file(
                    &format!("overflow-tab-with-a-long-name-{index:02}.txt"),
                    &index.to_string(),
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut editor = session.open_files("tabs-overflow-new-tab-visible", &files)?;
        editor.resize(480, 400)?;

        // Tabs never shrink below their minimum width, so the strip between
        // the pinned start and end controls cannot fit them all.
        let record = editor.wait_state("tab strip controls laid out", secs(5), |record| {
            record.recent_button_bounds_px.is_some() && record.all_tabs_button_bounds_px.is_some()
        })?;
        let (recent_x, _, recent_w, _) = record.recent_button_bounds_px.ok_or("recent button bounds")?;
        let (all_tabs_x, _, _, _) = record.all_tabs_button_bounds_px.ok_or("all tabs button bounds")?;
        let strip_width = all_tabs_x - (recent_x + recent_w);
        let tabs_min_width = files.len() as f32 * TAB_MIN_WIDTH_PX;
        assert!(
            strip_width < tabs_min_width,
            "{} tabs need at least {tabs_min_width}px but the strip has {strip_width}px: {record:?}",
            files.len()
        );

        editor.click_new_tab_button()?;
        editor.wait_state("new overflow tab active", secs(5), |record| {
            record.active_tab_index == files.len() && record.status_message == "Created a new scratchpad."
        })?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn opening_a_missing_file_reports_it_and_falls_back_to_a_scratchpad() -> TestResult {
    support::run_x11_test("tabs-open-missing-file", |session| {
        let missing = session.root().join("missing.txt");
        expect_failed_open_falls_back_to_scratchpad(session, &missing)?;
        assert!(!missing.exists(), "a failed open must not create {}", missing.display());
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn opening_a_non_utf8_file_reports_it_and_falls_back_to_a_scratchpad() -> TestResult {
    support::run_x11_test("tabs-open-non-utf8-file", |session| {
        let binary = session.root().join("latin1.txt");
        fs::write(&binary, b"caf\xe9\n")?;
        expect_failed_open_falls_back_to_scratchpad(session, &binary)?;
        assert_eq!(fs::read(&binary)?, b"caf\xe9\n");
        Ok(())
    })
}

/// Launch with `path` as the only file: the status names the failure and
/// the window opens on an autosaved scratchpad instead.
fn expect_failed_open_falls_back_to_scratchpad(session: &mut support::ScratchpadSession, path: &Path) -> TestResult {
    let failure = format!("Failed to open {}", path_text(path));
    let requested = path_text(path);
    let mut editor = session.open_file("tabs-open-failure", path)?;
    let record = editor.wait_state("open failure reported", secs(5), |record| {
        record.status_message.starts_with(&failure)
            && record
                .active_tab_path
                .as_deref()
                .is_some_and(|active| active != requested)
    })?;
    let scratchpad = PathBuf::from(record.active_tab_path.ok_or("fallback tab has no path")?);
    editor.keys("fallback")?;
    editor.expect_file(&scratchpad, "fallback")?;
    Ok(())
}
