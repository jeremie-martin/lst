use super::*;
use lst_editor::{EditorModel, TabId, UndoBoundary};
#[cfg(unix)]
use std::os::unix::fs::{symlink, PermissionsExt};
use std::{
    collections::HashSet,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);

fn temp_dir(label: &str) -> PathBuf {
    let id = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("lst-gpui-runtime-{label}-{}-{id}", process::id()));
    fs::create_dir(&dir).expect("create test temp dir");
    dir
}

fn tab_for_path(path: PathBuf, text: &str) -> ModelEditorTab {
    ModelEditorTab::from_path_with_stamp(
        TabId::from_raw(1),
        path,
        text,
        Some(FileStamp::from_raw(0, Some(0))),
    )
}

fn tab_for_path_with_id(id: u64, path: PathBuf, text: &str) -> ModelEditorTab {
    ModelEditorTab::from_path_with_stamp(
        TabId::from_raw(id),
        path,
        text,
        Some(FileStamp::from_raw(0, Some(0))),
    )
}

fn scratchpad_for_path_with_id(id: u64, path: PathBuf) -> ModelEditorTab {
    ModelEditorTab::scratchpad_with_stamp(
        TabId::from_raw(id),
        path,
        FileStamp::from_raw(0, Some(0)),
    )
}

#[test]
fn scratchpad_note_creation_uses_timestamped_names_and_collision_suffixes() {
    let dir = temp_dir("scratchpad");
    let timestamp = "2026-04-11_12-13-14".to_string();

    let (first, first_stamp) = create_scratchpad_note_with_timestamp(Some(&dir), timestamp.clone())
        .expect("create first scratchpad");
    let (second, second_stamp) = create_scratchpad_note_with_timestamp(Some(&dir), timestamp)
        .expect("create second scratchpad");

    assert_eq!(
        first.file_name().and_then(|name| name.to_str()),
        Some("2026-04-11_12-13-14.md")
    );
    assert_eq!(
        second.file_name().and_then(|name| name.to_str()),
        Some("2026-04-11_12-13-14_1.md")
    );
    assert_eq!(fs::read_to_string(&first).expect("read first"), "");
    assert_eq!(first_stamp, file_stamp(&first).expect("first stamp"));
    assert_eq!(second_stamp, file_stamp(&second).expect("second stamp"));

    fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[test]
fn successful_save_as_removes_previous_scratchpad_file_only_when_path_changes() {
    let dir = temp_dir("scratchpad-save-as");
    let old = dir.join("2026-04-11_12-13-14.md");
    let same = old.clone();
    let new = dir.join("saved.md");
    fs::write(&old, "").expect("write old scratchpad");

    remove_previous_scratchpad_after_save_as(Some(old.clone()), &same, &[]);
    assert!(old.exists());

    remove_previous_scratchpad_after_save_as(Some(old.clone()), &new, &[]);
    assert!(!old.exists());

    fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[test]
fn successful_save_as_keeps_target_when_path_spelling_changes() {
    let dir = temp_dir("scratchpad-save-as-same-file");
    let nested = dir.join("nested");
    fs::create_dir(&nested).expect("create nested test dir");
    let saved = dir.join("saved.md");
    let same_file = nested.join("..").join("saved.md");
    fs::write(&saved, "saved body").expect("write saved file");

    assert_ne!(same_file, saved);
    remove_previous_scratchpad_after_save_as(Some(same_file), &saved, &[]);

    assert_eq!(
        fs::read_to_string(&saved).expect("read saved file"),
        "saved body"
    );

    fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[test]
fn successful_save_as_keeps_source_when_another_tab_still_uses_it() {
    let dir = temp_dir("scratchpad-save-as-source-open");
    let old = dir.join("2026-04-11_12-13-14.md");
    let new = dir.join("saved.md");
    fs::write(&old, "shared scratchpad").expect("write old scratchpad");
    fs::write(&new, "saved body").expect("write saved file");
    let open_tabs = vec![tab_for_path_with_id(2, old.clone(), "other tab")];

    remove_previous_scratchpad_after_save_as(Some(old.clone()), &new, &open_tabs);

    assert_eq!(
        fs::read_to_string(&old).expect("read old scratchpad"),
        "shared scratchpad"
    );

    fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[test]
fn empty_scratchpad_cleanup_keeps_files_open_in_another_tab() {
    let dir = temp_dir("scratchpad-cleanup-shared");
    let path = dir.join("2026-04-11_12-13-14.md");
    fs::write(&path, "other tab content").expect("write scratchpad");
    let open_tabs = vec![
        scratchpad_for_path_with_id(1, path.clone()),
        tab_for_path_with_id(2, path.clone(), "other tab content"),
    ];

    remove_scratchpad_file_if_unreferenced(&open_tabs, TabId::from_raw(1), &path);

    assert_eq!(
        fs::read_to_string(&path).expect("read shared scratchpad"),
        "other tab content"
    );

    fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[test]
fn open_file_results_read_existing_files_and_report_failures() {
    let dir = temp_dir("open");
    let ok = dir.join("ok.txt");
    let missing = dir.join("missing.txt");
    fs::write(&ok, "hello").expect("write open fixture");

    let results = open_file_results([ok.clone(), missing.clone()]);

    assert_eq!(results.opened.len(), 1);
    assert_eq!(results.opened[0].0, ok);
    assert_eq!(results.opened[0].1, "hello");
    assert!(results.opened[0].2.is_some());
    assert_eq!(results.failed.len(), 1);
    assert_eq!(results.failed[0].0, missing);
    assert!(!results.failed[0].1.is_empty());

    fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[test]
fn save_file_result_writes_body_and_reports_result() {
    let dir = temp_dir("save");
    let path = dir.join("saved.txt");

    let tab_id = TabId::from_raw(1);
    let result = save_file_result(
        tab_id,
        path.clone(),
        "saved body".to_string(),
        7,
        None,
        SaveTicket::current_for_test(),
    );

    match result {
        SaveFileResult::Saved {
            tab_id: saved_tab,
            path: saved_path,
            revision,
            stamp,
            body,
        } => {
            assert_eq!(saved_tab, tab_id);
            assert_eq!(saved_path, path.clone());
            assert_eq!(revision, 7);
            assert_eq!(stamp, file_stamp(&path).expect("saved stamp"));
            assert_eq!(body, "saved body");
        }
        other => panic!("expected save success result, got {other:?}"),
    }
    assert_eq!(
        fs::read_to_string(&path).expect("read saved file"),
        "saved body"
    );

    fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[test]
fn superseded_save_ticket_does_not_write_stale_body() {
    let dir = temp_dir("save-stale-ticket");
    let path = dir.join("saved.txt");
    fs::write(&path, "newer body").expect("write newer fixture");
    let tab_id = TabId::from_raw(1);
    let current_generation = Arc::new(Mutex::new(0));
    let old_ticket = SaveTicket::issue(&current_generation);
    let _new_ticket = SaveTicket::issue(&current_generation);

    let result = save_file_result(
        tab_id,
        path.clone(),
        "older body".to_string(),
        1,
        None,
        old_ticket,
    );

    assert_eq!(
        result,
        SaveFileResult::Stale {
            tab_id,
            path: path.clone(),
            revision: 1,
        }
    );
    assert_eq!(
        fs::read_to_string(&path).expect("read saved file"),
        "newer body"
    );

    fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[cfg(unix)]
#[test]
fn atomic_save_preserves_existing_file_permissions() {
    let dir = temp_dir("save-mode");
    let path = dir.join("script.sh");
    fs::write(&path, "#!/bin/sh\n").expect("write script fixture");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("set executable mode");
    let tab_id = TabId::from_raw(1);

    let result = save_file_result(
        tab_id,
        path.clone(),
        "#!/bin/sh\necho saved\n".to_string(),
        1,
        None,
        SaveTicket::current_for_test(),
    );

    assert!(matches!(result, SaveFileResult::Saved { .. }), "{result:?}");
    let mode = fs::metadata(&path)
        .expect("saved metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o755);

    fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[cfg(unix)]
#[test]
fn save_through_symlink_updates_target_without_replacing_link() {
    let dir = temp_dir("save-symlink");
    let target = dir.join("target.txt");
    let link = dir.join("link.txt");
    fs::write(&target, "old").expect("write symlink target");
    symlink(&target, &link).expect("create symlink");
    let tab_id = TabId::from_raw(1);

    let result = save_file_result(
        tab_id,
        link.clone(),
        "new".to_string(),
        1,
        None,
        SaveTicket::current_for_test(),
    );

    assert!(matches!(result, SaveFileResult::Saved { .. }), "{result:?}");
    assert!(fs::symlink_metadata(&link)
        .expect("link metadata")
        .file_type()
        .is_symlink());
    assert_eq!(fs::read_to_string(&target).expect("read target"), "new");

    fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[cfg(unix)]
#[test]
fn failed_safe_save_preserves_existing_file_contents() {
    let dir = temp_dir("save-failure-preserves");
    let path = dir.join("note.txt");
    fs::write(&path, "old").expect("write save fixture");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).expect("set writable file mode");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).expect("make parent read-only");
    let tab_id = TabId::from_raw(1);

    let result = save_file_result(
        tab_id,
        path.clone(),
        "new".to_string(),
        1,
        None,
        SaveTicket::current_for_test(),
    );
    let saved_text = fs::read_to_string(&path).expect("read destination after failed save");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).expect("restore parent mode");

    match result {
        SaveFileResult::Failed { path: failed, .. } => assert_eq!(failed, path),
        other => panic!("expected save failure result, got {other:?}"),
    }
    assert_eq!(saved_text, "old");

    fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[cfg(unix)]
#[test]
fn save_file_result_refuses_read_only_targets() {
    let dir = temp_dir("save-readonly");
    let path = dir.join("readonly.txt");
    fs::write(&path, "old").expect("write read-only fixture");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).expect("set read-only mode");
    let tab_id = TabId::from_raw(1);

    let result = save_file_result(
        tab_id,
        path.clone(),
        "new".to_string(),
        1,
        None,
        SaveTicket::current_for_test(),
    );

    match result {
        SaveFileResult::Failed { path: failed, .. } => assert_eq!(failed, path),
        other => panic!("expected save failure result, got {other:?}"),
    }
    assert_eq!(fs::read_to_string(&path).expect("read destination"), "old");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).expect("restore writable mode");

    fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[test]
fn save_file_result_reports_write_failures() {
    let dir = temp_dir("save-failure");

    let tab_id = TabId::from_raw(1);
    let result = save_file_result(
        tab_id,
        dir.clone(),
        "cannot replace directory".to_string(),
        7,
        None,
        SaveTicket::current_for_test(),
    );

    match result {
        SaveFileResult::Failed {
            tab_id: failed_tab,
            path,
            message,
        } => {
            assert_eq!(failed_tab, tab_id);
            assert_eq!(path, dir.clone());
            assert!(!message.is_empty());
        }
        other => panic!("expected save failure result, got {other:?}"),
    }

    fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[test]
fn save_file_result_reports_external_conflicts_without_writing() {
    let dir = temp_dir("save-conflict");
    let path = dir.join("note.txt");
    fs::write(&path, "old").expect("write old fixture");
    let expected_stamp = file_stamp(&path).expect("old stamp");
    fs::write(&path, "external").expect("write external fixture");
    let disk_stamp = file_stamp(&path).expect("external stamp");
    let tab_id = TabId::from_raw(1);

    let result = save_file_result(
        tab_id,
        path.clone(),
        "local".to_string(),
        7,
        Some(expected_stamp),
        SaveTicket::current_for_test(),
    );

    assert_eq!(
        result,
        SaveFileResult::Conflict {
            tab_id,
            path: path.clone(),
            revision: 7,
            disk_stamp,
        }
    );
    assert_eq!(
        fs::read_to_string(&path).expect("read conflicted destination"),
        "external"
    );

    fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[test]
fn save_file_result_reports_deleted_backing_file_as_conflict() {
    let dir = temp_dir("save-deleted");
    let path = dir.join("note.txt");
    fs::write(&path, "old").expect("write old fixture");
    let expected_stamp = file_stamp(&path).expect("old stamp");
    fs::remove_file(&path).expect("delete backing file");
    let tab_id = TabId::from_raw(1);

    let result = save_file_result(
        tab_id,
        path.clone(),
        "local".to_string(),
        7,
        Some(expected_stamp),
        SaveTicket::current_for_test(),
    );

    assert_eq!(
        result,
        SaveFileResult::Conflict {
            tab_id,
            path: path.clone(),
            revision: 7,
            disk_stamp: expected_stamp,
        }
    );
    assert!(!path.exists());

    fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[test]
fn can_start_autosave_job_requires_current_revision_and_no_inflight_write() {
    let dir = temp_dir("autosave-start");
    let path = dir.join("note.txt");
    let tab = tab_for_path(path.clone(), "old");
    let mut inflight = HashSet::new();

    assert!(can_start_autosave_job(
        std::slice::from_ref(&tab),
        &inflight,
        tab.id(),
        &path,
        0
    ));

    inflight.insert(path.clone());
    assert!(!can_start_autosave_job(
        std::slice::from_ref(&tab),
        &inflight,
        tab.id(),
        &path,
        0
    ));

    let mut stale_model = EditorModel::from_tab(tab, "Ready.".to_string());
    stale_model.replace_text(Some(0..0), "new ".into(), UndoBoundary::Break);
    assert!(!can_start_autosave_job(
        stale_model.tabs(),
        &HashSet::new(),
        TabId::from_raw(1),
        &path,
        0
    ));

    fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[test]
fn autosave_completion_commits_current_revision() {
    let dir = temp_dir("autosave-commit");
    let path = dir.join("note.txt");
    fs::write(&path, "old").expect("write autosave destination");
    let expected_stamp = file_stamp(&path).expect("initial stamp");
    let tab = ModelEditorTab::from_path_with_stamp(
        TabId::from_raw(1),
        path.clone(),
        "old",
        Some(expected_stamp),
    );
    let job = AutosaveJob {
        tab_id: tab.id(),
        path: path.clone(),
        body: "new".to_string(),
        revision: 0,
        expected_stamp: Some(expected_stamp),
    };

    let temp_path = write_autosave_temp_file(&job).expect("write autosave temp file");
    let completion = autosave_completion(&[tab], job, Ok(temp_path));

    match completion {
        Some(AutosaveCompletion::Finished {
            tab_id,
            path: saved_path,
            revision,
            stamp,
            body,
        }) => {
            assert_eq!(tab_id, TabId::from_raw(1));
            assert_eq!(saved_path, path.clone());
            assert_eq!(revision, 0);
            assert_eq!(stamp, file_stamp(&path).expect("autosaved stamp"));
            assert_eq!(body, "new");
        }
        other => panic!("expected autosave completion, got {other:?}"),
    }
    assert_eq!(
        fs::read_to_string(&path).expect("read autosaved file"),
        "new"
    );

    fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[test]
fn autosave_completion_discards_stale_temp_without_command() {
    let dir = temp_dir("autosave-stale");
    let path = dir.join("note.txt");
    fs::write(&path, "old").expect("write autosave destination");
    let tab = tab_for_path(path.clone(), "old");
    let mut model = EditorModel::from_tab(tab, "Ready.".to_string());
    let expected_stamp = model.active_tab().file_stamp();
    model.replace_text(Some(0..0), "current ".into(), UndoBoundary::Break);
    let job = AutosaveJob {
        tab_id: model.active_tab_id(),
        path: path.clone(),
        body: "stale".to_string(),
        revision: 0,
        expected_stamp,
    };

    let temp_path = write_autosave_temp_file(&job).expect("write autosave temp file");
    let completion = autosave_completion(model.tabs(), job, Ok(temp_path.clone()));

    assert_eq!(completion, None);
    assert!(!temp_path.exists());
    assert_eq!(
        fs::read_to_string(&path).expect("read destination file"),
        "old"
    );

    fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[test]
fn autosave_completion_reports_conflict_without_renaming_temp() {
    let dir = temp_dir("autosave-conflict");
    let path = dir.join("note.txt");
    fs::write(&path, "old").expect("write autosave destination");
    let expected_stamp = file_stamp(&path).expect("old stamp");
    let tab = ModelEditorTab::from_path_with_stamp(
        TabId::from_raw(1),
        path.clone(),
        "old",
        Some(expected_stamp),
    );
    let job = AutosaveJob {
        tab_id: tab.id(),
        path: path.clone(),
        body: "local".to_string(),
        revision: 0,
        expected_stamp: Some(expected_stamp),
    };
    let temp_path = write_autosave_temp_file(&job).expect("write autosave temp file");
    fs::write(&path, "external").expect("write external fixture");
    let disk_stamp = file_stamp(&path).expect("external stamp");

    let completion = autosave_completion(&[tab], job, Ok(temp_path.clone()));

    assert_eq!(
        completion,
        Some(AutosaveCompletion::Conflict {
            tab_id: TabId::from_raw(1),
            path: path.clone(),
            revision: 0,
            disk_stamp,
        })
    );
    assert!(!temp_path.exists());
    assert_eq!(
        fs::read_to_string(&path).expect("read destination file"),
        "external"
    );

    fs::remove_dir_all(dir).expect("remove test temp dir");
}

#[test]
fn autosave_completion_reports_deleted_backing_file_as_conflict() {
    let dir = temp_dir("autosave-deleted");
    let path = dir.join("note.txt");
    fs::write(&path, "old").expect("write autosave destination");
    let expected_stamp = file_stamp(&path).expect("old stamp");
    let tab = ModelEditorTab::from_path_with_stamp(
        TabId::from_raw(1),
        path.clone(),
        "old",
        Some(expected_stamp),
    );
    let job = AutosaveJob {
        tab_id: tab.id(),
        path: path.clone(),
        body: "local".to_string(),
        revision: 0,
        expected_stamp: Some(expected_stamp),
    };
    let temp_path = write_autosave_temp_file(&job).expect("write autosave temp file");
    fs::remove_file(&path).expect("delete backing file");

    let completion = autosave_completion(&[tab], job, Ok(temp_path.clone()));

    assert_eq!(
        completion,
        Some(AutosaveCompletion::Conflict {
            tab_id: TabId::from_raw(1),
            path: path.clone(),
            revision: 0,
            disk_stamp: expected_stamp,
        })
    );
    assert!(!temp_path.exists());
    assert!(!path.exists());

    fs::remove_dir_all(dir).expect("remove test temp dir");
}
