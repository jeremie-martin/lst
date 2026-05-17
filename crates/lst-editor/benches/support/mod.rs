use std::path::PathBuf;

use lst_editor::{EditorCommand, EditorEffect, EditorModel, EditorTab, Position, Selection, TabId};

pub const WRAP_COLUMNS: usize = 80;
pub const VIEWPORT_ROWS: usize = 40;

#[derive(Clone, Copy, Debug)]
pub enum DocumentSize {
    Small,
    Medium,
    Large,
}

impl DocumentSize {
    pub fn all() -> [Self; 3] {
        [Self::Small, Self::Medium, Self::Large]
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Medium => "medium",
            Self::Large => "large",
        }
    }

    pub fn cursor_lines(self) -> usize {
        match self {
            Self::Small => 24,
            Self::Medium => 128,
            Self::Large => 512,
        }
    }

    fn rust_modules(self) -> usize {
        match self {
            Self::Small => 8,
            Self::Medium => 64,
            Self::Large => 256,
        }
    }

    fn plain_lines(self) -> usize {
        match self {
            Self::Small => 128,
            Self::Medium => 1_024,
            Self::Large => 18_000,
        }
    }
}

pub struct Corpus {
    name: String,
    text: String,
}

impl Corpus {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn bytes(&self) -> u64 {
        self.text.len() as u64
    }
}

pub struct ModelDriver {
    pub model: EditorModel,
    clipboard: Option<String>,
    primary: Option<String>,
}

impl ModelDriver {
    pub fn new(name: &str, text: &str) -> Self {
        let tab = EditorTab::from_path_with_stamp(TabId::from_raw(1), PathBuf::from(name), text, None);
        let mut driver = Self {
            model: EditorModel::from_tabs(tab, Vec::new(), "Ready.".to_string()),
            clipboard: None,
            primary: None,
        };
        driver.sync_effects();
        driver
    }

    pub fn with_two_tabs(first_name: &str, first_text: &str, second_name: &str, second_text: &str) -> Self {
        let first = EditorTab::from_path_with_stamp(TabId::from_raw(1), PathBuf::from(first_name), first_text, None);
        let second = EditorTab::from_path_with_stamp(TabId::from_raw(2), PathBuf::from(second_name), second_text, None);
        let mut driver = Self {
            model: EditorModel::from_tabs(first, vec![second], "Ready.".to_string()),
            clipboard: None,
            primary: None,
        };
        driver.sync_effects();
        driver
    }

    pub fn execute(&mut self, command: EditorCommand) {
        self.model.execute(command);
        self.sync_effects();
    }

    pub fn paste_text(&mut self, text: &str) {
        self.model.paste_text(text.to_string());
        self.sync_effects();
    }

    pub fn insert_text_from_input(&mut self, text: &str) {
        self.model.replace_text_from_input(None, text.to_string());
        self.sync_effects();
    }

    pub fn set_clipboard(&mut self, text: String) {
        self.clipboard = Some(text);
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
            let line_count = tab.line_count();
            let selected_lines = requested_lines.min(line_count);
            if selected_lines >= line_count {
                tab.len_chars()
            } else {
                tab.buffer().line_to_char(selected_lines)
            }
        };
        self.model.set_selection(Selection::from_range(0..end, false));
        self.sync_effects();
    }

    pub fn configure_viewport(&mut self) {
        self.model.set_viewport_rows(VIEWPORT_ROWS);
        self.model.set_viewport_top(0);
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

    pub fn cursor(&self) -> Position {
        self.model.active_tab().cursor_position()
    }

    pub fn text_len(&self) -> usize {
        self.model.active_tab().len_chars()
    }

    pub fn selection_count(&self) -> usize {
        self.model.selection_set().as_slice().len()
    }

    pub fn find_match_count(&self) -> usize {
        self.model.find().matches.len()
    }

    pub fn clipboard_len(&self) -> usize {
        self.clipboard.as_ref().map_or(0, |text| text.len())
    }

    pub fn primary_len(&self) -> usize {
        self.primary.as_ref().map_or(0, |text| text.len())
    }
}

pub fn rust_corpus(size: DocumentSize) -> Corpus {
    Corpus {
        name: format!("editor-model-{}.rs", size.label()),
        text: generated_rust_corpus(size.rust_modules(), 8),
    }
}

pub fn plain_corpus(size: DocumentSize) -> Corpus {
    Corpus {
        name: format!("editor-model-{}.txt", size.label()),
        text: generated_plain_corpus(size.plain_lines()),
    }
}

pub fn typing_payload(chars: usize) -> String {
    let seed = "the quick brown fox jumps over a lazy dog ";
    seed.chars().cycle().take(chars).collect()
}

pub fn position_of(text: &str, needle: &str) -> Option<Position> {
    let byte = text.find(needle)?;
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
    Some(Position { line, column })
}

fn generated_rust_corpus(module_count: usize, functions_per_module: usize) -> String {
    let mut text = String::new();
    text.push_str("// Generated benchmark corpus for editor-model benchmarks.\n");
    text.push_str("// It is intentionally deterministic and framework-neutral.\n\n");

    for module_ix in 0..module_count {
        text.push_str(&format!("pub mod bench_module_{module_ix:04} {{\n"));
        text.push_str("    #[derive(Clone, Debug, PartialEq, Eq)]\n");
        text.push_str("    pub struct RowState {\n");
        text.push_str("        pub id: usize,\n");
        text.push_str("        pub label: &'static str,\n");
        text.push_str("        pub selected: bool,\n");
        text.push_str("    }\n\n");

        for function_ix in 0..functions_per_module {
            let factor = module_ix + function_ix + 3;
            text.push_str(&format!(
                "    pub fn transform_{module_ix:04}_{function_ix:02}(input: usize) -> RowState {{\n"
            ));
            text.push_str(&format!(
                "        let scaled = input.wrapping_mul({factor}).wrapping_add({module_ix});\n"
            ));
            text.push_str("        let selected = scaled % 7 == 0 || scaled % 11 == 0;\n");
            text.push_str("        let label = if selected { \"selected\" } else { \"idle\" };\n");
            text.push_str("        RowState { id: scaled, label, selected }\n");
            text.push_str("    }\n\n");
        }

        text.push_str("}\n\n");
    }

    text
}

fn generated_plain_corpus(lines: usize) -> String {
    let mut text = String::new();
    text.push_str("Generated plain-text benchmark corpus for editor model navigation.\n");
    for line_ix in 0..lines {
        text.push_str(&format!(
            "row {line_ix:05}: viewport measurement text with numbers {} {} {}\n",
            line_ix % 17,
            line_ix % 31,
            line_ix % 127
        ));
    }
    text
}
