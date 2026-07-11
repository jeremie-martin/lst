#![allow(dead_code)]

use std::path::PathBuf;

use lst_editor::{
    vim::{self, Key, NamedKey},
    EditorCommand, EditorEffect, EditorModel, EditorTab, FocusTarget, InputMode, Position, Selection, TabId,
};

const WRAP_COLUMNS: usize = 80;

pub type CursorCase<'a> = (&'a str, &'a str, (usize, usize), &'a str, (usize, usize));
pub type TextCase<'a> = (&'a str, &'a str, (usize, usize), &'a str, &'a str);

pub struct VimHarness {
    pub model: EditorModel,
    pub focus: FocusTarget,
    effects: Vec<EditorEffect>,
    deferred_find_query: Option<String>,
}

pub struct ModelHarness {
    pub model: EditorModel,
    clipboard: Option<String>,
    primary: Option<String>,
}

impl ModelHarness {
    pub fn new(text: &str) -> Self {
        let tab = EditorTab::from_path_with_stamp(TabId::from_raw(1), PathBuf::from("model-spec.rs"), text, None);
        let mut harness = Self {
            model: EditorModel::from_tabs(tab, Vec::new(), "Ready.".to_string()),
            clipboard: None,
            primary: None,
        };
        harness.sync_effects();
        harness
    }

    pub fn with_two_tabs(first: &str, second: &str) -> Self {
        let first = EditorTab::from_path_with_stamp(TabId::from_raw(1), PathBuf::from("first.rs"), first, None);
        let second = EditorTab::from_path_with_stamp(TabId::from_raw(2), PathBuf::from("second.rs"), second, None);
        let mut harness = Self {
            model: EditorModel::from_tabs(first, vec![second], "Ready.".to_string()),
            clipboard: None,
            primary: None,
        };
        harness.sync_effects();
        harness
    }

    pub fn execute(&mut self, command: EditorCommand) {
        self.model.execute(command);
        self.sync_effects();
    }

    pub fn paste_text(&mut self, text: &str) {
        self.model.paste_text(text.to_string());
        self.sync_effects();
    }

    pub fn set_clipboard(&mut self, text: impl Into<String>) {
        self.clipboard = Some(text.into());
    }

    pub fn clear_transfer_buffers(&mut self) {
        self.clipboard = None;
        self.primary = None;
    }

    pub fn set_cursor(&mut self, position: Position) {
        self.model.set_active_cursor_position(position.line, position.column);
        self.sync_effects();
    }

    pub fn select_first_lines(&mut self, requested_lines: usize) {
        let end = {
            let tab = self.model.active_tab();
            let selected_lines = requested_lines.min(tab.line_count());
            if selected_lines >= tab.line_count() {
                tab.len_chars()
            } else {
                tab.buffer().line_to_char(selected_lines)
            }
        };
        self.model.set_selection(Selection::from_range(0..end, false));
        self.sync_effects();
    }

    pub fn configure_viewport(&mut self, rows: usize, top: usize) {
        self.model.set_viewport_rows(rows);
        self.model.set_viewport_top(top);
    }

    pub fn sync_effects(&mut self) {
        loop {
            let effects = self.model.drain_effects();
            if effects.is_empty() {
                break;
            }
            for effect in effects {
                match effect {
                    EditorEffect::WriteClipboard(text) => self.clipboard = Some(text),
                    EditorEffect::WritePrimary(text) => self.primary = Some(text),
                    EditorEffect::ReadClipboard => {
                        if let Some(text) = self.clipboard.clone() {
                            self.model.paste_text(text);
                        } else {
                            self.model.clipboard_unavailable();
                        }
                    }
                    EditorEffect::Focus(_)
                    | EditorEffect::Reveal(_)
                    | EditorEffect::OpenFiles
                    | EditorEffect::SaveFile { .. }
                    | EditorEffect::SaveFileAs { .. }
                    | EditorEffect::AutosaveFile { .. } => {}
                }
            }
        }
    }

    pub fn text(&self) -> String {
        self.model.active_tab().buffer_text()
    }

    pub fn tab_text(&self, index: usize) -> String {
        self.model.tab(index).expect("tab exists").buffer_text()
    }

    pub fn cursor(&self) -> Position {
        self.model.active_tab().cursor_position()
    }

    pub fn selection_count(&self) -> usize {
        self.model.selection_set().as_slice().len()
    }

    pub fn find_match_count(&self) -> usize {
        self.model.find().matches.len()
    }

    pub fn clipboard_text(&self) -> Option<&str> {
        self.clipboard.as_deref()
    }

    pub fn primary_text(&self) -> Option<&str> {
        self.primary.as_deref()
    }
}

impl VimHarness {
    pub fn new(text: &str) -> Self {
        let tab = EditorTab::from_path_with_stamp(TabId::from_raw(1), PathBuf::from("vim-spec.md"), text, None);
        let mut model = EditorModel::from_tabs(tab, Vec::new(), "Ready.".to_string());
        model.set_input_mode(InputMode::Vim);
        let mut harness = Self {
            model,
            focus: FocusTarget::Editor,
            effects: Vec::new(),
            deferred_find_query: None,
        };
        harness.sync_effects();
        harness
    }

    pub fn normal_at(text: &str, line: usize, column: usize) -> Self {
        let mut harness = Self::new(text);
        harness.keys("<esc>");
        harness.model.set_active_cursor_position(line, column);
        harness.sync_effects();
        harness.clear_effects();
        harness
    }

    pub fn with_two_tabs(first: &str, second: &str) -> Self {
        let first = EditorTab::from_path_with_stamp(TabId::from_raw(1), PathBuf::from("first.md"), first, None);
        let second = EditorTab::from_path_with_stamp(TabId::from_raw(2), PathBuf::from("second.md"), second, None);
        let mut model = EditorModel::from_tabs(first, vec![second], "Ready.".to_string());
        model.set_input_mode(InputMode::Vim);
        let mut harness = Self {
            model,
            focus: FocusTarget::Editor,
            effects: Vec::new(),
            deferred_find_query: None,
        };
        harness.sync_effects();
        harness
    }

    pub fn keys(&mut self, sequence: &str) {
        for (key, modifiers) in parse_keys(sequence) {
            self.key(key, modifiers);
        }
    }

    pub fn key(&mut self, key: Key, modifiers: vim::Modifiers) {
        match self.focus {
            FocusTarget::Editor => self.editor_key(key, modifiers),
            FocusTarget::FindQuery => self.find_query_key(key),
            FocusTarget::FindReplace | FocusTarget::GotoLine => {}
        }
        self.sync_effects();
    }

    fn editor_key(&mut self, key: Key, modifiers: vim::Modifiers) {
        if key == Key::Named(NamedKey::Enter) && self.model.vim_mode() == vim::Mode::Insert {
            self.model.execute(EditorCommand::InsertNewline);
            return;
        }
        if key == Key::Named(NamedKey::Tab) && self.model.vim_mode() == vim::Mode::Insert {
            self.model.execute(EditorCommand::InsertTab);
            return;
        }
        if key == Key::Named(NamedKey::Backspace) && self.model.vim_mode() == vim::Mode::Insert {
            self.model.execute(EditorCommand::Backspace);
            return;
        }
        if key == Key::Named(NamedKey::Delete) && self.model.vim_mode() == vim::Mode::Insert {
            self.model.execute(EditorCommand::DeleteForward);
            return;
        }
        if key == Key::Named(NamedKey::ArrowLeft) && self.model.vim_mode() == vim::Mode::Insert {
            self.model.execute(EditorCommand::MoveHorizontal(-1, false));
            return;
        }
        if key == Key::Named(NamedKey::ArrowRight) && self.model.vim_mode() == vim::Mode::Insert {
            self.model.execute(EditorCommand::MoveHorizontal(1, false));
            return;
        }

        if key == Key::Character("\u{1b}".to_string()) {
            self.model.handle_vim_escape();
            return;
        }

        if self.model.vim_mode() == vim::Mode::Insert && !modifiers.command && !modifiers.control {
            if let Key::Character(text) = key {
                self.model.replace_text_from_input(None, text);
                return;
            }
        }

        self.model.handle_vim_key(key, modifiers, WRAP_COLUMNS);
    }

    fn find_query_key(&mut self, key: Key) {
        match key {
            Key::Character(text) if text == "\u{1b}" => {
                self.deferred_find_query = None;
                self.model.close_find_panel();
            }
            Key::Character(text) => {
                self.deferred_find_query
                    .get_or_insert_with(|| self.model.find().query.clone())
                    .push_str(&text);
            }
            Key::Named(NamedKey::Backspace) => {
                let mut query = self
                    .deferred_find_query
                    .take()
                    .unwrap_or_else(|| self.model.find().query.clone());
                query.pop();
                self.deferred_find_query = Some(query);
            }
            Key::Named(NamedKey::Enter) => {
                let query = self
                    .deferred_find_query
                    .take()
                    .unwrap_or_else(|| self.model.find().query.clone());
                let was_visual = matches!(self.model.vim_mode(), vim::Mode::Visual | vim::Mode::VisualLine);
                self.model.update_find_query_and_activate(query);
                if was_visual {
                    self.model.close_find_panel();
                } else {
                    self.model.submit_find_query();
                }
            }
            _ => {}
        }
    }

    pub fn sync_effects(&mut self) {
        for effect in self.model.drain_effects() {
            if let EditorEffect::Focus(target) = effect {
                self.focus = target;
                if target == FocusTarget::FindQuery {
                    self.deferred_find_query = Some(self.model.find().query.clone());
                }
            }
            self.effects.push(effect);
        }
    }

    pub fn clear_effects(&mut self) {
        self.model.drain_effects();
        self.effects.clear();
    }

    pub fn take_effects(&mut self) -> Vec<EditorEffect> {
        self.sync_effects();
        std::mem::take(&mut self.effects)
    }

    pub fn text(&self) -> String {
        self.model.active_tab().buffer_text()
    }

    pub fn cursor(&self) -> Position {
        self.model.active_tab().cursor_position()
    }

    pub fn selected_text(&self) -> Option<String> {
        self.model.active_tab().selected_text()
    }

    #[track_caller]
    pub fn expect_text(&self, expected: &str) {
        assert_eq!(self.text(), expected);
    }

    #[track_caller]
    pub fn expect_cursor(&self, line: usize, column: usize) {
        assert_eq!(self.cursor(), Position { line, column });
    }

    #[track_caller]
    pub fn expect_mode(&self, expected: vim::Mode) {
        assert_eq!(self.model.vim_mode(), expected);
    }

    #[track_caller]
    pub fn expect_selection(&self, expected: &str) {
        assert_eq!(self.selected_text().as_deref(), Some(expected));
    }

    #[track_caller]
    pub fn expect_no_selection(&self) {
        assert_eq!(self.selected_text(), None);
    }

    #[track_caller]
    pub fn expect_char_register(&self, expected: &str) {
        assert_eq!(self.model.vim_register(), &vim::Register::Char(expected.to_string()));
    }

    #[track_caller]
    pub fn expect_line_register(&self, expected: &str) {
        assert_eq!(self.model.vim_register(), &vim::Register::Line(expected.to_string()));
    }

    #[track_caller]
    pub fn expect_visual_state(&self, anchor: (usize, usize), head: (usize, usize)) {
        let state = self.model.vim_visual_state().expect("visual state");
        assert_eq!(
            state.anchor,
            Position {
                line: anchor.0,
                column: anchor.1
            }
        );
        assert_eq!(
            state.head,
            Position {
                line: head.0,
                column: head.1
            }
        );
    }

    #[track_caller]
    pub fn expect_pending(&self, expected: &str) {
        assert_eq!(self.model.vim_pending_display(), expected);
    }
}

pub fn run_cursor_cases(cases: &[CursorCase<'_>]) {
    for &(name, text, start, keys, expected) in cases {
        let mut harness = VimHarness::normal_at(text, start.0, start.1);
        harness.keys(keys);
        assert_eq!(
            harness.cursor(),
            Position {
                line: expected.0,
                column: expected.1
            },
            "{}",
            name
        );
    }
}

pub fn run_text_cases(cases: &[TextCase<'_>]) {
    for &(name, text, start, keys, expected) in cases {
        let mut harness = VimHarness::normal_at(text, start.0, start.1);
        harness.keys(keys);
        assert_eq!(harness.text(), expected, "{}", name);
    }
}

pub fn run_text_cases_expect_normal(cases: &[TextCase<'_>]) {
    for &(name, text, start, keys, expected) in cases {
        let mut harness = VimHarness::normal_at(text, start.0, start.1);
        harness.keys(keys);
        assert_eq!(harness.text(), expected, "{}", name);
        harness.expect_mode(vim::Mode::Normal);
    }
}

pub fn position_of(text: &str, needle: &str) -> Position {
    let byte = text.find(needle).expect("needle exists in text");
    let mut line = 0usize;
    let mut column = 0usize;
    for ch in text[..byte].chars() {
        if ch == '\n' {
            line += 1;
            column = 0;
        } else {
            column += 1;
        }
    }
    Position { line, column }
}

pub fn parse_keys(sequence: &str) -> Vec<(Key, vim::Modifiers)> {
    let mut out = Vec::new();
    let chars: Vec<char> = sequence.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '<' {
            if let Some(end) = chars[index + 1..].iter().position(|ch| *ch == '>') {
                let token: String = chars[index + 1..index + 1 + end].iter().collect();
                out.push(parse_token(&token));
                index += end + 2;
                continue;
            }
        }
        out.push((Key::Character(chars[index].to_string()), vim::Modifiers::default()));
        index += 1;
    }
    out
}

fn parse_token(token: &str) -> (Key, vim::Modifiers) {
    let mut rest = token.to_ascii_lowercase();
    let mut modifiers = vim::Modifiers::default();
    loop {
        if let Some(stripped) = rest.strip_prefix("c-").or_else(|| rest.strip_prefix("ctrl-")) {
            modifiers.control = true;
            rest = stripped.to_string();
        } else if let Some(stripped) = rest
            .strip_prefix("cmd-")
            .or_else(|| rest.strip_prefix("command-"))
            .or_else(|| rest.strip_prefix("super-"))
        {
            modifiers.command = true;
            rest = stripped.to_string();
        } else {
            break;
        }
    }

    let key = match rest.as_str() {
        "esc" | "escape" => Key::Character("\u{1b}".to_string()),
        "enter" | "return" => Key::Named(NamedKey::Enter),
        "tab" => Key::Named(NamedKey::Tab),
        "bs" | "backspace" => Key::Named(NamedKey::Backspace),
        "del" | "delete" => Key::Named(NamedKey::Delete),
        "left" => Key::Named(NamedKey::ArrowLeft),
        "right" => Key::Named(NamedKey::ArrowRight),
        "up" => Key::Named(NamedKey::ArrowUp),
        "down" => Key::Named(NamedKey::ArrowDown),
        "home" => Key::Named(NamedKey::Home),
        "end" => Key::Named(NamedKey::End),
        "pageup" => Key::Named(NamedKey::PageUp),
        "pagedown" => Key::Named(NamedKey::PageDown),
        "space" => Key::Character(" ".to_string()),
        "lt" => Key::Character("<".to_string()),
        _ => Key::Character(rest),
    };

    (key, modifiers)
}
