use gpui::{Context, Div, InteractiveElement, Window};

use crate::{
    Backspace, CloseActiveTab, CopySelection, CutSelection, DeleteForward, DeleteLine,
    DeleteWordBackward, DeleteWordForward, DuplicateLine, FindNext, FindOpen, FindOpenReplace,
    FindPrev, GotoLineOpen, InsertNewline, InsertTab, LstGpuiApp, ModelInputSync, MoveDocumentEnd,
    MoveDocumentStart, MoveDown, MoveLeft, MoveLineDown, MoveLineEnd, MoveLineStart, MoveLineUp,
    MovePageDown, MovePageUp, MoveRight, MoveUp, MoveWordLeft, MoveWordRight, NewTab, NextTab,
    OpenFile, PasteClipboard, PrevTab, Quit, Redo, ReplaceAll, ReplaceOne, SaveFile, SaveFileAs,
    SelectAll, SelectDocumentEnd, SelectDocumentStart, SelectDown, SelectLeft, SelectLineEnd,
    SelectLineStart, SelectPageDown, SelectPageUp, SelectRight, SelectUp, SelectWordLeft,
    SelectWordRight, ToggleComment, ToggleWrap, Undo,
};

pub(crate) fn attach_workspace_actions(root: Div, cx: &mut Context<LstGpuiApp>) -> Div {
    macro_rules! bind_commands {
        ($root:ident, $($action:ty => $sync:expr, $update:expr;)*) => {
            $(
                let $root = $root.on_action(cx.listener(
                    |this, _: &$action, _: &mut Window, cx| {
                        this.update_model(cx, $sync, true, $update);
                    },
                ));
            )*
        };
    }

    bind_commands! {
        root,
        NewTab => ModelInputSync::None, |model| model.new_tab();
        OpenFile => ModelInputSync::None, |model| model.request_open_files();
        SaveFile => ModelInputSync::None, |model| model.request_save();
        SaveFileAs => ModelInputSync::None, |model| model.request_save_as();
        CloseActiveTab => ModelInputSync::None, |model| model.close_active_tab();
        NextTab => ModelInputSync::None, |model| model.next_tab();
        PrevTab => ModelInputSync::None, |model| model.prev_tab();
        ToggleWrap => ModelInputSync::None, |model| model.toggle_wrap();
        CopySelection => ModelInputSync::None, |model| model.copy_selection();
        CutSelection => ModelInputSync::None, |model| model.cut_selection();
        PasteClipboard => ModelInputSync::None, |model| model.request_paste();
        MoveLeft => ModelInputSync::None, |model| model.move_horizontal_collapsed(true);
        MoveRight => ModelInputSync::None, |model| model.move_horizontal_collapsed(false);
        MoveWordLeft => ModelInputSync::None, |model| model.move_word(true, false);
        MoveWordRight => ModelInputSync::None, |model| model.move_word(false, false);
        MoveDocumentStart => ModelInputSync::None, |model| model.move_document_boundary(false, false);
        MoveDocumentEnd => ModelInputSync::None, |model| model.move_document_boundary(true, false);
        SelectLeft => ModelInputSync::None, |model| model.move_horizontal_by(-1, true);
        SelectRight => ModelInputSync::None, |model| model.move_horizontal_by(1, true);
        SelectWordLeft => ModelInputSync::None, |model| model.move_word(true, true);
        SelectWordRight => ModelInputSync::None, |model| model.move_word(false, true);
        SelectDocumentStart => ModelInputSync::None, |model| model.move_document_boundary(false, true);
        SelectDocumentEnd => ModelInputSync::None, |model| model.move_document_boundary(true, true);
        MoveLineStart => ModelInputSync::None, |model| model.move_line_boundary(false, false);
        MoveLineEnd => ModelInputSync::None, |model| model.move_line_boundary(true, false);
        SelectLineStart => ModelInputSync::None, |model| model.move_line_boundary(false, true);
        SelectLineEnd => ModelInputSync::None, |model| model.move_line_boundary(true, true);
        Backspace => ModelInputSync::None, |model| model.backspace();
        DeleteForward => ModelInputSync::None, |model| model.delete_forward();
        DeleteWordBackward => ModelInputSync::None, |model| model.delete_word(true);
        DeleteWordForward => ModelInputSync::None, |model| model.delete_word(false);
        InsertNewline => ModelInputSync::None, |model| model.insert_newline_at_cursor();
        InsertTab => ModelInputSync::None, |model| model.insert_tab_at_cursor();
        SelectAll => ModelInputSync::None, |model| model.select_all();
        Undo => ModelInputSync::None, |model| model.undo();
        Redo => ModelInputSync::None, |model| model.redo();
        FindOpen => ModelInputSync::Find, |model| model.toggle_find_panel(false);
        FindOpenReplace => ModelInputSync::Find, |model| model.toggle_find_panel(true);
        FindNext => ModelInputSync::None, |model| model.find_next_match();
        FindPrev => ModelInputSync::None, |model| model.find_prev_match();
        ReplaceOne => ModelInputSync::None, |model| model.replace_current_match();
        ReplaceAll => ModelInputSync::None, |model| model.replace_all_matches_in_document();
        GotoLineOpen => ModelInputSync::Goto, |model| model.toggle_goto_line_panel();
        DeleteLine => ModelInputSync::None, |model| model.delete_line();
        MoveLineUp => ModelInputSync::None, |model| model.move_line_up();
        MoveLineDown => ModelInputSync::None, |model| model.move_line_down();
        DuplicateLine => ModelInputSync::None, |model| model.duplicate_line();
        ToggleComment => ModelInputSync::None, |model| model.toggle_comment();
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
