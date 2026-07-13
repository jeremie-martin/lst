use gpui::{Context, Div, InteractiveElement, KeyBinding, Keystroke, Modifiers, Window};
use lst_editor::EditorCommand as Command;

use crate::LstGpuiApp;
use std::collections::{BTreeMap, HashSet};

const EDITOR: &str = "Editor && !InlineInput";
const WS_FIND_OK: &str = "(Workspace && !InlineInput) || Find";
const WS: &str = "Workspace";
const FIND: &str = "Find";

#[derive(Clone, Copy, Debug, PartialEq, Eq, gpui::Action)]
#[action(namespace = lst_gpui, no_json)]
pub(crate) struct WorkspaceAction {
    command: WorkspaceCommand,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WorkspaceCommand {
    Model(Command),
    OpenCommandPalette,
    ToggleSettings,
    NewTab,
    OpenQuickRecent,
    ToggleRecentFiles,
    CleanupText,
    MoveVertical(isize, bool),
    ScrollLines(isize),
    Page(bool, bool),
    VisualLineBoundary { end: bool, select: bool },
    CloseActiveTab,
    ReopenClosedTab,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    Quit,
    SetSelectNextSkipPrefix,
    SelectNextOccurrenceOrSkip,
}

#[derive(Clone, Copy)]
struct WorkspaceBinding {
    keystroke: &'static str,
    context: &'static str,
    command: WorkspaceCommand,
    x11_fallback: bool,
}

const fn b(keystroke: &'static str, context: &'static str, command: WorkspaceCommand) -> WorkspaceBinding {
    WorkspaceBinding {
        keystroke,
        context,
        command,
        x11_fallback: false,
    }
}

const fn fb(keystroke: &'static str, context: &'static str, command: WorkspaceCommand) -> WorkspaceBinding {
    WorkspaceBinding {
        keystroke,
        context,
        command,
        x11_fallback: true,
    }
}

const fn model(command: Command) -> WorkspaceCommand {
    WorkspaceCommand::Model(command)
}

const BINDINGS: &[WorkspaceBinding] = &[
    b("ctrl-shift-p", WS, WorkspaceCommand::OpenCommandPalette),
    b("cmd-shift-p", WS, WorkspaceCommand::OpenCommandPalette),
    b("ctrl-,", WS, WorkspaceCommand::ToggleSettings),
    b("cmd-,", WS, WorkspaceCommand::ToggleSettings),
    b("ctrl-n", WS_FIND_OK, WorkspaceCommand::NewTab),
    b("cmd-n", WS_FIND_OK, WorkspaceCommand::NewTab),
    b("ctrl-o", WS_FIND_OK, model(Command::RequestOpenFiles)),
    b("cmd-o", WS_FIND_OK, model(Command::RequestOpenFiles)),
    b("ctrl-p", WS_FIND_OK, WorkspaceCommand::OpenQuickRecent),
    b("cmd-p", WS_FIND_OK, WorkspaceCommand::OpenQuickRecent),
    b("ctrl-r", WS_FIND_OK, WorkspaceCommand::ToggleRecentFiles),
    b("cmd-r", WS_FIND_OK, WorkspaceCommand::ToggleRecentFiles),
    b("ctrl-s", WS_FIND_OK, model(Command::RequestSave)),
    b("cmd-s", WS_FIND_OK, model(Command::RequestSave)),
    b("ctrl-shift-s", WS_FIND_OK, model(Command::RequestSaveAs)),
    b("cmd-shift-s", WS_FIND_OK, model(Command::RequestSaveAs)),
    fb("ctrl-w", WS_FIND_OK, WorkspaceCommand::CloseActiveTab),
    b("cmd-w", WS_FIND_OK, WorkspaceCommand::CloseActiveTab),
    b("ctrl-tab", WS_FIND_OK, model(Command::NextTab)),
    b("cmd-shift-]", WS_FIND_OK, model(Command::NextTab)),
    b("ctrl-shift-tab", WS_FIND_OK, model(Command::PrevTab)),
    b("cmd-shift-[", WS_FIND_OK, model(Command::PrevTab)),
    b("ctrl-shift-pageup", WS_FIND_OK, model(Command::MoveActiveTab(-1))),
    b("cmd-shift-pageup", WS_FIND_OK, model(Command::MoveActiveTab(-1))),
    b("ctrl-shift-pagedown", WS_FIND_OK, model(Command::MoveActiveTab(1))),
    b("cmd-shift-pagedown", WS_FIND_OK, model(Command::MoveActiveTab(1))),
    b("alt-z", WS_FIND_OK, model(Command::ToggleWrap)),
    b("alt-l", WS_FIND_OK, model(Command::CycleGutterMode)),
    b("ctrl-=", WS, WorkspaceCommand::ZoomIn),
    b("ctrl-+", WS, WorkspaceCommand::ZoomIn),
    b("cmd-=", WS, WorkspaceCommand::ZoomIn),
    b("cmd-+", WS, WorkspaceCommand::ZoomIn),
    b("ctrl--", WS, WorkspaceCommand::ZoomOut),
    b("cmd--", WS, WorkspaceCommand::ZoomOut),
    b("ctrl-0", WS, WorkspaceCommand::ZoomReset),
    b("cmd-0", WS, WorkspaceCommand::ZoomReset),
    b("ctrl-c", EDITOR, model(Command::CopySelection)),
    b("cmd-c", EDITOR, model(Command::CopySelection)),
    b("ctrl-x", EDITOR, model(Command::CutSelection)),
    b("cmd-x", EDITOR, model(Command::CutSelection)),
    b("ctrl-v", EDITOR, model(Command::RequestPaste)),
    b("cmd-v", EDITOR, model(Command::RequestPaste)),
    b("ctrl-z", EDITOR, model(Command::Undo)),
    b("cmd-z", EDITOR, model(Command::Undo)),
    b("ctrl-y", EDITOR, model(Command::Redo)),
    b("ctrl-shift-z", EDITOR, model(Command::Redo)),
    b("cmd-shift-z", EDITOR, model(Command::Redo)),
    b("ctrl-alt-y", EDITOR, model(Command::SwapRedoBranch)),
    b("cmd-alt-shift-z", EDITOR, model(Command::SwapRedoBranch)),
    b("ctrl-f", WS, model(Command::ToggleFindPanel(false))),
    b("cmd-f", WS, model(Command::ToggleFindPanel(false))),
    b("ctrl-h", WS, model(Command::ToggleFindPanel(true))),
    b("cmd-h", WS, model(Command::ToggleFindPanel(true))),
    b("f3", WS, model(Command::FindNext)),
    b("shift-f3", WS, model(Command::FindPrev)),
    fb("ctrl-g", WS, model(Command::ToggleGotoLinePanel)),
    b("cmd-g", WS, model(Command::ToggleGotoLinePanel)),
    b("alt-up", EDITOR, model(Command::MoveLineUp)),
    b("alt-down", EDITOR, model(Command::MoveLineDown)),
    b("ctrl-shift-k", EDITOR, model(Command::DeleteLine)),
    b("cmd-shift-k", EDITOR, model(Command::DeleteLine)),
    b("ctrl-shift-d", EDITOR, model(Command::DuplicateLine)),
    b("cmd-shift-d", EDITOR, model(Command::DuplicateLine)),
    fb("ctrl-alt-shift-up", EDITOR, model(Command::DuplicateLineAbove)),
    fb("ctrl-alt-shift-down", EDITOR, model(Command::DuplicateLine)),
    b("ctrl-/", EDITOR, model(Command::ToggleComment)),
    b("cmd-/", EDITOR, model(Command::ToggleComment)),
    fb("ctrl-shift-/", EDITOR, model(Command::ToggleBlockComment)),
    b("cmd-shift-/", EDITOR, model(Command::ToggleBlockComment)),
    fb("ctrl-?", EDITOR, model(Command::ToggleBlockComment)),
    b("cmd-?", EDITOR, model(Command::ToggleBlockComment)),
    b("left", EDITOR, model(Command::MoveHorizontalCollapsed(true))),
    b("right", EDITOR, model(Command::MoveHorizontalCollapsed(false))),
    b("ctrl-left", EDITOR, model(Command::MoveWord(true, false))),
    b("ctrl-right", EDITOR, model(Command::MoveWord(false, false))),
    b("alt-left", EDITOR, model(Command::MoveSubword(true, false))),
    b("alt-right", EDITOR, model(Command::MoveSubword(false, false))),
    b("up", EDITOR, WorkspaceCommand::MoveVertical(-1, false)),
    b("down", EDITOR, WorkspaceCommand::MoveVertical(1, false)),
    fb("pageup", EDITOR, WorkspaceCommand::Page(false, false)),
    fb("pagedown", EDITOR, WorkspaceCommand::Page(true, false)),
    b("ctrl-home", EDITOR, model(Command::MoveDocumentBoundary(false, false))),
    b("ctrl-end", EDITOR, model(Command::MoveDocumentBoundary(true, false))),
    b("cmd-up", EDITOR, model(Command::MoveDocumentBoundary(false, false))),
    b("cmd-down", EDITOR, model(Command::MoveDocumentBoundary(true, false))),
    fb("shift-left", EDITOR, model(Command::MoveHorizontal(-1, true))),
    fb("shift-right", EDITOR, model(Command::MoveHorizontal(1, true))),
    fb("ctrl-shift-left", EDITOR, model(Command::MoveWord(true, true))),
    fb("ctrl-shift-right", EDITOR, model(Command::MoveWord(false, true))),
    b("alt-shift-left", EDITOR, model(Command::SmartShrinkSelection)),
    b("alt-shift-right", EDITOR, model(Command::SmartExpandSelection)),
    b("shift-up", EDITOR, WorkspaceCommand::MoveVertical(-1, true)),
    b("shift-down", EDITOR, WorkspaceCommand::MoveVertical(1, true)),
    b("ctrl-up", EDITOR, WorkspaceCommand::ScrollLines(-1)),
    b("ctrl-down", EDITOR, WorkspaceCommand::ScrollLines(1)),
    b("shift-pageup", EDITOR, WorkspaceCommand::Page(false, true)),
    b("shift-pagedown", EDITOR, WorkspaceCommand::Page(true, true)),
    b(
        "ctrl-shift-home",
        EDITOR,
        model(Command::MoveDocumentBoundary(false, true)),
    ),
    b(
        "ctrl-shift-end",
        EDITOR,
        model(Command::MoveDocumentBoundary(true, true)),
    ),
    b(
        "cmd-shift-up",
        EDITOR,
        model(Command::MoveDocumentBoundary(false, true)),
    ),
    b(
        "cmd-shift-down",
        EDITOR,
        model(Command::MoveDocumentBoundary(true, true)),
    ),
    b(
        "home",
        EDITOR,
        WorkspaceCommand::VisualLineBoundary {
            end: false,
            select: false,
        },
    ),
    b("cmd-left", EDITOR, model(Command::MoveLineBoundary(false, false))),
    b("cmd-right", EDITOR, model(Command::MoveLineBoundary(true, false))),
    b(
        "shift-home",
        EDITOR,
        WorkspaceCommand::VisualLineBoundary {
            end: false,
            select: true,
        },
    ),
    b(
        "end",
        EDITOR,
        WorkspaceCommand::VisualLineBoundary {
            end: true,
            select: false,
        },
    ),
    b(
        "shift-end",
        EDITOR,
        WorkspaceCommand::VisualLineBoundary {
            end: true,
            select: true,
        },
    ),
    fb("cmd-shift-left", EDITOR, model(Command::MoveLineBoundary(false, true))),
    fb("cmd-shift-home", EDITOR, model(Command::MoveLineBoundary(false, true))),
    fb("cmd-shift-right", EDITOR, model(Command::MoveLineBoundary(true, true))),
    fb("cmd-shift-end", EDITOR, model(Command::MoveLineBoundary(true, true))),
    b("backspace", EDITOR, model(Command::Backspace)),
    b("delete", EDITOR, model(Command::DeleteForward)),
    b("ctrl-backspace", EDITOR, model(Command::DeleteWord(true))),
    b("alt-backspace", EDITOR, model(Command::DeleteWord(true))),
    b("ctrl-delete", EDITOR, model(Command::DeleteWord(false))),
    b("alt-delete", EDITOR, model(Command::DeleteWord(false))),
    b("enter", EDITOR, model(Command::InsertNewline)),
    b("tab", EDITOR, model(Command::InsertTab)),
    b("ctrl-]", EDITOR, model(Command::IndentLines)),
    fb("shift-tab", EDITOR, model(Command::Outdent)),
    b("ctrl-[", EDITOR, model(Command::Outdent)),
    fb("ctrl-a", EDITOR, model(Command::SelectAll)),
    b("cmd-a", EDITOR, model(Command::SelectAll)),
    fb("ctrl-d", EDITOR, WorkspaceCommand::SelectNextOccurrenceOrSkip),
    b("cmd-d", EDITOR, WorkspaceCommand::SelectNextOccurrenceOrSkip),
    b("ctrl-u", EDITOR, model(Command::PopPrimarySelectionCursor)),
    fb("ctrl-k", EDITOR, WorkspaceCommand::SetSelectNextSkipPrefix),
    fb("ctrl-shift-l", EDITOR, model(Command::SelectAllOccurrences)),
    b("cmd-shift-l", EDITOR, model(Command::SelectAllOccurrences)),
    b("ctrl-f2", EDITOR, model(Command::SelectAllOccurrences)),
    b("alt-enter", FIND, model(Command::SelectAllFindMatches)),
    b("shift-alt-i", EDITOR, model(Command::AddCursorsToSelectedLineEnds)),
    b("alt-shift-i", EDITOR, model(Command::AddCursorsToSelectedLineEnds)),
    fb("alt-shift-up", EDITOR, model(Command::AddCursorAbove)),
    fb("alt-shift-down", EDITOR, model(Command::AddCursorBelow)),
    b("ctrl-alt-up", EDITOR, model(Command::AddCursorAbove)),
    b("cmd-alt-up", EDITOR, model(Command::AddCursorAbove)),
    b("ctrl-alt-down", EDITOR, model(Command::AddCursorBelow)),
    b("cmd-alt-down", EDITOR, model(Command::AddCursorBelow)),
    b("ctrl-l", EDITOR, model(Command::SelectCurrentLine)),
    b("cmd-l", EDITOR, model(Command::SelectCurrentLine)),
    fb("ctrl-q", WS_FIND_OK, WorkspaceCommand::Quit),
    b("cmd-q", WS_FIND_OK, WorkspaceCommand::Quit),
    b("ctrl-t", EDITOR, model(Command::TransposeChars)),
    b("cmd-t", EDITOR, model(Command::TransposeChars)),
    b("insert", EDITOR, model(Command::ToggleOvertype)),
    b("ctrl-alt-k", EDITOR, model(Command::ToggleBookmark)),
    b("ctrl-alt-l", EDITOR, model(Command::JumpNextBookmark)),
    b("ctrl-alt-j", EDITOR, model(Command::JumpPreviousBookmark)),
    b("ctrl-shift-t", WS_FIND_OK, WorkspaceCommand::ReopenClosedTab),
    b("cmd-shift-t", WS_FIND_OK, WorkspaceCommand::ReopenClosedTab),
    b("alt-s", WS, model(Command::ToggleFindInSelection)),
    b("ctrl-alt-enter", FIND, model(Command::ReplaceAllMatches)),
    b("cmd-alt-enter", FIND, model(Command::ReplaceAllMatches)),
];

pub(crate) fn editor_keybindings(overrides: &BTreeMap<String, Vec<String>>) -> Vec<KeyBinding> {
    let overridden: HashSet<&str> = overrides.keys().map(String::as_str).collect();
    let mut bindings = BINDINGS
        .iter()
        .filter(|binding| !overridden.contains(command_id(binding.command)))
        .map(|binding| {
            KeyBinding::new(
                binding.keystroke,
                WorkspaceAction {
                    command: binding.command,
                },
                Some(binding.context),
            )
        })
        .collect::<Vec<_>>();
    for (id, keystrokes) in overrides {
        let Some(default) = BINDINGS.iter().find(|binding| command_id(binding.command) == id) else {
            continue;
        };
        for keystroke in keystrokes {
            // KeyBinding::new panics on malformed keystrokes; overrides come
            // from user-edited config, so invalid ones are dropped instead.
            if !valid_keystroke_sequence(keystroke) {
                continue;
            }
            bindings.push(KeyBinding::new(
                keystroke,
                WorkspaceAction {
                    command: default.command,
                },
                Some(default.context),
            ));
        }
    }
    bindings
}

fn valid_keystroke_sequence(sequence: &str) -> bool {
    let mut any = false;
    for keystroke in sequence.split_whitespace() {
        if Keystroke::parse(keystroke).is_err() {
            return false;
        }
        any = true;
    }
    any
}

pub(crate) fn command_id(command: WorkspaceCommand) -> &'static str {
    use lst_editor::EditorCommand::*;
    match command {
        WorkspaceCommand::OpenCommandPalette => "workbench.command_palette",
        WorkspaceCommand::ToggleSettings => "workbench.settings",
        WorkspaceCommand::NewTab => "file.new_scratchpad",
        WorkspaceCommand::OpenQuickRecent => "file.quick_open",
        WorkspaceCommand::ToggleRecentFiles => "file.open_recent",
        WorkspaceCommand::CleanupText => "tools.cleanup_text",
        WorkspaceCommand::MoveVertical(-1, false) => "cursor.up",
        WorkspaceCommand::MoveVertical(1, false) => "cursor.down",
        WorkspaceCommand::MoveVertical(-1, true) => "cursor.up_select",
        WorkspaceCommand::MoveVertical(1, true) => "cursor.down_select",
        WorkspaceCommand::MoveVertical(_, _) => "cursor.move_vertical",
        WorkspaceCommand::ScrollLines(-1) => "view.scroll_line_up",
        WorkspaceCommand::ScrollLines(1) => "view.scroll_line_down",
        WorkspaceCommand::ScrollLines(_) => "view.scroll_lines",
        WorkspaceCommand::Page(false, false) => "cursor.page_up",
        WorkspaceCommand::Page(true, false) => "cursor.page_down",
        WorkspaceCommand::Page(false, true) => "cursor.page_up_select",
        WorkspaceCommand::Page(true, true) => "cursor.page_down_select",
        WorkspaceCommand::VisualLineBoundary {
            end: false,
            select: false,
        } => "cursor.visual_home",
        WorkspaceCommand::VisualLineBoundary {
            end: true,
            select: false,
        } => "cursor.visual_end",
        WorkspaceCommand::VisualLineBoundary {
            end: false,
            select: true,
        } => "cursor.visual_home_select",
        WorkspaceCommand::VisualLineBoundary {
            end: true,
            select: true,
        } => "cursor.visual_end_select",
        WorkspaceCommand::CloseActiveTab => "file.close_tab",
        WorkspaceCommand::ReopenClosedTab => "file.reopen_closed_tab",
        WorkspaceCommand::ZoomIn => "view.zoom_in",
        WorkspaceCommand::ZoomOut => "view.zoom_out",
        WorkspaceCommand::ZoomReset => "view.zoom_reset",
        WorkspaceCommand::Quit => "file.quit",
        WorkspaceCommand::SetSelectNextSkipPrefix => "selection.skip_next_prefix",
        WorkspaceCommand::SelectNextOccurrenceOrSkip => "selection.add_next_occurrence",
        WorkspaceCommand::Model(command) => match command {
            RequestOpenFiles => "file.open",
            RequestSave => "file.save",
            RequestSaveAs => "file.save_as",
            NextTab => "tabs.next",
            PrevTab => "tabs.previous",
            MoveActiveTab(-1) => "tabs.move_left",
            MoveActiveTab(1) => "tabs.move_right",
            MoveActiveTab(_) => "tabs.move",
            ToggleWrap => "view.toggle_word_wrap",
            CycleGutterMode => "view.cycle_line_numbers",
            CopySelection => "edit.copy",
            CutSelection => "edit.cut",
            RequestPaste => "edit.paste",
            MoveHorizontalCollapsed(true) => "cursor.left",
            MoveHorizontalCollapsed(false) => "cursor.right",
            MoveHorizontal(-1, true) => "cursor.left_select",
            MoveHorizontal(1, true) => "cursor.right_select",
            MoveHorizontal(_, _) => "cursor.horizontal",
            MoveWord(true, false) => "cursor.word_left",
            MoveWord(false, false) => "cursor.word_right",
            MoveWord(true, true) => "cursor.word_left_select",
            MoveWord(false, true) => "cursor.word_right_select",
            MoveSubword(true, false) => "cursor.subword_left",
            MoveSubword(false, false) => "cursor.subword_right",
            MoveSubword(_, _) => "cursor.subword",
            MoveDocumentBoundary(false, false) => "cursor.document_start",
            MoveDocumentBoundary(true, false) => "cursor.document_end",
            MoveDocumentBoundary(false, true) => "cursor.document_start_select",
            MoveDocumentBoundary(true, true) => "cursor.document_end_select",
            SmartHome(false) => "cursor.smart_home",
            SmartHome(true) => "cursor.smart_home_select",
            MoveLineBoundary(false, false) => "cursor.line_start",
            MoveLineBoundary(true, false) => "cursor.line_end",
            MoveLineBoundary(false, true) => "cursor.line_start_select",
            MoveLineBoundary(true, true) => "cursor.line_end_select",
            MoveDisplayRows(_, _, _) | Page(_, _, _) => "cursor.viewport_motion",
            Backspace => "edit.backspace",
            DeleteForward => "edit.delete_forward",
            DeleteWord(true) => "edit.delete_word_left",
            DeleteWord(false) => "edit.delete_word_right",
            InsertNewline => "edit.insert_newline",
            InsertTab => "edit.indent",
            IndentLines => "edit.indent_lines",
            Outdent => "edit.outdent",
            SelectAll => "selection.select_all",
            SmartExpandSelection => "selection.expand",
            SmartShrinkSelection => "selection.shrink",
            SelectNextOccurrence => "selection.add_next_occurrence",
            SelectAllOccurrences => "selection.select_all_occurrences",
            SelectAllFindMatches => "selection.select_all_find_matches",
            SkipNextOccurrence => "selection.skip_next_occurrence",
            PopPrimarySelectionCursor => "selection.undo_cursor",
            AddCursorAbove => "selection.add_cursor_above",
            AddCursorBelow => "selection.add_cursor_below",
            AddCursorsToSelectedLineEnds => "selection.add_cursors_line_ends",
            SelectCurrentLine => "selection.select_line",
            SelectCurrentParagraph => "selection.select_paragraph",
            Undo => "edit.undo",
            Redo => "edit.redo",
            SwapRedoBranch => "edit.swap_redo_branch",
            ToggleFindPanel(false) => "find.open",
            ToggleFindPanel(true) => "find.replace",
            FindNext => "find.next",
            FindPrev => "find.previous",
            ReplaceCurrentMatch => "find.replace_one",
            ReplaceAllMatches => "find.replace_all",
            ToggleFindCaseSensitive => "find.toggle_case_sensitive",
            ToggleFindWholeWord => "find.toggle_whole_word",
            ToggleFindRegex => "find.toggle_regex",
            ToggleFindInSelection => "find.toggle_in_selection",
            ToggleGotoLinePanel => "navigation.goto_line",
            SubmitGotoLine => "navigation.submit_goto_line",
            DeleteLine => "edit.delete_line",
            MoveLineUp => "edit.move_line_up",
            MoveLineDown => "edit.move_line_down",
            DuplicateLineAbove => "edit.duplicate_line_above",
            DuplicateLine => "edit.duplicate_line",
            ToggleComment => "edit.toggle_line_comment",
            ToggleBlockComment => "edit.toggle_block_comment",
            TransposeChars => "edit.transpose_characters",
            ToggleOvertype => "edit.toggle_overtype",
            ToggleBookmark => "navigation.toggle_bookmark",
            JumpNextBookmark => "navigation.next_bookmark",
            JumpPreviousBookmark => "navigation.previous_bookmark",
        },
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CommandSpec {
    pub(crate) id: &'static str,
    pub(crate) title: String,
    pub(crate) category: &'static str,
    pub(crate) command: WorkspaceCommand,
    pub(crate) shortcuts: Vec<String>,
}

pub(crate) fn command_specs(overrides: &BTreeMap<String, Vec<String>>) -> Vec<CommandSpec> {
    let mut specs: Vec<CommandSpec> = Vec::new();
    let mut indices = std::collections::HashMap::<&'static str, usize>::new();
    for binding in BINDINGS {
        let id = command_id(binding.command);
        if let Some(index) = indices.get(id).copied() {
            if !overrides.contains_key(id) {
                let shortcut = binding.keystroke.to_string();
                if !specs[index].shortcuts.contains(&shortcut) {
                    specs[index].shortcuts.push(shortcut);
                }
            }
            continue;
        }
        let shortcuts = overrides
            .get(id)
            .cloned()
            .unwrap_or_else(|| vec![binding.keystroke.to_string()]);
        indices.insert(id, specs.len());
        specs.push(CommandSpec {
            id,
            title: command_title(id),
            category: command_category(id),
            command: binding.command,
            shortcuts,
        });
    }
    // Commands intentionally discoverable only through the palette live here
    // rather than carrying a hidden or misleading default shortcut.
    let cleanup_id = command_id(WorkspaceCommand::CleanupText);
    if !indices.contains_key(cleanup_id) {
        specs.push(CommandSpec {
            id: cleanup_id,
            title: command_title(cleanup_id),
            category: command_category(cleanup_id),
            command: WorkspaceCommand::CleanupText,
            shortcuts: overrides.get(cleanup_id).cloned().unwrap_or_default(),
        });
    }
    specs.sort_by(|left, right| {
        left.category
            .cmp(right.category)
            .then_with(|| left.title.cmp(&right.title))
    });
    specs
}

fn command_category(id: &str) -> &'static str {
    match id.split('.').next().unwrap_or_default() {
        "file" | "tabs" => "File",
        "edit" => "Edit",
        "selection" | "cursor" => "Selection",
        "find" | "navigation" => "Navigate",
        "view" => "View",
        "tools" => "Tools",
        "workbench" => "Preferences",
        _ => "Other",
    }
}

fn command_title(id: &str) -> String {
    let explicit = match id {
        "workbench.command_palette" => Some("Show Command Palette"),
        "workbench.settings" => Some("Open Settings"),
        "file.new_scratchpad" => Some("New Scratchpad"),
        "file.quick_open" => Some("Quick Open"),
        "file.open_recent" => Some("Open Recent"),
        "file.save_as" => Some("Save As"),
        "file.close_tab" => Some("Close Tab"),
        "file.reopen_closed_tab" => Some("Reopen Closed Tab"),
        "find.open" => Some("Find"),
        "find.replace" => Some("Replace"),
        "find.replace_one" => Some("Replace Current Match"),
        "find.replace_all" => Some("Replace All Matches"),
        "navigation.goto_line" => Some("Go to Line or Column"),
        "view.toggle_word_wrap" => Some("Toggle Word Wrap"),
        "view.cycle_line_numbers" => Some("Cycle Line Number Mode"),
        "tools.cleanup_text" => Some("Clean Up Text with AI"),
        "selection.add_next_occurrence" => Some("Add Selection to Next Match"),
        "selection.select_all_occurrences" => Some("Select All Occurrences"),
        "selection.add_cursors_line_ends" => Some("Add Cursors to Line Ends"),
        "edit.toggle_line_comment" => Some("Toggle Line Comment"),
        "edit.toggle_block_comment" => Some("Toggle Block Comment"),
        "edit.indent_lines" => Some("Indent Lines"),
        _ => None,
    };
    if let Some(title) = explicit {
        return title.to_string();
    }
    id.rsplit('.')
        .next()
        .unwrap_or(id)
        .split('_')
        .map(|word| {
            let mut chars = word.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn workspace_fallback_command(key: &str, modifiers: Modifiers) -> Option<WorkspaceCommand> {
    let keystroke = fallback_keystroke(key, modifiers)?;
    BINDINGS
        .iter()
        .find(|binding| binding.x11_fallback && binding.keystroke == keystroke)
        .map(|binding| binding.command)
}

pub(crate) fn attach_workspace_actions(root: Div, cx: &mut Context<LstGpuiApp>) -> Div {
    root.on_action(cx.listener(|this, action: &WorkspaceAction, window: &mut Window, cx| {
        this.dispatch_workspace_command(action.command, window, cx);
        cx.stop_propagation();
    }))
}

impl LstGpuiApp {
    pub(crate) fn dispatch_workspace_command(
        &mut self,
        command: WorkspaceCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.quit_review.is_some() || self.cleanup_confirmation.is_some() {
            return;
        }
        if self.close_prompt.is_some() {
            if command == WorkspaceCommand::Model(Command::InsertNewline) {
                self.confirm_close_prompt_save(cx);
            }
            return;
        }
        if self.recent.is_open() {
            if command == WorkspaceCommand::ToggleRecentFiles {
                self.toggle_recent_files_panel(window, cx);
            }
            return;
        }
        if self.workspace_surface != crate::WorkspaceSurface::None {
            match (self.workspace_surface, command) {
                (crate::WorkspaceSurface::Settings, WorkspaceCommand::ToggleSettings) => {
                    self.toggle_settings(window, cx);
                }
                (crate::WorkspaceSurface::CommandPalette, WorkspaceCommand::OpenCommandPalette) => {
                    self.close_workspace_surface(cx);
                }
                _ => {}
            }
            return;
        }
        let command = match command {
            WorkspaceCommand::SetSelectNextSkipPrefix => {
                self.clear_x11_modifier_chord_state();
                self.x11_ctrl_k_pending = true;
                cx.notify();
                return;
            }
            WorkspaceCommand::SelectNextOccurrenceOrSkip => {
                let skip = self.x11_ctrl_k_pending;
                self.clear_x11_modifier_chord_state();
                WorkspaceCommand::Model(if skip {
                    Command::SkipNextOccurrence
                } else {
                    Command::SelectNextOccurrence
                })
            }
            command => {
                self.clear_x11_modifier_chord_state();
                command
            }
        };

        match command {
            WorkspaceCommand::OpenCommandPalette => {
                self.open_command_palette(window, cx);
            }
            WorkspaceCommand::ToggleSettings => {
                self.toggle_settings(window, cx);
            }
            WorkspaceCommand::Model(command) => {
                self.execute_model_command(cx, command);
            }
            WorkspaceCommand::NewTab => {
                self.request_new_tab(cx);
            }
            WorkspaceCommand::OpenQuickRecent => {
                self.open_recent_quick_picker(window, cx);
            }
            WorkspaceCommand::ToggleRecentFiles => {
                self.toggle_recent_files_panel(window, cx);
            }
            WorkspaceCommand::CleanupText => {
                self.start_cleanup(cx);
            }
            WorkspaceCommand::MoveVertical(delta, select) => {
                self.move_vertical(delta, select, window, cx);
            }
            WorkspaceCommand::ScrollLines(delta) => {
                self.scroll_editor_lines(delta, cx);
            }
            WorkspaceCommand::Page(down, select) => {
                self.move_page(down, select, window, cx);
            }
            WorkspaceCommand::VisualLineBoundary { end, select } => {
                self.move_visual_line_boundary(end, select, cx);
            }
            WorkspaceCommand::CloseActiveTab => {
                self.request_close_active_tab(cx);
            }
            WorkspaceCommand::ReopenClosedTab => {
                self.reopen_recently_closed_tab(cx);
            }
            WorkspaceCommand::ZoomIn => {
                self.zoom_in(window, cx);
            }
            WorkspaceCommand::ZoomOut => {
                self.zoom_out(window, cx);
            }
            WorkspaceCommand::ZoomReset => {
                self.zoom_reset(window, cx);
            }
            WorkspaceCommand::Quit => {
                self.request_quit(cx);
            }
            WorkspaceCommand::SetSelectNextSkipPrefix | WorkspaceCommand::SelectNextOccurrenceOrSkip => unreachable!(),
        }
    }
}

fn fallback_keystroke(key: &str, modifiers: Modifiers) -> Option<String> {
    let key = key.to_ascii_lowercase();
    if key.is_empty() {
        return None;
    }

    let mut keystroke = String::new();
    if modifiers.control {
        keystroke.push_str("ctrl-");
    }
    if modifiers.platform {
        keystroke.push_str("cmd-");
    }
    if modifiers.alt {
        keystroke.push_str("alt-");
    }
    if modifiers.shift {
        keystroke.push_str("shift-");
    }
    keystroke.push_str(&key);
    Some(keystroke)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_insertion_and_line_indent_have_independent_configurable_commands() {
        let mut overrides = BTreeMap::new();
        overrides.insert("edit.indent".to_string(), vec!["alt-t".to_string()]);
        overrides.insert("edit.indent_lines".to_string(), vec!["alt-i".to_string()]);

        let specs = command_specs(&overrides);
        let tab = specs
            .iter()
            .find(|spec| spec.id == "edit.indent")
            .expect("Tab insertion command should remain discoverable");
        let lines = specs
            .iter()
            .find(|spec| spec.id == "edit.indent_lines")
            .expect("Explicit line-indent command should remain discoverable");

        assert_eq!(tab.command, WorkspaceCommand::Model(Command::InsertTab));
        assert_eq!(tab.shortcuts, ["alt-t"]);
        assert_eq!(lines.command, WorkspaceCommand::Model(Command::IndentLines));
        assert_eq!(lines.title, "Indent Lines");
        assert_eq!(lines.shortcuts, ["alt-i"]);
    }
}
