//! Real-display specs for files changed or deleted by another program while
//! they are open: clean tabs reload in place, dirty tabs get a per-tab banner
//! that never blocks editing, and the banner's actions decide which version
//! wins.

mod support;

use std::fs;

use support::{path_text, secs, EditorTestExt, FileConflictAction, TestResult};

#[test]
#[ignore = "requires a real X11 display"]
fn external_change_uses_a_non_modal_per_tab_banner_and_dismiss_is_version_scoped() -> TestResult {
    support::run_x11_test("external-conflict-banner", |session| {
        let path = session.seed_file("external-conflict.txt", "original\n")?;
        let identity = path_text(&path);
        let mut editor = session.open_file("external-conflict", &path)?;

        editor.keys("local edit")?;
        fs::write(&path, "external version\n")?;
        let banner = editor.wait_state("external conflict banner", secs(4), |record| {
            record.file_conflict_path.as_deref() == Some(identity.as_str())
                && record.file_conflict_button_bounds_px.dismiss.is_some()
        })?;

        // Non-modal: typing still edits the document under the banner.
        editor.keys("Z")?;
        let edited = editor.wait_state("edit under the banner", secs(2), |record| {
            record.revision > banner.revision && record.file_conflict_path.as_deref() == Some(identity.as_str())
        })?;

        // Per-tab: another tab has no banner, and returning shows it again.
        editor.keys("<C-n>")?;
        editor.wait_state("new tab without banner", secs(2), |record| {
            record.active_tab_id != edited.active_tab_id && record.file_conflict_path.is_none()
        })?;
        editor.keys("<C-S-tab>")?;
        editor.wait_state("banner back on its tab", secs(2), |record| {
            record.active_tab_id == edited.active_tab_id
                && record.file_conflict_path.as_deref() == Some(identity.as_str())
        })?;

        editor.click_file_conflict_action(FileConflictAction::Dismiss)?;
        editor.wait_state("conflict dismissed", secs(2), |record| {
            record.file_conflict_path.is_none() && record.active_tab_modified
        })?;

        // Version-scoped: Dismiss covers only the version it was shown for.
        fs::write(&path, "second external version\n")?;
        editor.wait_state("banner for the next external version", secs(4), |record| {
            record.file_conflict_path.as_deref() == Some(identity.as_str())
        })?;
        editor.click_file_conflict_action(FileConflictAction::Dismiss)?;
        editor.wait_state("second version dismissed", secs(2), |record| {
            record.file_conflict_path.is_none() && record.active_tab_modified
        })?;

        // A later explicit Save is the user's intentional overwrite.
        editor.save_then_expect_file(&path, "original\nlocal editZ")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display"]
fn external_change_banner_reload_action_replaces_local_edits() -> TestResult {
    support::run_x11_test("external-conflict-reload", |session| {
        let path = session.seed_file("external-reload.txt", "original\n")?;
        let identity = path_text(&path);
        let mut editor = session.open_file("external-reload", &path)?;

        editor.keys("local edit")?;
        fs::write(&path, "disk wins\nsecond line\n")?;
        editor.wait_state("external conflict banner", secs(4), |record| {
            record.file_conflict_path.as_deref() == Some(identity.as_str())
                && record.file_conflict_button_bounds_px.reload.is_some()
        })?;
        editor.click_file_conflict_action(FileConflictAction::Reload)?;
        editor.wait_state("disk version reloaded", secs(4), |record| {
            record.file_conflict_path.is_none() && !record.active_tab_modified && record.line_count == 3
        })?;

        editor.keys("<C-a>verified reload")?;
        editor.save_then_expect_file(&path, "verified reload")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display"]
fn external_change_banner_keep_mine_overwrites_only_the_observed_disk_version() -> TestResult {
    support::run_x11_test("external-conflict-keep-mine", |session| {
        let path = session.seed_file("external-keep-mine.txt", "original\n")?;
        let identity = path_text(&path);
        let mut editor = session.open_file("external-keep-mine", &path)?;

        editor.keys("local edit")?;
        fs::write(&path, "external version\n")?;
        editor.wait_state("external conflict banner", secs(4), |record| {
            record.file_conflict_path.as_deref() == Some(identity.as_str())
                && record.file_conflict_button_bounds_px.keep_mine.is_some()
        })?;
        editor.click_file_conflict_action(FileConflictAction::KeepMine)?;
        editor.expect_file(&path, "original\nlocal edit")?;
        editor.wait_state("local version saved", secs(4), |record| {
            record.file_conflict_path.is_none() && !record.active_tab_modified
        })?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display"]
fn clean_external_change_reloads_in_place_without_a_prompt() -> TestResult {
    support::run_x11_test("external-clean-reload", |session| {
        let path = session.seed_file("clean-external-reload.txt", "original\n")?;
        let mut editor = session.open_file("clean-external-reload", &path)?;

        fs::write(&path, "disk version\nsecond line\n")?;
        editor.wait_state("clean external reload", secs(4), |record| {
            record.file_conflict_path.is_none()
                && !record.active_tab_modified
                && record.line_count == 3
                && record.status_message.starts_with("Reloaded ")
        })?;
        editor.keys("<C-a>verified clean reload")?;
        editor.save_then_expect_file(&path, "verified clean reload")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display"]
fn reappearing_deleted_file_conflicts_instead_of_replacing_the_editor_copy() -> TestResult {
    support::run_x11_test("external-reappearing-backing-file", |session| {
        let path = session.seed_file("reappearing-backing-file.txt", "only copy in the editor\n")?;
        let identity = path_text(&path);
        let mut editor = session.open_file("reappearing-backing-file", &path)?;

        fs::remove_file(&path)?;
        editor.wait_state("missing backing file", secs(4), |record| {
            record.active_tab_backing_file_missing
        })?;
        fs::write(&path, "a different recreated file\n")?;
        editor.wait_state("reappeared file conflict", secs(4), |record| {
            record.active_tab_backing_file_missing
                && record.file_conflict_path.as_deref() == Some(identity.as_str())
                && record.file_conflict_button_bounds_px.keep_mine.is_some()
        })?;
        assert_eq!(fs::read_to_string(&path)?, "a different recreated file\n");

        editor.click_file_conflict_action(FileConflictAction::KeepMine)?;
        editor.expect_file(&path, "only copy in the editor\n")?;
        editor.wait_state("editor copy saved", secs(4), |record| {
            !record.active_tab_backing_file_missing
                && record.file_conflict_path.is_none()
                && !record.active_tab_modified
        })?;
        Ok(())
    })
}
