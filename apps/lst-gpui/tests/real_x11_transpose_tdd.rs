//! Under-review executable specs for `Ctrl+T` transpose.
//!
//! Pinned semantics (Emacs `C-t` flavor):
//!
//! - In the middle of a line, swap the character to the left of the caret
//!   with the character to the right of the caret. The caret moves one
//!   character to the right after the swap.
//! - At column zero, swap the first two characters of the line. (The Emacs
//!   "first call at column 0 is a no-op" behavior is rejected — most users
//!   expect the swap to fire.)
//! - At end-of-line, swap the two characters that sit before the caret —
//!   matching the Emacs convention that `C-t` at EOL transposes the last
//!   two chars rather than crossing the line break.
//!
//! Specs run under the `x11-tdd` profile. Promote to `real_x11_transpose.rs`
//! once green.
//!
//!     cargo nextest run --profile x11-tdd -p lst-gpui --test real_x11_transpose_tdd --run-ignored only

mod support;

use support::{EditorTestExt, TestResult};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_t_in_middle_of_word_swaps_surrounding_chars() -> TestResult {
    support::run_x11_test("transpose-middle", |session| {
        let path = session.seed_file("transpose-middle.txt", "abcd")?;
        let mut editor = session.open_file("transpose-middle", &path)?;

        // Caret between 'b' and 'c' (column 2). `Ctrl+T` swaps the two
        // surrounding chars: 'b' and 'c' → "acbd".
        editor.place_cursor_at_document_start()?;
        editor.keys("<right><right>")?;
        editor.expect_cursor_heads(&[(0, 2)])?;

        editor.keys("<C-t>")?;
        editor.save_then_expect_file(&path, "acbd")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_t_at_column_zero_swaps_first_two_chars() -> TestResult {
    support::run_x11_test("transpose-bol", |session| {
        let path = session.seed_file("transpose-bol.txt", "abcd")?;
        let mut editor = session.open_file("transpose-bol", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<C-t>")?;
        editor.save_then_expect_file(&path, "bacd")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_t_at_end_of_line_swaps_last_two_chars() -> TestResult {
    support::run_x11_test("transpose-eol", |session| {
        let path = session.seed_file("transpose-eol.txt", "abcd\nrest")?;
        let mut editor = session.open_file("transpose-eol", &path)?;

        // Caret at end of line 0 (column 4). With no char to the right,
        // transpose the two characters to the left: "abcd" → "abdc".
        // The newline and trailing "rest" must remain untouched.
        editor.place_cursor_at_document_start()?;
        editor.keys("<end>")?;
        editor.expect_cursor_heads(&[(0, 4)])?;

        editor.keys("<C-t>")?;
        editor.save_then_expect_file(&path, "abdc\nrest")?;
        Ok(())
    })
}
