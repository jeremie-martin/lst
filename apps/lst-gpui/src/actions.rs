use gpui::{Context, Div, InteractiveElement, Window};
use lst_editor::EditorCommand as Command;

use crate::{
    AddCursorAbove, AddCursorBelow, AddCursorsToLineEnds, Backspace, CleanupText, CloseActiveTab, CopySelection,
    CutSelection, DeleteForward, DeleteLine, DeleteWordBackward, DeleteWordForward, DuplicateLine, FindNext, FindOpen,
    FindOpenReplace, FindPrev, GotoLineOpen, InsertNewline, InsertTab, LstGpuiApp, MoveDocumentEnd, MoveDocumentStart,
    MoveDown, MoveLeft, MoveLineDown, MoveLineEnd, MoveLineStart, MoveLineUp, MovePageDown, MovePageUp, MoveRight,
    MoveSmartHome, MoveSubwordLeft, MoveSubwordRight, MoveTabLeft, MoveTabRight, MoveUp, MoveWordLeft, MoveWordRight,
    NewTab, NextBookmark, NextTab, OpenFile, OutdentSelection, PasteClipboard, PopSelectionCursor, PrevTab,
    PreviousBookmark, Quit, Redo, ReopenClosedTab, ReplaceAll, ReplaceOne, SaveFile, SaveFileAs, SelectAll,
    SelectAllOccurrences, SelectDocumentEnd, SelectDocumentStart, SelectDown, SelectFindMatches, SelectLeft,
    SelectLine, SelectLineEnd, SelectLineStart, SelectNextOccurrence, SelectPageDown, SelectPageUp, SelectParagraph,
    SelectRight, SelectSmartHome, SelectSubwordLeft, SelectSubwordRight, SelectUp, SelectWordLeft, SelectWordRight,
    SkipNextOccurrence, SwapRedoBranch, ToggleBlockComment, ToggleBookmark, ToggleComment, ToggleFindCase,
    ToggleFindInSelection, ToggleFindRegex, ToggleFindWholeWord, ToggleLineNumberMode, ToggleOvertype,
    ToggleRecentFiles, ToggleTheme, ToggleWrap, TransposeChars, Undo, ZoomIn, ZoomOut, ZoomReset,
};

pub(crate) fn attach_workspace_actions(root: Div, cx: &mut Context<LstGpuiApp>) -> Div {
    macro_rules! cmd {
        ($root:ident, $($action:ty => $command:expr;)*) => {
            $(let $root = $root.on_action(cx.listener(
                |this, _: &$action, _: &mut Window, cx| {
                    this.clear_x11_modifier_chord_state();
                    this.execute_model_command(cx, $command);
                    cx.stop_propagation();
                },
            ));)*
        };
    }
    macro_rules! call {
        ($root:ident, $($action:ty => |$this:ident, $win:ident, $context:ident| $body:expr;)*) => {
            $(let $root = $root.on_action(cx.listener(
                |$this, _: &$action, $win: &mut Window, $context| {
                    $this.clear_x11_modifier_chord_state();
                    $body;
                    $context.stop_propagation();
                },
            ));)*
        };
    }

    cmd! { root,
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

    call! { root,
        NewTab => |this, _window, cx| this.request_new_tab(cx);
        ToggleRecentFiles => |this, window, cx| this.toggle_recent_files_panel(window, cx);
        ToggleTheme => |this, _window, cx| this.cycle_theme(cx);
        CleanupText => |this, _window, cx| this.start_cleanup(cx);
        MoveUp => |this, window, cx| this.move_vertical(-1, false, window, cx);
        MoveDown => |this, window, cx| this.move_vertical(1, false, window, cx);
        MovePageUp => |this, window, cx| this.move_page(false, false, window, cx);
        MovePageDown => |this, window, cx| this.move_page(true, false, window, cx);
        SelectUp => |this, window, cx| this.move_vertical(-1, true, window, cx);
        SelectDown => |this, window, cx| this.move_vertical(1, true, window, cx);
        SelectPageUp => |this, window, cx| this.move_page(false, true, window, cx);
        SelectPageDown => |this, window, cx| this.move_page(true, true, window, cx);
        CloseActiveTab => |this, _window, cx| this.request_close_active_tab(cx);
        ReopenClosedTab => |this, _window, cx| this.reopen_recently_closed_tab(cx);
        ZoomIn => |this, window, cx| this.zoom_in(window, cx);
        ZoomOut => |this, window, cx| this.zoom_out(window, cx);
        ZoomReset => |this, window, cx| this.zoom_reset(window, cx);
        Quit => |this, _window, cx| this.request_quit(cx);
    }

    // Read the x11_ctrl_k_pending flag BEFORE clearing modifier-chord state,
    // so a `ctrl-k ctrl-d` chord routes to SkipNextOccurrence instead of
    // SelectNextOccurrence. The cmd! / call! macros above clear chord state
    // first, which would race the flag away.
    let root = root.on_action(cx.listener(|this, _: &SelectNextOccurrence, _: &mut Window, cx| {
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

    root
}
