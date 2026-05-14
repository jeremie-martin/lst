//! Real-display specs for accepted find-panel behavior.

mod support;

use support::{secs, EditorTestExt, TestResult};

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
