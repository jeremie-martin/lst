//! Real-display tests for Ctrl/Shift modifier chords. Each test is a
//! self-contained "do inputs, assert output" scenario, exercising one
//! modifier-driven behaviour through the harness's `send_keys` so we
//! verify the chord notation (`<C-a>`, `<C-z>`, `<C-y>`) actually drives
//! the editor end-to-end.
//!
//! Run with
//!
//!     cargo test -p lst-gpui --tests -- --ignored --test-threads=1 --nocapture

mod support;

use std::fs;

use support::{EditorTestExt, ScratchpadSession, TestResult};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_a_select_all_then_type_replaces_buffer() -> TestResult {
    // Open a file with pre-seeded content, select all with Ctrl+A, then
    // type a fresh string. The active selection makes the next literal
    // input replace the entire buffer in one transaction. This is the
    // canonical "I want to start over" gesture and the simplest possible
    // proof that modifier chords mid-`send_keys` reach the editor.
    let mut session = ScratchpadSession::new("modifier-ctrl-a")?;
    let seed_path = session.root().join("seed.txt");
    fs::write(&seed_path, "old content here\nstill old\n")?;
    let mut editor = session.open_file("file", &seed_path)?;

    editor.send_keys("<C-a>this should replace the existing text")?;
    editor.save_then_expect_file(&seed_path, "this should replace the existing text")?;
    Ok(())
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_z_undoes_a_typed_run() -> TestResult {
    // Type a single word — no internal word boundaries, so the editor
    // coalesces it into one undo group — then press Ctrl+Z. The buffer
    // should return to the empty state of a fresh scratchpad, which we
    // verify by saving and asserting the autosave path is empty.
    let mut session = ScratchpadSession::new("modifier-ctrl-z")?;
    let (mut editor, path) = session.open("scratch")?;

    editor.send_keys("hello")?;
    editor.send_keys("<C-z>")?;
    editor.save_then_expect_file(&path, "")?;
    Ok(())
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_y_redoes_after_ctrl_z() -> TestResult {
    // Round-trip: type → undo → redo. The redo must restore the original
    // text exactly. Single word so the typing coalesces into one undo
    // group, mirroring `ctrl_z_undoes_a_typed_run`.
    let mut session = ScratchpadSession::new("modifier-ctrl-y")?;
    let (mut editor, path) = session.open("scratch")?;

    editor.send_keys("hello<C-z><C-y>")?;
    editor.save_then_expect_file(&path, "hello")?;
    Ok(())
}
