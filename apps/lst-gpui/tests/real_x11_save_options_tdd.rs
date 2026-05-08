//! Under-review executable specs for save-time text policies.
//!
//! Two opt-in policies are pinned by these specs:
//!
//! - **Trim trailing whitespace on save**, activated by
//!   `LST_SAVE_TRIM_TRAILING_WS=1`. When on, every line written to disk has
//!   trailing spaces and tabs stripped.
//! - **Ensure final newline on save**, activated by
//!   `LST_SAVE_ENSURE_FINAL_NEWLINE=1`. When on, a non-empty buffer that
//!   does not already end with `\n` is written with one appended.
//!
//! The flags default off so existing users are unaffected — Markdown hard
//! breaks and intentional trailing whitespace survive a normal save. They
//! exist here as test seams in the spirit of `LST_LLM_FAKE_RESPONSE`; once
//! a real settings surface lands, both opts move there.
//!
//! Specs run under the `x11-tdd` profile. Failures are diagnostic until the
//! feature lands; once green and accepted, rename this file to
//! `real_x11_save_options.rs` to promote it into the blocking `x11` lane.
//!
//!     cargo nextest run --profile x11-tdd -p lst-gpui --test real_x11_save_options_tdd --run-ignored only

mod support;

use std::ffi::OsStr;

use support::{EditorTestExt, TestResult};

const TRIM_ENV: &str = "LST_SAVE_TRIM_TRAILING_WS";
const FINAL_NEWLINE_ENV: &str = "LST_SAVE_ENSURE_FINAL_NEWLINE";

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn trim_trailing_whitespace_strips_spaces_and_tabs_per_line() -> TestResult {
    support::run_x11_test("save-trim-strips", |session| {
        let env: [(&OsStr, &OsStr); 1] = [(OsStr::new(TRIM_ENV), OsStr::new("1"))];
        let (mut editor, path) = session.open_with_env("scratch", &env)?;

        editor.keys("alpha   <enter>beta\t\t<enter>gamma")?;
        editor.save_then_expect_file(&path, "alpha\nbeta\ngamma")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn trim_is_idempotent_on_already_clean_buffer() -> TestResult {
    support::run_x11_test("save-trim-idempotent", |session| {
        let env: [(&OsStr, &OsStr); 1] = [(OsStr::new(TRIM_ENV), OsStr::new("1"))];
        let (mut editor, path) = session.open_with_env("scratch", &env)?;

        // No trailing whitespace anywhere; trim must be a no-op.
        editor.keys("alpha<enter>beta<enter>gamma")?;
        editor.save_then_expect_file(&path, "alpha\nbeta\ngamma")?;

        // Saving again without changes preserves the same bytes.
        editor.save_then_expect_file(&path, "alpha\nbeta\ngamma")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ensure_final_newline_appends_when_missing() -> TestResult {
    support::run_x11_test("save-final-newline-append", |session| {
        let env: [(&OsStr, &OsStr); 1] = [(OsStr::new(FINAL_NEWLINE_ENV), OsStr::new("1"))];
        let (mut editor, path) = session.open_with_env("scratch", &env)?;

        // Buffer ends without a trailing newline. Save must add exactly
        // one — not two, not none.
        editor.keys("alpha<enter>beta")?;
        editor.save_then_expect_file(&path, "alpha\nbeta\n")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ensure_final_newline_is_idempotent_when_already_terminated() -> TestResult {
    support::run_x11_test("save-final-newline-idempotent", |session| {
        let env: [(&OsStr, &OsStr); 1] = [(OsStr::new(FINAL_NEWLINE_ENV), OsStr::new("1"))];
        let (mut editor, path) = session.open_with_env("scratch", &env)?;

        // Buffer already ends with one newline. Save must not append a
        // second one.
        editor.keys("alpha<enter>beta<enter>")?;
        editor.save_then_expect_file(&path, "alpha\nbeta\n")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ensure_final_newline_does_not_touch_an_empty_buffer() -> TestResult {
    support::run_x11_test("save-final-newline-empty", |session| {
        let env: [(&OsStr, &OsStr); 1] = [(OsStr::new(FINAL_NEWLINE_ENV), OsStr::new("1"))];
        let (mut editor, path) = session.open_with_env("scratch", &env)?;

        // Empty scratchpad. Saving with ensure-final-newline on must not
        // synthesize a stray `\n` — an empty buffer maps to an empty file.
        editor.save_then_expect_file(&path, "")?;
        Ok(())
    })
}
