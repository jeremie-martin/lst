use gpui::{Context, Div, InteractiveElement, Window};

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
        ($root:ident, $($action:ty => $update:expr;)*) => {
            $(
                let $root = $root.on_action(cx.listener(
                    |this, _: &$action, _: &mut Window, cx| {
                        this.clear_x11_modifier_chord_state();
                        this.update_model(cx, true, $update);
                        cx.stop_propagation();
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
        MoveTabLeft => |model| model.move_active_tab(-1);
        MoveTabRight => |model| model.move_active_tab(1);
        ToggleWrap => |model| model.toggle_wrap();
        ToggleLineNumberMode => |model| model.cycle_gutter_mode();
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
        SelectSubwordLeft => |model| model.smart_shrink_selection();
        SelectSubwordRight => |model| model.smart_expand_selection();
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
        SelectAllOccurrences => |model| model.select_all_occurrences();
        SelectFindMatches => |model| model.select_all_find_matches();
        SkipNextOccurrence => |model| model.skip_next_occurrence();
        PopSelectionCursor => |model| model.pop_primary_selection_cursor();
        AddCursorAbove => |model| model.add_cursor_above();
        AddCursorBelow => |model| model.add_cursor_below();
        AddCursorsToLineEnds => |model| model.add_cursors_to_selected_line_ends();
        SelectLine => |model| model.select_current_line();
        SelectParagraph => |model| model.select_current_paragraph();
        Undo => |model| model.undo();
        Redo => |model| model.redo();
        SwapRedoBranch => |model| model.swap_redo_branch();
        FindOpen => |model| model.toggle_find_panel(false);
        FindOpenReplace => |model| model.toggle_find_panel(true);
        FindNext => |model| model.find_next_match();
        FindPrev => |model| model.find_prev_match();
        ReplaceOne => |model| model.replace_current_match();
        ReplaceAll => |model| model.replace_all_matches_in_document();
        ToggleFindCase => |model| model.toggle_find_case_sensitive();
        ToggleFindWholeWord => |model| model.toggle_find_whole_word();
        ToggleFindRegex => |model| model.toggle_find_regex();
        ToggleFindInSelection => |model| model.toggle_find_in_selection();
        GotoLineOpen => |model| model.toggle_goto_line_panel();
        DeleteLine => |model| model.delete_line();
        MoveLineUp => |model| model.move_line_up();
        MoveLineDown => |model| model.move_line_down();
        DuplicateLine => |model| model.duplicate_line();
        ToggleComment => |model| model.toggle_comment();
        ToggleBlockComment => |model| model.toggle_block_comment();
        TransposeChars => |model| model.transpose_chars();
        ToggleOvertype => |model| model.toggle_overtype();
        ToggleBookmark => |model| model.toggle_bookmark();
        NextBookmark => |model| model.jump_next_bookmark();
        PreviousBookmark => |model| model.jump_previous_bookmark();
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
            if skip {
                model.skip_next_occurrence();
            } else {
                model.select_next_occurrence();
            }
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
