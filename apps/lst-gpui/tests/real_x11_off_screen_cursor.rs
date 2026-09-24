//! Accepted real-X11 behavior for off-screen cursor indicators and viewport
//! reveal across adjacent-line cursor extension.
//!
//! Pinned contract:
//!
//! - When N active cursors lie above the painted viewport, the status bar
//!   contains the substring `"▲<N>"`. When N lie below, it contains
//!   `"▼<N>"`.
//! - When `Ctrl+Alt+Down` extends the cursor cluster past the painted
//!   viewport's bottom edge, the viewport scrolls so the bottom-most cursor,
//!   the one just added, is visible.
//!
//! Tests compute the off-screen counts from the same trace record's
//! `viewport.rows` and `cursors`, so they are robust against viewport size
//! changes between hosts.

mod support;

use lst_x11_harness::StateTraceRecord;

use support::{secs, EditorTestExt, TestResult};

fn hundred_line_fixture() -> String {
    (0..100).map(|i| format!("line{i:03}")).collect::<Vec<_>>().join("\n")
}

/// Count the cursors above and below the painted rows of `record`, or `None`
/// before anything is painted.
fn cursors_outside_viewport(record: &StateTraceRecord) -> Option<(usize, usize)> {
    let top = record.viewport.rows.iter().map(|row| row.logical_line).min()?;
    let bottom = record.viewport.rows.iter().map(|row| row.logical_line).max()?;
    let above = record.cursors.iter().filter(|cursor| cursor.head_line < top).count();
    let below = record.cursors.iter().filter(|cursor| cursor.head_line > bottom).count();
    Some((above, below))
}

fn line_is_painted(record: &StateTraceRecord, line: usize) -> bool {
    record.viewport.rows.iter().any(|row| row.logical_line == line)
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn status_bar_shows_count_when_secondary_cursors_are_below_viewport() -> TestResult {
    support::run_x11_test("off-screen-below", |session| {
        let path = session.seed_file("off-screen-below.txt", &hundred_line_fixture())?;
        let mut editor = session.open_file("off-screen-below", &path)?;

        // Caret at line 99; <C-A-up> adds cursors above. Reveal follows the
        // newest cursor near the top of the cluster, so the cursors near
        // line 99 fall below the viewport.
        editor.keys("<C-end>")?;
        let visible_rows = editor.expect_cursor_heads(&[(99, 7)])?.viewport.rows.len();
        let added = (visible_rows + 5).min(90);
        editor.keys(&"<C-A-up>".repeat(added))?;

        editor.wait_state("below-viewport count in status bar", secs(5), |record| {
            record.cursors.len() == added + 1
                && cursors_outside_viewport(record)
                    .is_some_and(|(_, below)| below > 0 && record.status_bar.contains(&format!("▼{below}")))
        })?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_alt_down_past_viewport_reveals_newest_cursor_and_counts_those_above() -> TestResult {
    support::run_x11_test("off-screen-above", |session| {
        let path = session.seed_file("off-screen-above.txt", &hundred_line_fixture())?;
        let mut editor = session.open_file("off-screen-above", &path)?;

        editor.place_cursor_at_document_start()?;
        let visible_rows = editor.read_state()?.viewport.rows.len();
        let added = (visible_rows + 5).min(90);
        editor.keys(&"<C-A-down>".repeat(added))?;

        editor.wait_state("bottom-most cursor revealed and above count shown", secs(5), |record| {
            record.cursors.len() == added + 1
                && record
                    .cursors
                    .last()
                    .is_some_and(|cursor| line_is_painted(record, cursor.head_line))
                && cursors_outside_viewport(record)
                    .is_some_and(|(above, _)| above > 0 && record.status_bar.contains(&format!("▲{above}")))
        })?;
        Ok(())
    })
}
