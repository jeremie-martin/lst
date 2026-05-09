use gpui::{Context, Div, InteractiveElement, Window};
use lst_editor::EditorCommand as Command;

use crate::{
    AddCursorAbove, AddCursorBelow, AddCursorsToLineEnds, Backspace, CleanupText, CloseActiveTab,
    CopySelection, CutSelection, DeleteForward, DeleteLine, DeleteWordBackward, DeleteWordForward,
    DuplicateLine, FindNext, FindOpen, FindOpenReplace, FindPrev, GotoLineOpen, InsertNewline,
    InsertTab, LstGpuiApp, MoveDocumentEnd, MoveDocumentStart, MoveDown, MoveLeft, MoveLineDown,
    MoveLineEnd, MoveLineStart, MoveLineUp, MovePageDown, MovePageUp, MoveRight, MoveSmartHome,
    MoveSubwordLeft, MoveSubwordRight, MoveTabLeft, MoveTabRight, MoveUp, MoveWordLeft,
    MoveWordRight, NewTab, NextBookmark, NextTab, OpenFile, OutdentSelection, PasteClipboard,
    PopSelectionCursor, PrevTab, PreviousBookmark, Quit, Redo, ReopenClosedTab, ReplaceAll,
    ReplaceOne, SaveFile, SaveFileAs, SelectAll, SelectAllOccurrences, SelectDocumentEnd,
    SelectDocumentStart, SelectDown, SelectFindMatches, SelectLeft, SelectLine, SelectLineEnd,
    SelectLineStart, SelectNextOccurrence, SelectPageDown, SelectPageUp, SelectParagraph,
    SelectRight, SelectSmartHome, SelectSubwordLeft, SelectSubwordRight, SelectUp, SelectWordLeft,
    SelectWordRight, SkipNextOccurrence, SwapRedoBranch, ToggleBlockComment, ToggleBookmark,
    ToggleComment, ToggleFindCase, ToggleFindInSelection, ToggleFindRegex, ToggleFindWholeWord,
    ToggleLineNumberMode, ToggleOvertype, ToggleRecentFiles, ToggleTheme, ToggleWrap,
    TransposeChars, Undo, ZoomIn, ZoomOut, ZoomReset,
};

pub(crate) fn attach_workspace_actions(root: Div, cx: &mut Context<LstGpuiApp>) -> Div {
    macro_rules! bind_commands {
        ($root:ident, $($action:ty => $command:expr;)*) => {
            $(
                let $root = $root.on_action(cx.listener(
                    |this, _: &$action, _: &mut Window, cx| {
                        this.clear_x11_modifier_chord_state();
                        this.execute_model_command(cx, $command);
                        cx.stop_propagation();
                    },
                ));
            )*
        };
    }

    bind_commands! {
        root,
        OpenFile => Command::RequestOpenFiles;
        SaveFile => Command::RequestSave;
        SaveFileAs => Command::RequestSaveAs;
        NextTab => Command::NextTab;
        PrevTab => Command::PrevTab;
        MoveTabLeft => Command::MoveActiveTab(-1);
        MoveTabRight => Command::MoveActiveTab(1);
        ToggleWrap => Command::ToggleWrap;
        ToggleLineNumberMode => Command::CycleGutterMode;
        CopySelection => Command::CopySelection;
        CutSelection => Command::CutSelection;
        PasteClipboard => Command::RequestPaste;
        MoveLeft => Command::MoveHorizontalCollapsed(true);
        MoveRight => Command::MoveHorizontalCollapsed(false);
        MoveWordLeft => Command::MoveWord(true, false);
        MoveWordRight => Command::MoveWord(false, false);
        MoveSubwordLeft => Command::MoveSubword(true, false);
        MoveSubwordRight => Command::MoveSubword(false, false);
        MoveDocumentStart => Command::MoveDocumentBoundary(false, false);
        MoveDocumentEnd => Command::MoveDocumentBoundary(true, false);
        SelectLeft => Command::MoveHorizontal(-1, true);
        SelectRight => Command::MoveHorizontal(1, true);
        SelectWordLeft => Command::MoveWord(true, true);
        SelectWordRight => Command::MoveWord(false, true);
        SelectSubwordLeft => Command::SmartShrinkSelection;
        SelectSubwordRight => Command::SmartExpandSelection;
        SelectDocumentStart => Command::MoveDocumentBoundary(false, true);
        SelectDocumentEnd => Command::MoveDocumentBoundary(true, true);
        MoveSmartHome => Command::SmartHome(false);
        MoveLineStart => Command::MoveLineBoundary(false, false);
        MoveLineEnd => Command::MoveLineBoundary(true, false);
        SelectSmartHome => Command::SmartHome(true);
        SelectLineStart => Command::MoveLineBoundary(false, true);
        SelectLineEnd => Command::MoveLineBoundary(true, true);
        Backspace => Command::Backspace;
        DeleteForward => Command::DeleteForward;
        DeleteWordBackward => Command::DeleteWord(true);
        DeleteWordForward => Command::DeleteWord(false);
        InsertNewline => Command::InsertNewline;
        InsertTab => Command::InsertTab;
        OutdentSelection => Command::Outdent;
        SelectAll => Command::SelectAll;
        SelectAllOccurrences => Command::SelectAllOccurrences;
        SelectFindMatches => Command::SelectAllFindMatches;
        SkipNextOccurrence => Command::SkipNextOccurrence;
        PopSelectionCursor => Command::PopPrimarySelectionCursor;
        AddCursorAbove => Command::AddCursorAbove;
        AddCursorBelow => Command::AddCursorBelow;
        AddCursorsToLineEnds => Command::AddCursorsToSelectedLineEnds;
        SelectLine => Command::SelectCurrentLine;
        SelectParagraph => Command::SelectCurrentParagraph;
        Undo => Command::Undo;
        Redo => Command::Redo;
        SwapRedoBranch => Command::SwapRedoBranch;
        FindOpen => Command::ToggleFindPanel(false);
        FindOpenReplace => Command::ToggleFindPanel(true);
        FindNext => Command::FindNext;
        FindPrev => Command::FindPrev;
        ReplaceOne => Command::ReplaceCurrentMatch;
        ReplaceAll => Command::ReplaceAllMatches;
        ToggleFindCase => Command::ToggleFindCaseSensitive;
        ToggleFindWholeWord => Command::ToggleFindWholeWord;
        ToggleFindRegex => Command::ToggleFindRegex;
        ToggleFindInSelection => Command::ToggleFindInSelection;
        GotoLineOpen => Command::ToggleGotoLinePanel;
        DeleteLine => Command::DeleteLine;
        MoveLineUp => Command::MoveLineUp;
        MoveLineDown => Command::MoveLineDown;
        DuplicateLine => Command::DuplicateLine;
        ToggleComment => Command::ToggleComment;
        ToggleBlockComment => Command::ToggleBlockComment;
        TransposeChars => Command::TransposeChars;
        ToggleOvertype => Command::ToggleOvertype;
        ToggleBookmark => Command::ToggleBookmark;
        NextBookmark => Command::JumpNextBookmark;
        PreviousBookmark => Command::JumpPreviousBookmark;
    }

    let root = root.on_action(cx.listener(|this, _: &NewTab, _window, cx| {
        this.clear_x11_modifier_chord_state();
        this.request_new_tab(cx);
        cx.stop_propagation();
    }));
    let root = root.on_action(cx.listener(|this, _: &ToggleRecentFiles, window, cx| {
        this.clear_x11_modifier_chord_state();
        this.toggle_recent_files_panel(window, cx);
        cx.stop_propagation();
    }));
    let root = root.on_action(cx.listener(|this, _: &ToggleTheme, _window, cx| {
        this.clear_x11_modifier_chord_state();
        this.cycle_theme(cx);
        cx.stop_propagation();
    }));
    let root = root.on_action(cx.listener(|this, _: &CleanupText, _window, cx| {
        this.clear_x11_modifier_chord_state();
        this.start_cleanup(cx);
        cx.stop_propagation();
    }));
    let root = root.on_action(cx.listener(|this, _: &SelectNextOccurrence, _window, cx| {
        let skip = this.x11_ctrl_k_pending;
        this.clear_x11_modifier_chord_state();
        this.update_model(cx, true, |model| {
            model.execute(if skip {
                Command::SkipNextOccurrence
            } else {
                Command::SelectNextOccurrence
            });
        });
        cx.stop_propagation();
    }));

    let root = root.on_action(cx.listener(|this, _: &MoveUp, window, cx| {
        this.clear_x11_modifier_chord_state();
        this.move_vertical(-1, false, window, cx);
        cx.stop_propagation();
    }));
    let root = root.on_action(cx.listener(|this, _: &MoveDown, window, cx| {
        this.clear_x11_modifier_chord_state();
        this.move_vertical(1, false, window, cx);
        cx.stop_propagation();
    }));
    let root = root.on_action(cx.listener(|this, _: &MovePageUp, window, cx| {
        this.clear_x11_modifier_chord_state();
        this.move_page(false, false, window, cx);
        cx.stop_propagation();
    }));
    let root = root.on_action(cx.listener(|this, _: &MovePageDown, window, cx| {
        this.clear_x11_modifier_chord_state();
        this.move_page(true, false, window, cx);
        cx.stop_propagation();
    }));
    let root = root.on_action(cx.listener(|this, _: &SelectUp, window, cx| {
        this.clear_x11_modifier_chord_state();
        this.move_vertical(-1, true, window, cx);
        cx.stop_propagation();
    }));
    let root = root.on_action(cx.listener(|this, _: &SelectDown, window, cx| {
        this.clear_x11_modifier_chord_state();
        this.move_vertical(1, true, window, cx);
        cx.stop_propagation();
    }));
    let root = root.on_action(cx.listener(|this, _: &SelectPageUp, window, cx| {
        this.clear_x11_modifier_chord_state();
        this.move_page(false, true, window, cx);
        cx.stop_propagation();
    }));
    let root = root.on_action(cx.listener(|this, _: &SelectPageDown, window, cx| {
        this.clear_x11_modifier_chord_state();
        this.move_page(true, true, window, cx);
        cx.stop_propagation();
    }));

    let root = root.on_action(cx.listener(|this, _: &CloseActiveTab, _window, cx| {
        this.clear_x11_modifier_chord_state();
        this.request_close_active_tab(cx);
        cx.stop_propagation();
    }));

    let root = root.on_action(cx.listener(|this, _: &ReopenClosedTab, _window, cx| {
        this.clear_x11_modifier_chord_state();
        this.reopen_recently_closed_tab(cx);
        cx.stop_propagation();
    }));

    let root = root.on_action(cx.listener(|this, _: &ZoomIn, window, cx| {
        this.clear_x11_modifier_chord_state();
        this.zoom_in(window, cx);
        cx.stop_propagation();
    }));
    let root = root.on_action(cx.listener(|this, _: &ZoomOut, window, cx| {
        this.clear_x11_modifier_chord_state();
        this.zoom_out(window, cx);
        cx.stop_propagation();
    }));
    let root = root.on_action(cx.listener(|this, _: &ZoomReset, window, cx| {
        this.clear_x11_modifier_chord_state();
        this.zoom_reset(window, cx);
        cx.stop_propagation();
    }));

    root.on_action(cx.listener(|this, _: &Quit, _window, cx| {
        this.clear_x11_modifier_chord_state();
        this.request_quit(cx);
        cx.stop_propagation();
    }))
}
