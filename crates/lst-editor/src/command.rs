use lst_core::document::UndoBoundary;
use std::{ops::Range, path::PathBuf};

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditorCommand {
    InsertText(String),
    ReplaceText {
        range: Option<Range<usize>>,
        text: String,
        boundary: UndoBoundary,
    },
    ReplaceTextFromInput {
        range: Option<Range<usize>>,
        text: String,
    },
    ReplaceAndMarkText {
        range: Option<Range<usize>>,
        text: String,
        selected_range: Option<Range<usize>>,
    },
    ClearMarkedText,
    NewTab,
    CloseActiveTab,
    CloseTab(usize),
    SetActiveTab(usize),
    NextTab,
    PrevTab,
    ToggleWrap,
    SelectAll,
    MoveHorizontal {
        delta: isize,
        select: bool,
    },
    MoveHorizontalCollapse {
        backward: bool,
    },
    MoveVertical {
        delta: isize,
        select: bool,
    },
    MoveDisplayRows {
        delta: isize,
        select: bool,
        wrap_columns: usize,
    },
    MovePage {
        rows: usize,
        down: bool,
        select: bool,
    },
    MoveWord {
        backward: bool,
        select: bool,
    },
    MoveLineBoundary {
        to_end: bool,
        select: bool,
    },
    MoveDocumentBoundary {
        to_end: bool,
        select: bool,
    },
    MoveToChar {
        offset: usize,
        select: bool,
        preferred_column: Option<usize>,
    },
    SetSelection {
        range: Range<usize>,
        reversed: bool,
    },
    Backspace,
    DeleteForward,
    DeleteWord {
        backward: bool,
    },
    InsertNewline,
    InsertTab,
    DeleteLine,
    MoveLineUp,
    MoveLineDown,
    DuplicateLine,
    ToggleComment,
    CopySelection,
    CutSelection,
    RequestPaste,
    ClipboardUnavailable,
    PasteText(String),
    OpenFind {
        show_replace: bool,
    },
    ToggleFind {
        show_replace: bool,
    },
    CloseFind,
    SetFindQuery(String),
    SetFindQueryAndSelect(String),
    SetFindReplacement(String),
    FindNext,
    FindPrev,
    ReplaceOne,
    ReplaceAll,
    OpenGotoLine,
    ToggleGotoLine,
    CloseGotoLine,
    SetGotoLine(String),
    SubmitGotoLine,
    RequestOpenFiles,
    OpenFiles(Vec<(PathBuf, String)>),
    OpenFileFailed {
        path: PathBuf,
        message: String,
    },
    RequestSave,
    RequestSaveAs,
    SaveFinished {
        path: PathBuf,
    },
    SaveFailed {
        path: PathBuf,
        message: String,
    },
    AutosaveTick,
    AutosaveFinished {
        path: PathBuf,
        revision: u64,
    },
    AutosaveFailed {
        path: PathBuf,
        message: String,
    },
    Undo,
    Redo,
}
