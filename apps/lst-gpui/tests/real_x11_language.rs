//! Real-display coverage for language-sensitive editor behavior.
//!
//! These tests intentionally assert through saved file bytes. The feature under
//! test is not "which enum was detected"; it is the user-visible behavior that
//! follows from opening a path with a given language.

mod support;

use support::{EditorTestExt, TestResult};

fn expect_edit(label: &str, file_name: &str, initial: &str, keys: &str, expected: &str) -> TestResult {
    support::run_x11_test(label, |session| {
        let path = session.seed_file(file_name, initial)?;
        let mut editor = session.open_file(label, &path)?;

        editor.keys(keys)?;
        editor.save_then_expect_file(&path, expected)?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn line_comment_uses_python_hash_prefix() -> TestResult {
    expect_edit("language-line-comment-python", "script.py", "x = 1", "<C-/>", "# x = 1")
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn line_comment_uses_rust_slash_prefix() -> TestResult {
    expect_edit(
        "language-line-comment-rust",
        "main.rs",
        "let x = 1;",
        "<C-/>",
        "// let x = 1;",
    )
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn block_comment_toggles_current_rust_line() -> TestResult {
    support::run_x11_test("language-block-comment-rust", |session| {
        let path = session.seed_file("main.rs", "let x = 1;\n")?;
        let mut editor = session.open_file("language-block-comment-rust", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<C-S-/>")?;
        editor.save_then_expect_file(&path, "/*let x = 1;*/\n")?;

        editor.keys("<C-S-/>")?;
        editor.save_then_expect_file(&path, "let x = 1;\n")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn angle_bracket_pairing_is_language_sensitive() -> TestResult {
    support::run_x11_test("language-angle-pairing", |session| {
        let tsx = session.seed_file("component.tsx", "")?;
        {
            let mut editor = session.open_file("language-angle-pairing-tsx", &tsx)?;
            editor.keys("<lt>")?;
            editor.save_then_expect_file(&tsx, "<>")?;
            editor.quit_default()?;
        }

        let rust = session.seed_file("main.rs", "")?;
        let mut editor = session.open_file("language-angle-pairing-rust", &rust)?;
        editor.keys("<lt>")?;
        editor.save_then_expect_file(&rust, "<")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn single_quote_pairing_is_suppressed_for_rust_lifetimes() -> TestResult {
    support::run_x11_test("language-single-quote-pairing", |session| {
        let rust = session.seed_file("lifetime.rs", "")?;
        {
            let mut editor = session.open_file("language-single-quote-rust", &rust)?;
            editor.keys("'")?;
            editor.save_then_expect_file(&rust, "'")?;
            editor.quit_default()?;
        }

        let js = session.seed_file("string.js", "")?;
        let mut editor = session.open_file("language-single-quote-js", &js)?;
        editor.keys("'")?;
        editor.save_then_expect_file(&js, "''")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn tab_and_soft_tab_backspace_use_language_indent_unit() -> TestResult {
    support::run_x11_test("language-indent-unit", |session| {
        let js = session.seed_file("indent.js", "")?;
        {
            let mut editor = session.open_file("language-indent-tab-js", &js)?;
            editor.keys("<tab>")?;
            editor.save_then_expect_file(&js, "  ")?;
            editor.quit_default()?;
        }

        let spaces = session.seed_file("backspace.js", "      ")?;
        let mut editor = session.open_file("language-indent-backspace-js", &spaces)?;
        editor.keys("<end><bs>")?;
        editor.save_then_expect_file(&spaces, "    ")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn close_brace_dedents_only_for_languages_with_brace_blocks() -> TestResult {
    support::run_x11_test("language-auto-dedent", |session| {
        let rust = session.seed_file("dedent.rs", "        ")?;
        {
            let mut editor = session.open_file("language-dedent-rust", &rust)?;
            editor.keys("<end>}")?;
            editor.save_then_expect_file(&rust, "    }")?;
            editor.quit_default()?;
        }

        let python = session.seed_file("dedent.py", "        ")?;
        let mut editor = session.open_file("language-dedent-python", &python)?;
        editor.keys("<end>}")?;
        editor.save_then_expect_file(&python, "        }")?;
        Ok(())
    })
}
