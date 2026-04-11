use gpui::{Context, Window};
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

impl LstGpuiApp {
    pub(crate) fn handle_quit(&mut self, _: &Quit, _: &mut Window, cx: &mut Context<Self>) {
        cx.quit();
    }

    pub(crate) fn handle_new_tab(&mut self, _: &NewTab, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_model_command(EditorCommand::NewTab, cx);
    }

    pub(crate) fn handle_open_file(
        &mut self,
        _: &OpenFile,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(EditorCommand::RequestOpenFiles, cx);
    }

    pub(crate) fn handle_save_file(
        &mut self,
        _: &SaveFile,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(EditorCommand::RequestSave, cx);
    }

    pub(crate) fn handle_save_file_as(
        &mut self,
        _: &SaveFileAs,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(EditorCommand::RequestSaveAs, cx);
    }

    pub(crate) fn handle_close_active_tab(
        &mut self,
        _: &CloseActiveTab,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(EditorCommand::CloseTab(self.model.active), cx);
    }

    pub(crate) fn handle_next_tab(&mut self, _: &NextTab, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_model_command(EditorCommand::NextTab, cx);
    }

    pub(crate) fn handle_prev_tab(&mut self, _: &PrevTab, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_model_command(EditorCommand::PrevTab, cx);
    }

    pub(crate) fn handle_toggle_wrap(
        &mut self,
        _: &ToggleWrap,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(EditorCommand::ToggleWrap, cx);
    }

    pub(crate) fn handle_copy_selection(
        &mut self,
        _: &CopySelection,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(EditorCommand::CopySelection, cx);
    }

    pub(crate) fn handle_cut_selection(
        &mut self,
        _: &CutSelection,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(EditorCommand::CutSelection, cx);
    }

    pub(crate) fn handle_paste_clipboard(
        &mut self,
        _: &PasteClipboard,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(EditorCommand::RequestPaste, cx);
    }

    pub(crate) fn handle_move_left(
        &mut self,
        _: &MoveLeft,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(EditorCommand::MoveHorizontalCollapse { backward: true }, cx);
    }

    pub(crate) fn handle_move_right(
        &mut self,
        _: &MoveRight,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(
            EditorCommand::MoveHorizontalCollapse { backward: false },
            cx,
        );
    }

    pub(crate) fn handle_move_word_left(
        &mut self,
        _: &MoveWordLeft,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(
            EditorCommand::MoveWord {
                backward: true,
                select: false,
            },
            cx,
        );
    }

    pub(crate) fn handle_move_word_right(
        &mut self,
        _: &MoveWordRight,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(
            EditorCommand::MoveWord {
                backward: false,
                select: false,
            },
            cx,
        );
    }

    pub(crate) fn handle_move_up(
        &mut self,
        _: &MoveUp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_vertical(-1, false, window, cx);
    }

    pub(crate) fn handle_move_down(
        &mut self,
        _: &MoveDown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_vertical(1, false, window, cx);
    }

    pub(crate) fn handle_move_page_up(
        &mut self,
        _: &MovePageUp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_page(false, false, window, cx);
    }

    pub(crate) fn handle_move_page_down(
        &mut self,
        _: &MovePageDown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_page(true, false, window, cx);
    }

    pub(crate) fn handle_move_document_start(
        &mut self,
        _: &MoveDocumentStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(
            EditorCommand::MoveDocumentBoundary {
                to_end: false,
                select: false,
            },
            cx,
        );
    }

    pub(crate) fn handle_move_document_end(
        &mut self,
        _: &MoveDocumentEnd,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(
            EditorCommand::MoveDocumentBoundary {
                to_end: true,
                select: false,
            },
            cx,
        );
    }

    pub(crate) fn handle_select_left(
        &mut self,
        _: &SelectLeft,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(
            EditorCommand::MoveHorizontal {
                delta: -1,
                select: true,
            },
            cx,
        );
    }

    pub(crate) fn handle_select_right(
        &mut self,
        _: &SelectRight,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(
            EditorCommand::MoveHorizontal {
                delta: 1,
                select: true,
            },
            cx,
        );
    }

    pub(crate) fn handle_select_word_left(
        &mut self,
        _: &SelectWordLeft,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(
            EditorCommand::MoveWord {
                backward: true,
                select: true,
            },
            cx,
        );
    }

    pub(crate) fn handle_select_word_right(
        &mut self,
        _: &SelectWordRight,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(
            EditorCommand::MoveWord {
                backward: false,
                select: true,
            },
            cx,
        );
    }

    pub(crate) fn handle_select_up(
        &mut self,
        _: &SelectUp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_vertical(-1, true, window, cx);
    }

    pub(crate) fn handle_select_down(
        &mut self,
        _: &SelectDown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_vertical(1, true, window, cx);
    }

    pub(crate) fn handle_select_page_up(
        &mut self,
        _: &SelectPageUp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_page(false, true, window, cx);
    }

    pub(crate) fn handle_select_page_down(
        &mut self,
        _: &SelectPageDown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_page(true, true, window, cx);
    }

    pub(crate) fn handle_select_document_start(
        &mut self,
        _: &SelectDocumentStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(
            EditorCommand::MoveDocumentBoundary {
                to_end: false,
                select: true,
            },
            cx,
        );
    }

    pub(crate) fn handle_select_document_end(
        &mut self,
        _: &SelectDocumentEnd,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(
            EditorCommand::MoveDocumentBoundary {
                to_end: true,
                select: true,
            },
            cx,
        );
    }

    pub(crate) fn handle_move_line_start(
        &mut self,
        _: &MoveLineStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(
            EditorCommand::MoveLineBoundary {
                to_end: false,
                select: false,
            },
            cx,
        );
    }

    pub(crate) fn handle_move_line_end(
        &mut self,
        _: &MoveLineEnd,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(
            EditorCommand::MoveLineBoundary {
                to_end: true,
                select: false,
            },
            cx,
        );
    }

    pub(crate) fn handle_select_line_start(
        &mut self,
        _: &SelectLineStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(
            EditorCommand::MoveLineBoundary {
                to_end: false,
                select: true,
            },
            cx,
        );
    }

    pub(crate) fn handle_select_line_end(
        &mut self,
        _: &SelectLineEnd,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(
            EditorCommand::MoveLineBoundary {
                to_end: true,
                select: true,
            },
            cx,
        );
    }

    pub(crate) fn handle_backspace(
        &mut self,
        _: &Backspace,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(EditorCommand::Backspace, cx);
    }

    pub(crate) fn handle_delete_forward(
        &mut self,
        _: &DeleteForward,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(EditorCommand::DeleteForward, cx);
    }

    pub(crate) fn handle_delete_word_backward(
        &mut self,
        _: &DeleteWordBackward,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(EditorCommand::DeleteWord { backward: true }, cx);
    }

    pub(crate) fn handle_delete_word_forward(
        &mut self,
        _: &DeleteWordForward,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(EditorCommand::DeleteWord { backward: false }, cx);
    }

    pub(crate) fn handle_insert_newline(
        &mut self,
        _: &InsertNewline,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(EditorCommand::InsertNewline, cx);
    }

    pub(crate) fn handle_insert_tab(
        &mut self,
        _: &InsertTab,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(EditorCommand::InsertTab, cx);
    }

    pub(crate) fn handle_select_all(
        &mut self,
        _: &SelectAll,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(EditorCommand::SelectAll, cx);
        self.sync_primary_selection(cx);
    }

    pub(crate) fn handle_undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_model_command(EditorCommand::Undo, cx);
    }

    pub(crate) fn handle_redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_model_command(EditorCommand::Redo, cx);
    }

    pub(crate) fn handle_find_open(
        &mut self,
        _: &FindOpen,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(
            EditorCommand::ToggleFind {
                show_replace: false,
            },
            cx,
        );
    }

    pub(crate) fn handle_find_open_replace(
        &mut self,
        _: &FindOpenReplace,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(EditorCommand::ToggleFind { show_replace: true }, cx);
    }

    pub(crate) fn handle_find_next(
        &mut self,
        _: &FindNext,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(EditorCommand::FindNext, cx);
    }

    pub(crate) fn handle_find_prev(
        &mut self,
        _: &FindPrev,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(EditorCommand::FindPrev, cx);
    }

    pub(crate) fn handle_replace_one(
        &mut self,
        _: &ReplaceOne,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(EditorCommand::ReplaceOne, cx);
    }

    pub(crate) fn handle_replace_all(
        &mut self,
        _: &ReplaceAll,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(EditorCommand::ReplaceAll, cx);
    }

    pub(crate) fn handle_goto_line_open(
        &mut self,
        _: &GotoLineOpen,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(EditorCommand::ToggleGotoLine, cx);
    }

    pub(crate) fn handle_delete_line(
        &mut self,
        _: &DeleteLine,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(EditorCommand::DeleteLine, cx);
    }

    pub(crate) fn handle_move_line_up(
        &mut self,
        _: &MoveLineUp,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(EditorCommand::MoveLineUp, cx);
    }

    pub(crate) fn handle_move_line_down(
        &mut self,
        _: &MoveLineDown,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(EditorCommand::MoveLineDown, cx);
    }

    pub(crate) fn handle_duplicate_line(
        &mut self,
        _: &DuplicateLine,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(EditorCommand::DuplicateLine, cx);
    }

    pub(crate) fn handle_toggle_comment(
        &mut self,
        _: &ToggleComment,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(EditorCommand::ToggleComment, cx);
    }
}
