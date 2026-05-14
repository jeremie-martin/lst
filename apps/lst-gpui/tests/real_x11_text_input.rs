//! Real-display text-input edge cases that need seeded non-ASCII fixtures.
//!
//! The key harness cannot type arbitrary Unicode today, but it can open files
//! that contain Unicode and drive normal movement/deletion commands through the
//! production app.

mod support;

use support::{EditorTestExt, TestResult};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn backspace_deletes_seeded_combining_cluster_as_one_character() -> TestResult {
    support::run_x11_test("text-input-grapheme-backspace", |session| {
        let path = session.seed_file("grapheme.txt", "e\u{301}x")?;
        let mut editor = session.open_file("text-input-grapheme-backspace", &path)?;

        editor.keys("<right><bs>")?;
        editor.save_then_expect_file(&path, "x")?;
        Ok(())
    })
}
