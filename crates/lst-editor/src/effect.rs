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
        path: PathBuf,
        body: String,
    },
    SaveFileAs {
        suggested_name: String,
        body: String,
    },
    AutosaveFile {
        path: PathBuf,
        body: String,
        revision: u64,
    },
}
