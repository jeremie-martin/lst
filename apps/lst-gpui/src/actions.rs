use gpui::{Context, Div, InteractiveElement, Window};
use lst_editor::EditorCommand;

use crate::{
    Backspace, CloseActiveTab, CopySelection, CutSelection, DeleteForward, DeleteLine,
    DeleteWordBackward, DeleteWordForward, DuplicateLine, FindNext, FindOpen, FindOpenReplace,
    FindPrev, GotoLineOpen, InsertNewline, InsertTab, LstGpuiApp, MoveDocumentEnd,
    MoveDocumentStart, MoveDown, MoveLeft, MoveLineDown, MoveLineEnd, MoveLineStart, MoveLineUp,
    MovePageDown, MovePageUp, MoveRight, MoveUp, MoveWordLeft, MoveWordRight, NewTab, NextTab,
    OpenFile, PasteClipboard, PrevTab, Quit, Redo, ReplaceAll, ReplaceOne, SaveFile, SaveFileAs,
    SelectAll, SelectDocumentEnd, SelectDocumentStart, SelectDown, SelectLeft, SelectLineEnd,
    SelectLineStart, SelectPageDown, SelectPageUp, SelectRight, SelectUp, SelectWordLeft,
    SelectWordRight, ToggleComment, ToggleWrap, Undo,
};

pub(crate) fn attach_workspace_actions(root: Div, cx: &mut Context<LstGpuiApp>) -> Div {
    macro_rules! bind_commands {
        ($root:ident, $($action:ty => $command:expr;)*) => {
            $(
                let $root = $root.on_action(cx.listener(
                    |this, _: &$action, _: &mut Window, cx| {
                        this.apply_model_command($command, cx);
                    },
                ));
            )*
        };
    }

    bind_commands! {
        root,
        NewTab => EditorCommand::NewTab;
        OpenFile => EditorCommand::RequestOpenFiles;
        SaveFile => EditorCommand::RequestSave;
        SaveFileAs => EditorCommand::RequestSaveAs;
        CloseActiveTab => EditorCommand::CloseActiveTab;
        NextTab => EditorCommand::NextTab;
        PrevTab => EditorCommand::PrevTab;
        ToggleWrap => EditorCommand::ToggleWrap;
        CopySelection => EditorCommand::CopySelection;
        CutSelection => EditorCommand::CutSelection;
        PasteClipboard => EditorCommand::RequestPaste;
        MoveLeft => EditorCommand::MoveHorizontalCollapse { backward: true };
        MoveRight => EditorCommand::MoveHorizontalCollapse { backward: false };
        MoveWordLeft => EditorCommand::MoveWord { backward: true, select: false };
        MoveWordRight => EditorCommand::MoveWord { backward: false, select: false };
        MoveDocumentStart => EditorCommand::MoveDocumentBoundary { to_end: false, select: false };
        MoveDocumentEnd => EditorCommand::MoveDocumentBoundary { to_end: true, select: false };
        SelectLeft => EditorCommand::MoveHorizontal { delta: -1, select: true };
        SelectRight => EditorCommand::MoveHorizontal { delta: 1, select: true };
        SelectWordLeft => EditorCommand::MoveWord { backward: true, select: true };
        SelectWordRight => EditorCommand::MoveWord { backward: false, select: true };
        SelectDocumentStart => EditorCommand::MoveDocumentBoundary { to_end: false, select: true };
        SelectDocumentEnd => EditorCommand::MoveDocumentBoundary { to_end: true, select: true };
        MoveLineStart => EditorCommand::MoveLineBoundary { to_end: false, select: false };
        MoveLineEnd => EditorCommand::MoveLineBoundary { to_end: true, select: false };
        SelectLineStart => EditorCommand::MoveLineBoundary { to_end: false, select: true };
        SelectLineEnd => EditorCommand::MoveLineBoundary { to_end: true, select: true };
        Backspace => EditorCommand::Backspace;
        DeleteForward => EditorCommand::DeleteForward;
        DeleteWordBackward => EditorCommand::DeleteWord { backward: true };
        DeleteWordForward => EditorCommand::DeleteWord { backward: false };
        InsertNewline => EditorCommand::InsertNewline;
        InsertTab => EditorCommand::InsertTab;
        SelectAll => EditorCommand::SelectAll;
        Undo => EditorCommand::Undo;
        Redo => EditorCommand::Redo;
        FindOpen => EditorCommand::ToggleFind { show_replace: false };
        FindOpenReplace => EditorCommand::ToggleFind { show_replace: true };
        FindNext => EditorCommand::FindNext;
        FindPrev => EditorCommand::FindPrev;
        ReplaceOne => EditorCommand::ReplaceOne;
        ReplaceAll => EditorCommand::ReplaceAll;
        GotoLineOpen => EditorCommand::ToggleGotoLine;
        DeleteLine => EditorCommand::DeleteLine;
        MoveLineUp => EditorCommand::MoveLineUp;
        MoveLineDown => EditorCommand::MoveLineDown;
        DuplicateLine => EditorCommand::DuplicateLine;
        ToggleComment => EditorCommand::ToggleComment;
    }

    let root = root.on_action(cx.listener(|this, _: &MoveUp, window, cx| {
        this.move_vertical(-1, false, window, cx);
    }));
    let root = root.on_action(cx.listener(|this, _: &MoveDown, window, cx| {
        this.move_vertical(1, false, window, cx);
    }));
    let root = root.on_action(cx.listener(|this, _: &MovePageUp, window, cx| {
        this.move_page(false, false, window, cx);
    }));
    let root = root.on_action(cx.listener(|this, _: &MovePageDown, window, cx| {
        this.move_page(true, false, window, cx);
    }));
    let root = root.on_action(cx.listener(|this, _: &SelectUp, window, cx| {
        this.move_vertical(-1, true, window, cx);
    }));
    let root = root.on_action(cx.listener(|this, _: &SelectDown, window, cx| {
        this.move_vertical(1, true, window, cx);
    }));
    let root = root.on_action(cx.listener(|this, _: &SelectPageUp, window, cx| {
        this.move_page(false, true, window, cx);
    }));
    let root = root.on_action(cx.listener(|this, _: &SelectPageDown, window, cx| {
        this.move_page(true, true, window, cx);
    }));

    root.on_action(cx.listener(|_, _: &Quit, _: &mut Window, cx| cx.quit()))
}
