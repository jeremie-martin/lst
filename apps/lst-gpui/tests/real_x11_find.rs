//! Real-display specs for accepted find-panel behavior.

mod support;

use lst_x11_harness::{clipboard::write_clipboard_text, Selection};
use support::{secs, EditorTestExt, FindChip, TestResult};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn lowercase_find_query_uses_smart_case() -> TestResult {
    support::run_x11_test("find-smart-case-lowercase", |session| {
        let path = session.seed_file("smart-case-lowercase.txt", "Foo foo FOO")?;
        let mut editor = session.open_file("find-smart-case-lowercase", &path)?;

        editor.keys("<C-f>foo")?;
        let record = editor.expect_find_state("foo", 3)?;
        assert!(!record.find.case_sensitive, "{record:?}");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn uppercase_find_query_is_case_sensitive() -> TestResult {
    support::run_x11_test("find-smart-case-uppercase", |session| {
        let path = session.seed_file("smart-case-uppercase.txt", "Foo foo FOO")?;
        let mut editor = session.open_file("find-smart-case-uppercase", &path)?;

        editor.keys("<C-f>Foo")?;
        let record = editor.expect_find_state("Foo", 1)?;
        assert_eq!(record.cursors[0].head_pos(), (0, 0), "{record:?}");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn submitting_find_query_advances_to_next_match() -> TestResult {
    support::run_x11_test("find-submit-next-match", |session| {
        let path = session.seed_file("find-next.txt", "foo bar\nbaz foo")?;
        let mut editor = session.open_file("find-submit-next-match", &path)?;

        editor.keys("<C-f>foo")?;
        let first = editor.expect_find_state("foo", 2)?;
        assert_eq!(first.cursors[0].head_pos(), (0, 0), "{first:?}");

        editor.keys("<enter>")?;
        let second = editor.wait_state("second find match", secs(5), |record| {
            record.find.visible
                && record.find.query == "foo"
                && record.find.match_count == 2
                && record.find.active_index == Some(1)
                && matches!(record.cursors.as_slice(), [cursor] if cursor.head_pos() == (1, 4))
        })?;
        assert_eq!(second.cursors[0].head_pos(), (1, 4), "{second:?}");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn case_sensitive_chip_disables_smart_case() -> TestResult {
    support::run_x11_test("find-chip-case-sensitive", |session| {
        let path = session.seed_file("find-chip-case.txt", "Foo foo FOO")?;
        let mut editor = session.open_file("find-chip-case-sensitive", &path)?;

        editor.keys("<C-f>foo")?;
        editor.expect_find_state("foo", 3)?;

        editor.click_find_chip(FindChip::CaseSensitive)?;
        let record = editor.wait_state("case-sensitive find", secs(5), |record| {
            record.find.visible
                && record.find.query == "foo"
                && record.find.case_sensitive
                && record.find.match_count == 1
        })?;
        assert!(record.find.case_sensitive, "{record:?}");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn whole_word_chip_restricts_matches_to_word_boundaries() -> TestResult {
    support::run_x11_test("find-chip-whole-word", |session| {
        let path = session.seed_file("find-chip-word.txt", "foobar foo_bar foo (foo) foo!")?;
        let mut editor = session.open_file("find-chip-whole-word", &path)?;

        editor.keys("<C-f>foo")?;
        editor.expect_find_state("foo", 5)?;

        editor.click_find_chip(FindChip::WholeWord)?;
        let record = editor.wait_state("whole-word find", secs(5), |record| {
            record.find.visible
                && record.find.query == "foo"
                && record.find.whole_word
                && record.find.match_count == 3
        })?;
        assert!(record.find.whole_word, "{record:?}");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn regex_chip_treats_query_as_pattern() -> TestResult {
    support::run_x11_test("find-chip-regex-pattern", |session| {
        let path = session.seed_file("find-chip-regex.txt", "foo foa fo.")?;
        let mut editor = session.open_file("find-chip-regex-pattern", &path)?;

        editor.keys("<C-f>fo.")?;
        editor.expect_find_state("fo.", 1)?;

        editor.click_find_chip(FindChip::Regex)?;
        let record = editor.wait_state("regex find", secs(5), |record| {
            record.find.visible
                && record.find.query == "fo."
                && record.find.use_regex
                && record.find.match_count == 3
        })?;
        assert!(record.find.use_regex, "{record:?}");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn invalid_regex_query_reports_error_and_clears_matches() -> TestResult {
    support::run_x11_test("find-chip-invalid-regex", |session| {
        let path = session.seed_file("find-chip-invalid-regex.txt", "[abc] [def]")?;
        let mut editor = session.open_file("find-chip-invalid-regex", &path)?;

        editor.keys("<C-f>[")?;
        editor.expect_find_state("[", 2)?;

        editor.click_find_chip(FindChip::Regex)?;
        let record = editor.wait_state("invalid regex find", secs(5), |record| {
            record.find.visible
                && record.find.query == "["
                && record.find.use_regex
                && record.find.match_count == 0
                && record.find.active_index.is_none()
                && record
                    .find
                    .error
                    .as_deref()
                    .is_some_and(|error| error.starts_with("regex:"))
        })?;
        assert!(record.find.error.is_some(), "{record:?}");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn find_query_ctrl_a_replaces_existing_query() -> TestResult {
    support::run_x11_test("find-query-ctrl-a-replace", |session| {
        let path = session.seed_file("find-query-replace.txt", "alpha beta\nalpha beta")?;
        let mut editor = session.open_file("find-query-ctrl-a-replace", &path)?;

        editor.keys("<C-f>alpha")?;
        editor.expect_find_state("alpha", 2)?;

        editor.keys("<C-a>beta")?;
        let record = editor.expect_find_state("beta", 2)?;
        assert_eq!(record.focused_input, "find_query", "{record:?}");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn find_respects_grapheme_boundaries_for_combining_clusters() -> TestResult {
    support::run_x11_test("find-grapheme-boundaries", |session| {
        let path = session.seed_file("find-grapheme.txt", "cafe\u{0301}\ncafe")?;
        let mut editor = session.open_file("find-grapheme-boundaries", &path)?;

        editor.keys("<C-f>e")?;
        let ascii = editor.expect_find_state("e", 1)?;
        assert_eq!(ascii.cursors[0].head_pos(), (1, 3), "{ascii:?}");

        write_clipboard_text(Selection::Clipboard, "e\u{0301}")?;
        editor.keys("<C-a><C-v>")?;
        let decomposed = editor.expect_find_state("e\u{0301}", 1)?;
        assert_eq!(decomposed.cursors[0].head_pos(), (0, 3), "{decomposed:?}");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn replace_current_match_only_rewrites_active_match() -> TestResult {
    support::run_x11_test("find-replace-one-active-match", |session| {
        let path = session.seed_file("replace-one.txt", "foo foo foo")?;
        let mut editor = session.open_file("find-replace-one-active-match", &path)?;

        editor.keys("<C-h>foo<tab>bar<enter>")?;
        editor.save_then_expect_file(&path, "bar foo foo")?;
        Ok(())
    })
}
