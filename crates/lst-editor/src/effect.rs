use crate::{FileStamp, TabId};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusTarget {
    Editor,
    FindQuery,
    FindReplace,
    GotoLine,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditorEffect {
    Focus(FocusTarget),
    RevealCursor,
    WriteClipboard(String),
    WritePrimary(String),
    ReadClipboard,
    OpenFiles,
    SaveFile {
        tab_id: TabId,
        path: PathBuf,
        body: String,
        expected_stamp: Option<FileStamp>,
    },
    SaveFileAs {
        tab_id: TabId,
        suggested_name: String,
        body: String,
        previous_scratchpad_path: Option<PathBuf>,
    },
    AutosaveFile {
        tab_id: TabId,
        path: PathBuf,
        body: String,
        revision: u64,
        expected_stamp: Option<FileStamp>,
    },
}
