mod common;

use common::*;
use lst::app::{App, Message, RuntimeMode};
use std::path::PathBuf;

#[test]
fn boot_without_files_creates_scratchpad_in_default_dir() {
    let app = AppHarness::boot(&[]);
    let snap = app.snapshot();
    let scratchpad_dir = PathBuf::from("/tmp/lst-harness-home/.local/share/lst");

    assert_eq!(snap.tab_count, 1);
    assert_eq!(snap.tab_titles, vec!["1970-01-01_00-00-00.md"]);
    assert!(app.fs.created_dirs().contains(&scratchpad_dir));
}

#[test]
fn boot_respects_scratchpad_dir_override() {
    let app = AppHarness::boot_with(
        &["--scratchpad-dir", "/tmp/notes"],
        "2026-04-09_12-00-00",
        |_, _| {},
    );
    let snap = app.snapshot();

    assert!(app.fs.created_dirs().contains(&PathBuf::from("/tmp/notes")));
    assert_eq!(snap.tab_titles, vec!["2026-04-09_12-00-00.md"]);
}

#[test]
fn boot_respects_custom_title_override() {
    let app = AppHarness::boot_with(
        &[
            "--title",
            "lst-scratchpad",
            "--scratchpad-dir",
            "/tmp/notes",
        ],
        "2026-04-09_12-00-00",
        |_, _| {},
    );

    assert_eq!(app.snapshot().title, "lst-scratchpad");
}

#[test]
fn boot_opens_existing_files_and_skips_unreadable_paths() {
    let mut app = AppHarness::boot_with(
        &["/tmp/a.txt", "/tmp/missing.txt", "/tmp/b.txt"],
        "2026-04-09_12-00-00",
        |fs, _| {
            fs.seed_file("/tmp/a.txt", "aaa");
            fs.seed_file("/tmp/b.txt", "bbb");
            fs.set_canonical_path("/tmp/a.txt", "/real/a.txt");
        },
    );
    let first = app.snapshot();

    assert_eq!(first.tab_count, 2);
    assert_eq!(first.tab_titles, vec!["a.txt", "b.txt"]);
    assert_eq!(first.text, "aaa");

    app.send(Message::TabSelect(1));
    assert_eq!(app.snapshot().text, "bbb");
}

#[test]
fn boot_avoids_scratchpad_name_collisions() {
    let app = AppHarness::boot_with(
        &["--scratchpad-dir", "/tmp/notes"],
        "2026-04-09_12-00-00",
        |fs, _| {
            fs.seed_file("/tmp/notes/2026-04-09_12-00-00.md", "existing");
        },
    );

    assert_eq!(app.snapshot().tab_titles, vec!["2026-04-09_12-00-00_1.md"]);
}

#[test]
fn boot_with_files_does_not_require_home_when_scratchpad_is_not_needed() {
    let (_clipboard, fs, dialogs, services) =
        runtime_services_with_options("2026-04-09_12-00-00", RuntimeMode::Async, None::<&str>);
    fs.seed_file("/tmp/a.txt", "aaa");
    let args = lst::app::AppArgs::parse_from(["/tmp/a.txt"]).unwrap();

    let (app, _) = App::boot_with(args, services).unwrap();

    assert_eq!(app.snapshot().tab_titles, vec!["a.txt"]);
    assert_eq!(dialogs.open_requests(), 0);
}
