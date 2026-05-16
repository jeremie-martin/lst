use gpui::{Context, Div, InteractiveElement, KeyBinding, Modifiers, Window};
use lst_editor::EditorCommand as Command;

use crate::LstGpuiApp;

const EDITOR: &str = "Editor && !InlineInput";
const WS_NO_INPUT: &str = "Workspace && !InlineInput";
const WS: &str = "Workspace";

#[derive(Clone, Copy, Debug, PartialEq, Eq, gpui::Action)]
#[action(namespace = lst_gpui, no_json)]
pub(crate) struct WorkspaceAction {
    command: WorkspaceCommand,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WorkspaceCommand {
    Model(Command),
    NewTab,
    ToggleRecentFiles,
    CleanupText,
    MoveVertical(isize, bool),
    Page(bool, bool),
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
    b("ctrl-n", WS_NO_INPUT, WorkspaceCommand::NewTab),
    b("cmd-n", WS_NO_INPUT, WorkspaceCommand::NewTab),
    b("ctrl-o", WS_NO_INPUT, model(Command::RequestOpenFiles)),
    b("cmd-o", WS_NO_INPUT, model(Command::RequestOpenFiles)),
    b("ctrl-r", WS_NO_INPUT, WorkspaceCommand::ToggleRecentFiles),
    b("cmd-r", WS_NO_INPUT, WorkspaceCommand::ToggleRecentFiles),
    b("ctrl-s", WS_NO_INPUT, model(Command::RequestSave)),
    b("cmd-s", WS_NO_INPUT, model(Command::RequestSave)),
    b("ctrl-shift-s", WS_NO_INPUT, model(Command::RequestSaveAs)),
    b("cmd-shift-s", WS_NO_INPUT, model(Command::RequestSaveAs)),
    b("ctrl-w", WS_NO_INPUT, WorkspaceCommand::CloseActiveTab),
    b("cmd-w", WS_NO_INPUT, WorkspaceCommand::CloseActiveTab),
    b("ctrl-tab", WS_NO_INPUT, model(Command::NextTab)),
    b("cmd-shift-]", WS_NO_INPUT, model(Command::NextTab)),
    b("ctrl-shift-tab", WS_NO_INPUT, model(Command::PrevTab)),
    b("cmd-shift-[", WS_NO_INPUT, model(Command::PrevTab)),
    b("ctrl-shift-pageup", WS_NO_INPUT, model(Command::MoveActiveTab(-1))),
    b("cmd-shift-pageup", WS_NO_INPUT, model(Command::MoveActiveTab(-1))),
    b("ctrl-shift-pagedown", WS_NO_INPUT, model(Command::MoveActiveTab(1))),
    b("cmd-shift-pagedown", WS_NO_INPUT, model(Command::MoveActiveTab(1))),
    b("alt-z", WS_NO_INPUT, model(Command::ToggleWrap)),
    b("alt-l", WS_NO_INPUT, model(Command::CycleGutterMode)),
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
    fb("ctrl-alt-shift-up", EDITOR, model(Command::DuplicateLine)),
    fb("ctrl-alt-shift-down", EDITOR, model(Command::DuplicateLine)),
    b("ctrl-/", EDITOR, model(Command::ToggleComment)),
    b("cmd-/", EDITOR, model(Command::ToggleComment)),
    b("ctrl-shift-/", EDITOR, model(Command::ToggleBlockComment)),
    b("cmd-shift-/", EDITOR, model(Command::ToggleBlockComment)),
    b("ctrl-?", EDITOR, model(Command::ToggleBlockComment)),
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
    b("ctrl-up", EDITOR, WorkspaceCommand::MoveVertical(-1, true)),
    b("ctrl-down", EDITOR, WorkspaceCommand::MoveVertical(1, true)),
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
    b("home", EDITOR, model(Command::SmartHome(false))),
    b("end", EDITOR, model(Command::MoveLineBoundary(true, false))),
    b("cmd-left", EDITOR, model(Command::MoveLineBoundary(false, false))),
    b("cmd-right", EDITOR, model(Command::MoveLineBoundary(true, false))),
    b("shift-home", EDITOR, model(Command::SmartHome(true))),
    b("shift-end", EDITOR, model(Command::MoveLineBoundary(true, true))),
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
    b("ctrl-]", EDITOR, model(Command::InsertTab)),
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
    b("alt-enter", WS, model(Command::SelectAllFindMatches)),
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
    b("ctrl-shift-p", EDITOR, model(Command::SelectCurrentParagraph)),
    b("cmd-shift-p", EDITOR, model(Command::SelectCurrentParagraph)),
    b("ctrl-q", WS_NO_INPUT, WorkspaceCommand::Quit),
    b("cmd-q", WS_NO_INPUT, WorkspaceCommand::Quit),
    b("ctrl-shift-r", EDITOR, WorkspaceCommand::CleanupText),
    b("cmd-shift-r", EDITOR, WorkspaceCommand::CleanupText),
    b("ctrl-t", EDITOR, model(Command::TransposeChars)),
    b("cmd-t", EDITOR, model(Command::TransposeChars)),
    b("insert", EDITOR, model(Command::ToggleOvertype)),
    b("ctrl-alt-k", EDITOR, model(Command::ToggleBookmark)),
    b("ctrl-alt-l", EDITOR, model(Command::JumpNextBookmark)),
    b("ctrl-alt-j", EDITOR, model(Command::JumpPreviousBookmark)),
    b("ctrl-shift-t", WS_NO_INPUT, WorkspaceCommand::ReopenClosedTab),
    b("cmd-shift-t", WS_NO_INPUT, WorkspaceCommand::ReopenClosedTab),
    b("alt-s", WS, model(Command::ToggleFindInSelection)),
    b("ctrl-alt-enter", WS, model(Command::ReplaceAllMatches)),
    b("cmd-alt-enter", WS, model(Command::ReplaceAllMatches)),
];

pub(crate) fn editor_keybindings() -> Vec<KeyBinding> {
    BINDINGS
        .iter()
        .map(|binding| {
            KeyBinding::new(
                binding.keystroke,
                WorkspaceAction {
                    command: binding.command,
                },
                Some(binding.context),
            )
        })
        .collect()
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
            WorkspaceCommand::Model(command) => {
                self.execute_model_command(cx, command);
            }
            WorkspaceCommand::NewTab => {
                self.request_new_tab(cx);
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
            WorkspaceCommand::Page(down, select) => {
                self.move_page(down, select, window, cx);
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
