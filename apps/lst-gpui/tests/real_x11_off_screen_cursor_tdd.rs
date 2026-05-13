//! Under-review executable specs for off-screen cursor indicators and
//! viewport reveal across `Shift+Alt+Down` cursor extension.
//!
//! Pinned contract:
//!
//! - When N active cursors lie above the painted viewport, the status bar
//!   contains the substring `"▲<N>"`. When N lie below, it contains
//!   `"▼<N>"`. With no off-screen cursors, neither substring appears.
//! - When `Shift+Alt+Down` extends the cursor cluster past the painted
//!   viewport's bottom edge, the viewport scrolls so the bottom-most
//!   cursor is visible. The harness can only observe document-ordered
//!   cursors via the trace, so the assertion is on the bottom-most
//!   cursor's line — under this gesture the bottom-most cursor is also
//!   the most recently added one.
//!
//! Tests compute the off-screen counts directly from
//! `record.viewport.rows` and `record.cursors`, so they are robust against
//! viewport size changes between hosts.
//!
//! Specs run under the `x11-tdd` profile. Promote to
//! `real_x11_off_screen_cursor.rs` once green.
//!
//!     cargo nextest run --profile x11-tdd -p lst-gpui --test real_x11_off_screen_cursor_tdd --run-ignored only

mod support;

use lst_x11_harness::StateTraceRecord;

use support::{EditorTestExt, TestResult};

fn hundred_line_fixture() -> String {
    (0..100)
        .map(|i| format!("line{i:03}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Returns the smallest and largest logical line currently painted in the
/// viewport. Falls back to None when the viewport has not painted anything
/// (in which case the test is bogus and should fail loudly).
fn painted_line_range(record: &StateTraceRecord) -> Option<(usize, usize)> {
    let mut min = usize::MAX;
    let mut max = 0usize;
    for row in &record.viewport.rows {
        min = min.min(row.logical_line);
        max = max.max(row.logical_line);
    }
    if record.viewport.rows.is_empty() {
        None
    } else {
        Some((min, max))
    }
}

fn count_cursors_outside(record: &StateTraceRecord, painted: (usize, usize)) -> (usize, usize) {
    let (top, bottom) = painted;
    let mut above = 0usize;
    let mut below = 0usize;
    for cursor in &record.cursors {
        if cursor.head_line < top {
            above += 1;
        } else if cursor.head_line > bottom {
            below += 1;
        }
    }
    (above, below)
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn status_bar_shows_count_when_secondary_cursors_are_below_viewport() -> TestResult {
    support::run_x11_test("off-screen-below", |session| {
        let path = session.seed_file("off-screen-below.txt", &hundred_line_fixture())?;
        let mut editor = session.open_file("off-screen-below", &path)?;

        // Caret at line 99; <S-A-up> adds cursors above (at 98, 97, ...).
        // Reveal targets the newly-added cursor near the top of the cluster,
        // so the cursors near line 99 fall below the viewport.
        editor.keys("<C-end>")?;
        let visible_rows = editor.read_state()?.viewport.rows.len().max(1);
        for _ in 0..(visible_rows + 5).min(90) {
            editor.keys("<S-A-up>")?;
        }

        let record = editor.read_state()?;
        let painted = painted_line_range(&record).expect("viewport must have painted rows");
        let (_above, below) = count_cursors_outside(&record, painted);
        assert!(
            below > 0,
            "test setup did not produce below-viewport cursors; viewport {painted:?}, cursors {:?}",
            record
                .cursors
                .iter()
                .map(|c| c.head_line)
                .collect::<Vec<_>>()
        );

        let expected = format!("▼{below}");
        assert!(
            record.status_bar.contains(&expected),
            "status bar should mention {expected:?}; got {:?}",
            record.status_bar
        );
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn status_bar_shows_count_when_secondary_cursors_are_above_viewport() -> TestResult {
    support::run_x11_test("off-screen-above", |session| {
        let path = session.seed_file("off-screen-above.txt", &hundred_line_fixture())?;
        let mut editor = session.open_file("off-screen-above", &path)?;

        // Caret at line 0; <S-A-down> adds cursors below. Reveal targets
        // the newly-added cursor near the bottom of the cluster, so the
        // cursors near line 0 fall above the viewport.
        editor.place_cursor_at_document_start()?;
        let visible_rows = editor.read_state()?.viewport.rows.len().max(1);
        for _ in 0..(visible_rows + 5).min(90) {
            editor.keys("<S-A-down>")?;
        }

        let record = editor.read_state()?;
        let painted = painted_line_range(&record).expect("viewport must have painted rows");
        let (above, _below) = count_cursors_outside(&record, painted);
        assert!(
            above > 0,
            "test setup did not produce above-viewport cursors; viewport {painted:?}, cursors {:?}",
            record
                .cursors
                .iter()
                .map(|c| c.head_line)
                .collect::<Vec<_>>()
        );

        let expected = format!("▲{above}");
        assert!(
            record.status_bar.contains(&expected),
            "status bar should mention {expected:?}; got {:?}",
            record.status_bar
        );
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn shift_alt_down_keeps_bottom_most_cursor_visible_after_extending_past_viewport() -> TestResult {
    support::run_x11_test("off-screen-reveal-bottom-most", |session| {
        let path = session.seed_file("reveal-bottom-most.txt", &hundred_line_fixture())?;
        let mut editor = session.open_file("off-screen-reveal-bottom-most", &path)?;

        editor.place_cursor_at_document_start()?;
        // Extend the cursor cluster well past a typical viewport's bottom
        // edge. The viewport must scroll so the bottom-most cursor stays
        // visible — otherwise the user cannot see the leading edge of the
        // cluster they're building.
        let visible_rows = editor.read_state()?.viewport.rows.len().max(1);
        for _ in 0..(visible_rows + 5).min(90) {
            editor.keys("<S-A-down>")?;
        }

        let record = editor.read_state()?;
        let bottom_cursor_line = record
            .cursors
            .last()
            .expect("at least one cursor")
            .head_line;
        let visible = record
            .viewport
            .rows
            .iter()
            .any(|row| row.logical_line == bottom_cursor_line);
        assert!(
            visible,
            "viewport rows {:?} must include the bottom-most cursor's line {bottom_cursor_line}",
            record
                .viewport
                .rows
                .iter()
                .map(|row| row.logical_line)
                .collect::<Vec<_>>()
        );
        Ok(())
    })
}
