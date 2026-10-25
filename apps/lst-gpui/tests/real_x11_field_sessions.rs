//! Reusing a field starts a fresh edit session without reviving old local history.
mod support;

use lst_x11_harness::{
    clipboard::{wait_clipboard_text, write_clipboard_text},
    Selection,
};
use support::{secs, EditorTestExt, TestResult};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn reopening_empty_workspace_fields_cannot_restore_previous_undo_or_redo() -> TestResult {
    support::run_x11_test("field-sessions", |session| {
        let path = session.seed_file("unchanged.txt", "document stays unchanged")?;
        let mut editor = session.open_file("field-sessions", &path)?;
        let initial = editor.read_state()?;
        for open in ["<C-S-p>", "<C-,>", "<C-p>", "<C-f>"] {
            for (leave_empty, old_history) in [("a<bs>", "<C-z>"), ("a<C-z>", "<C-y>")] {
                write_clipboard_text(Selection::Clipboard, "before field copy")?;
                editor.keys(&format!(
                    "{open}<C-a><bs>{leave_empty}<esc>{open}{old_history}q<C-a><C-c>"
                ))?;
                // The copied q proves later input was handled by this field.
                // An old undo/redo entry would instead produce aq.
                wait_clipboard_text(Selection::Clipboard, "q", secs(3))?;
                assert_eq!(editor.read_state()?.revision, initial.revision);
                editor.keys("<esc>")?;
            }
        }
        Ok(())
    })
}
