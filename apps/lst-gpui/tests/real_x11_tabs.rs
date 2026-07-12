//! Real-display specs for tab ordering and active-tab selection.

mod support;

use support::{secs, EditorTestExt, TestResult};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn moving_active_tab_keeps_the_same_file_focused() -> TestResult {
    support::run_x11_test("tabs-move-active-keeps-file", |session| {
        let a = session.seed_file("tabs-a.txt", "a")?;
        let b = session.seed_file("tabs-b.txt", "b")?;
        let c = session.seed_file("tabs-c.txt", "c")?;
        let b_path = b.to_string_lossy().into_owned();
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
        let b_path = b.to_string_lossy().into_owned();
        let c_path = c.to_string_lossy().into_owned();
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

        editor.click_new_tab_button()?;
        editor.wait_state("new overflow tab active", secs(5), |record| {
            record.active_tab_index == files.len() && record.status_message == "Created a new scratchpad."
        })?;
        Ok(())
    })
}
