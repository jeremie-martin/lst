use gpui::{Context, Div, InteractiveElement, Window};

use crate::{
    Backspace, CloseActiveTab, CopySelection, CutSelection, DeleteForward, DeleteLine,
    DeleteWordBackward, DeleteWordForward, DuplicateLine, FindNext, FindOpen, FindOpenReplace,
    FindPrev, GotoLineOpen, InsertNewline, InsertTab, LstGpuiApp, MoveDocumentEnd,
    MoveDocumentStart, MoveDown, MoveLeft, MoveLineDown, MoveLineEnd, MoveLineStart, MoveLineUp,
    MovePageDown, MovePageUp, MoveRight, MoveSmartHome, MoveSubwordLeft, MoveSubwordRight, MoveUp,
    MoveWordLeft, MoveWordRight, NewTab, NextTab, OpenFile, OutdentSelection, PasteClipboard,
    PrevTab, Quit, Redo, ReplaceAll, ReplaceOne, SaveFile, SaveFileAs, SelectAll,
    SelectDocumentEnd, SelectDocumentStart, SelectDown, SelectLeft, SelectLineEnd, SelectLineStart,
    SelectPageDown, SelectPageUp, SelectRight, SelectSmartHome, SelectSubwordLeft,
    SelectSubwordRight, SelectUp, SelectWordLeft, SelectWordRight, ToggleComment, ToggleWrap, Undo,
    ZoomIn, ZoomOut, ZoomReset,
};

pub(crate) fn attach_workspace_actions(root: Div, cx: &mut Context<LstGpuiApp>) -> Div {
    macro_rules! bind_commands {
        ($root:ident, $($action:ty => $update:expr;)*) => {
            $(
                let $root = $root.on_action(cx.listener(
                    |this, _: &$action, _: &mut Window, cx| {
                        this.update_model(cx, true, $update);
                    },
                ));
            )*
        };
    }

    bind_commands! {
        root,
        OpenFile => |model| model.request_open_files();
        SaveFile => |model| model.request_save();
        SaveFileAs => |model| model.request_save_as();
        NextTab => |model| model.next_tab();
        PrevTab => |model| model.prev_tab();
        ToggleWrap => |model| model.toggle_wrap();
        CopySelection => |model| model.copy_selection();
        CutSelection => |model| model.cut_selection();
        PasteClipboard => |model| model.request_paste();
        MoveLeft => |model| model.move_horizontal_collapsed(true);
        MoveRight => |model| model.move_horizontal_collapsed(false);
        MoveWordLeft => |model| model.move_word(true, false);
        MoveWordRight => |model| model.move_word(false, false);
        MoveSubwordLeft => |model| model.move_subword(true, false);
        MoveSubwordRight => |model| model.move_subword(false, false);
        MoveDocumentStart => |model| model.move_document_boundary(false, false);
        MoveDocumentEnd => |model| model.move_document_boundary(true, false);
        SelectLeft => |model| model.move_horizontal_by(-1, true);
        SelectRight => |model| model.move_horizontal_by(1, true);
        SelectWordLeft => |model| model.move_word(true, true);
        SelectWordRight => |model| model.move_word(false, true);
        SelectSubwordLeft => |model| model.move_subword(true, true);
        SelectSubwordRight => |model| model.move_subword(false, true);
        SelectDocumentStart => |model| model.move_document_boundary(false, true);
        SelectDocumentEnd => |model| model.move_document_boundary(true, true);
        MoveSmartHome => |model| model.smart_home(false);
        MoveLineStart => |model| model.move_line_boundary(false, false);
        MoveLineEnd => |model| model.move_line_boundary(true, false);
        SelectSmartHome => |model| model.smart_home(true);
        SelectLineStart => |model| model.move_line_boundary(false, true);
        SelectLineEnd => |model| model.move_line_boundary(true, true);
        Backspace => |model| model.backspace();
        DeleteForward => |model| model.delete_forward();
        DeleteWordBackward => |model| model.delete_word(true);
        DeleteWordForward => |model| model.delete_word(false);
        InsertNewline => |model| model.insert_newline_at_cursor();
        InsertTab => |model| model.insert_tab_at_cursor();
        OutdentSelection => |model| model.outdent_at_cursor();
        SelectAll => |model| model.select_all();
        Undo => |model| model.undo();
        Redo => |model| model.redo();
        FindOpen => |model| model.toggle_find_panel(false);
        FindOpenReplace => |model| model.toggle_find_panel(true);
        FindNext => |model| model.find_next_match();
        FindPrev => |model| model.find_prev_match();
        ReplaceOne => |model| model.replace_current_match();
        ReplaceAll => |model| model.replace_all_matches_in_document();
        GotoLineOpen => |model| model.toggle_goto_line_panel();
        DeleteLine => |model| model.delete_line();
        MoveLineUp => |model| model.move_line_up();
        MoveLineDown => |model| model.move_line_down();
        DuplicateLine => |model| model.duplicate_line();
        ToggleComment => |model| model.toggle_comment();
    }

    let root = root.on_action(cx.listener(|this, _: &NewTab, _window, cx| {
        this.request_new_tab(cx);
    }));

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

    let root = root.on_action(cx.listener(|this, _: &CloseActiveTab, _window, cx| {
        this.request_close_active_tab(cx);
    }));

    let root = root.on_action(cx.listener(|this, _: &ZoomIn, window, cx| {
        this.zoom_in(window, cx);
    }));
    let root = root.on_action(cx.listener(|this, _: &ZoomOut, window, cx| {
        this.zoom_out(window, cx);
    }));
    let root = root.on_action(cx.listener(|this, _: &ZoomReset, window, cx| {
        this.zoom_reset(window, cx);
    }));

    root.on_action(cx.listener(|this, _: &Quit, _window, cx| {
        this.request_quit(cx);
    }))
}
