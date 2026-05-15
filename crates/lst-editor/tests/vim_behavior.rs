use std::path::PathBuf;

use lst_editor::{
    vim::{self, Key, NamedKey},
    EditorCommand, EditorEffect, EditorModel, EditorTab, FocusTarget, Position, RevealIntent, TabId,
};

const WRAP_COLUMNS: usize = 80;

struct VimHarness {
    model: EditorModel,
    focus: FocusTarget,
    effects: Vec<EditorEffect>,
    deferred_find_query: Option<String>,
}

impl VimHarness {
    fn new(text: &str) -> Self {
        let tab = EditorTab::from_path_with_stamp(TabId::from_raw(1), PathBuf::from("vim-spec.md"), text, None);
        let mut harness = Self { model: EditorModel::from_tabs(tab, Vec::new(), "Ready.".to_string()), focus: FocusTarget::Editor, effects: Vec::new(), deferred_find_query: None };
        harness.sync_effects();
        harness
    }

    fn normal_at(text: &str, line: usize, column: usize) -> Self {
        let mut harness = Self::new(text);
        harness.keys("<esc>");
        harness.model.set_active_cursor_position(line, column);
        harness.sync_effects();
        harness.clear_effects();
        harness
    }

    fn with_two_tabs(first: &str, second: &str) -> Self {
        let first = EditorTab::from_path_with_stamp(TabId::from_raw(1), PathBuf::from("first.md"), first, None);
        let second = EditorTab::from_path_with_stamp(TabId::from_raw(2), PathBuf::from("second.md"), second, None);
        let mut harness = Self { model: EditorModel::from_tabs(first, vec![second], "Ready.".to_string()), focus: FocusTarget::Editor, effects: Vec::new(), deferred_find_query: None };
        harness.sync_effects();
        harness
    }

    fn keys(&mut self, sequence: &str) {
        for (key, modifiers) in parse_keys(sequence) {
            self.key(key, modifiers);
        }
    }

    fn key(&mut self, key: Key, modifiers: vim::Modifiers) {
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
                self.deferred_find_query.get_or_insert_with(|| self.model.find().query.clone()).push_str(&text);
            }
            Key::Named(NamedKey::Backspace) => {
                let mut query = self.deferred_find_query.take().unwrap_or_else(|| self.model.find().query.clone());
                query.pop();
                self.deferred_find_query = Some(query);
            }
            Key::Named(NamedKey::Enter) => {
                let query = self.deferred_find_query.take().unwrap_or_else(|| self.model.find().query.clone());
                self.model.update_find_query_and_activate(query);
                self.model.close_find_panel();
            }
            _ => {}
        }
    }

    fn sync_effects(&mut self) {
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

    fn clear_effects(&mut self) {
        self.model.drain_effects();
        self.effects.clear();
    }

    fn take_effects(&mut self) -> Vec<EditorEffect> {
        self.sync_effects();
        std::mem::take(&mut self.effects)
    }

    fn text(&self) -> String {
        self.model.active_tab().buffer_text()
    }

    fn cursor(&self) -> Position {
        self.model.active_tab().cursor_position()
    }

    fn selected_text(&self) -> Option<String> {
        self.model.active_tab().selected_text()
    }

    #[track_caller]
    fn expect_text(&self, expected: &str) {
        assert_eq!(self.text(), expected);
    }

    #[track_caller]
    fn expect_cursor(&self, line: usize, column: usize) {
        assert_eq!(self.cursor(), Position { line, column });
    }

    #[track_caller]
    fn expect_mode(&self, expected: vim::Mode) {
        assert_eq!(self.model.vim_mode(), expected);
    }

    #[track_caller]
    fn expect_selection(&self, expected: &str) {
        assert_eq!(self.selected_text().as_deref(), Some(expected));
    }

    #[track_caller]
    fn expect_no_selection(&self) {
        assert_eq!(self.selected_text(), None);
    }

    #[track_caller]
    fn expect_pending(&self, expected: &str) {
        assert_eq!(self.model.vim_pending_display(), expected);
    }
}

fn parse_keys(sequence: &str) -> Vec<(Key, vim::Modifiers)> {
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
        } else if let Some(stripped) = rest.strip_prefix("cmd-").or_else(|| rest.strip_prefix("command-")).or_else(|| rest.strip_prefix("super-")) {
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

#[test]
fn x11_vim_smoke_specs_run_through_the_editor_model() {
    let cases = [
        ("top line delete", "A<enter>B<enter>C<enter><esc>ggdd", "B\nC\n"),
        ("visual line indent", "alpha<enter>beta<enter>gamma<esc>gg0Vjj><esc>", "  alpha\n  beta\n  gamma"),
        ("surround inner word", "hello<esc>0ysiw)", "(hello)"),
        ("change inner word", "hello world<esc>0ciwHEY<esc>", "HEY world"),
        ("normal open join replace", "foo<enter>bar<esc>ggOtop<esc>jJ0rx", "top\nxoo bar"),
        ("linewise paste", "one<enter>two<enter>three<esc>ggyyGp", "one\ntwo\nthree\none"),
        ("surround change delete", "hello<esc>0ysiw)cs)]ds[", "hello"),
        ("visual text object case", "hello world<esc>0viwU", "HELLO world"),
    ];

    for (name, keys, expected) in cases {
        let mut harness = VimHarness::new("");
        harness.keys(keys);
        assert_eq!(harness.text(), expected, "{name}");
    }
}

#[test]
fn normal_motions_cover_words_lines_char_search_and_brackets() {
    let cases = [
        ("line start", "abc def", (0, 4), "0", (0, 0)),
        ("first nonblank", "  abc", (0, 4), "^", (0, 2)),
        ("line end", "abc", (0, 0), "$", (0, 2)),
        ("word forward", "  alpha beta", (0, 0), "w", (0, 2)),
        ("word end", "alpha beta", (0, 0), "e", (0, 4)),
        ("word backward", "alpha beta", (0, 8), "b", (0, 6)),
        ("big word end", "a+b c", (0, 0), "E", (0, 2)),
        ("document start", "  a\n b\nc", (2, 0), "gg", (0, 2)),
        ("counted gg", "a\n  b\nc", (0, 0), "2gg", (1, 2)),
        ("document end", "a\n  b\n c", (0, 0), "G", (2, 1)),
        ("counted G", "a\n  b\n c", (0, 0), "2G", (1, 2)),
        ("matching bracket", "call(foo)", (0, 4), "%", (0, 8)),
        ("line percentage", "a\nb\nc\nd", (0, 0), "50%", (1, 0)),
        ("find char", "abc abc", (0, 0), "fc", (0, 2)),
        ("till char", "abc abc", (0, 0), "tc", (0, 1)),
        ("counted find char", "abc abc", (0, 0), "2fb", (0, 5)),
        ("find char backward", "abc abc", (0, 6), "Fb", (0, 5)),
        ("till char backward", "abc abc", (0, 6), "T ", (0, 4)),
        ("repeat and reverse char search", "abcabc", (0, 0), "fc;,", (0, 2)),
    ];

    for (name, text, start, keys, expected) in cases {
        let mut harness = VimHarness::normal_at(text, start.0, start.1);
        harness.keys(keys);
        assert_eq!(harness.cursor(), Position { line: expected.0, column: expected.1 }, "{name}",);
    }
}

#[test]
fn vertical_motions_preserve_preferred_column() {
    let mut harness = VimHarness::normal_at("abcdef\nab\nabcdef", 0, 5);

    harness.keys("j");
    harness.expect_cursor(1, 1);
    harness.keys("j");
    harness.expect_cursor(2, 5);
    harness.keys("k");
    harness.expect_cursor(1, 1);
    harness.keys("0j");
    harness.expect_cursor(2, 0);
}

#[test]
fn modes_state_pending_and_escape_follow_vim_contracts() {
    let mut harness = VimHarness::new("");
    harness.keys("abc<esc>");
    harness.expect_text("abc");
    harness.expect_mode(vim::Mode::Normal);
    harness.expect_cursor(0, 2);

    let mut harness = VimHarness::new("");
    harness.keys("a\u{301}b<esc>");
    harness.expect_cursor(0, 2);
    harness.keys("h");
    harness.expect_cursor(0, 0);

    let mut harness = VimHarness::normal_at("alpha beta", 0, 0);
    harness.keys("vww");
    harness.expect_mode(vim::Mode::Visual);
    harness.expect_selection("alpha beta");
    harness.keys("<esc>");
    harness.expect_mode(vim::Mode::Normal);
    harness.expect_no_selection();

    let mut harness = VimHarness::with_two_tabs("alpha beta", "other");
    harness.keys("<esc>vww");
    harness.expect_mode(vim::Mode::Visual);
    let second = harness.model.tab_id_at(1).unwrap();
    harness.model.set_active_tab(second);
    harness.sync_effects();
    harness.expect_mode(vim::Mode::Normal);
    harness.expect_no_selection();

    let mut harness = VimHarness::normal_at("alpha beta", 0, 0);
    for (keys, pending) in [("2", "2"), ("d", "2d"), ("3", "2d3"), ("<esc>", "")] {
        harness.keys(keys);
        harness.expect_pending(pending);
    }
    harness.keys("y");
    harness.expect_pending("y");
    harness.keys("s");
    harness.expect_pending("ys");
    harness.keys("i");
    harness.expect_pending("ysi");
    harness.keys("w");
    harness.expect_pending("ys…");
    harness.keys("<esc>");
    harness.expect_pending("");
}

#[test]
fn named_and_page_motions_cover_keyboard_boundary_paths() {
    let mut harness = VimHarness::normal_at("abcdef\nab\nabcdef\nlast", 0, 3);
    harness.keys("<left>");
    harness.expect_cursor(0, 2);
    harness.keys("<right>");
    harness.expect_cursor(0, 3);
    harness.keys("<home>");
    harness.expect_cursor(0, 0);
    harness.keys("<end>");
    harness.expect_cursor(0, 5);
    harness.keys("<down>");
    harness.expect_cursor(1, 1);
    harness.keys("<up>");
    harness.expect_cursor(0, 5);

    let mut harness = VimHarness::normal_at("a\nb\nc\nd\ne\nf\ng\nh", 0, 0);
    harness.model.set_viewport_rows(4);
    harness.keys("<C-d>");
    harness.expect_cursor(2, 0);
    harness.keys("<C-u>");
    harness.expect_cursor(0, 0);
    harness.keys("<C-f>");
    harness.expect_cursor(2, 0);
    harness.keys("<C-b>");
    harness.expect_cursor(0, 0);
    harness.keys("<pagedown>");
    harness.expect_cursor(2, 0);
    harness.keys("<pageup>");
    harness.expect_cursor(0, 0);
}

#[test]
fn word_and_big_word_motions_cover_counts_punctuation_empty_lines_and_unicode() {
    let cases = [
        ("counted word forward", "aa bb cc", (0, 0), "2w", (0, 6)),
        ("counted word end", "aa bb cc", (0, 0), "2e", (0, 4)),
        ("counted word backward", "aa bb cc", (0, 6), "2b", (0, 0)),
        ("word crosses empty line", "aa\n\nbb", (0, 0), "w", (1, 0)),
        ("word end crosses empty line", "aa\n\nbb", (0, 0), "2e", (2, 1)),
        ("big word forward treats punctuation as word", "aa+bb cc", (0, 0), "W", (0, 6)),
        ("small word forward stops after punctuation", "aa+bb cc", (0, 0), "w", (0, 2)),
        ("big word backward treats punctuation as word", "aa+bb cc", (0, 6), "B", (0, 0)),
        ("small word backward stops at identifier run", "aa+bb cc", (0, 6), "b", (0, 3)),
        ("unicode word motion is grapheme aligned", "éclair cafe", (0, 0), "e", (0, 5)),
        ("combining grapheme horizontal right", "a\u{301}bc", (0, 0), "l", (0, 2)),
        ("combining grapheme horizontal left", "a\u{301}bc", (0, 2), "h", (0, 0)),
    ];

    for (name, text, start, keys, expected) in cases {
        let mut harness = VimHarness::normal_at(text, start.0, start.1);
        harness.keys(keys);
        assert_eq!(harness.cursor(), Position { line: expected.0, column: expected.1 }, "{name}");
    }
}

#[test]
fn operators_cover_motion_ranges_text_objects_counts_and_lines() {
    let cases = [
        ("delete word", "alpha beta", (0, 0), "dw", "beta"),
        ("delete to word end", "alpha beta", (0, 0), "de", " beta"),
        ("delete backward word", "alpha beta", (0, 6), "db", "beta"),
        ("delete to end", "abc def\nnext", (0, 4), "D", "abc \nnext"),
        ("delete through multiplied counts", "one two three four five", (0, 0), "2d2w", "five"),
        ("delete lines to end", "a\nb\nc\nd", (1, 0), "dG", "a"),
        ("line change", "alpha\nbeta", (0, 0), "ccX<esc>", "X\nbeta"),
        ("change word uses word-end semantics", "alpha beta", (0, 0), "cwX<esc>", "X beta"),
        ("change big word uses big-word-end semantics", "alpha+beta gamma", (0, 0), "cWX<esc>", "X gamma"),
        ("change quote inner object", "prefix \"alpha beta\" tail", (0, 9), "ci\"X<esc>", "prefix \"X\" tail"),
        ("change quote a-object", "prefix \"alpha beta\" tail", (0, 9), "ca\"X<esc>", "prefix X tail"),
        ("change paren inner object", "call(alpha, beta)", (0, 7), "ci(X<esc>", "call(X)"),
        ("change bracket a-object", "items[one, two] tail", (0, 8), "ca[X<esc>", "itemsX tail"),
    ];

    for (name, text, start, keys, expected) in cases {
        let mut harness = VimHarness::normal_at(text, start.0, start.1);
        harness.keys(keys);
        assert_eq!(harness.text(), expected, "{name}");
        harness.expect_mode(vim::Mode::Normal);
    }
}

#[test]
fn operators_cover_linewise_inclusive_exclusive_and_register_edges() {
    let cases = [
        ("delete down is linewise", "a\nb\nc", (0, 0), "dj", "c"),
        ("delete up is linewise", "a\nb\nc", (2, 0), "dk", "a"),
        ("delete counted line end is linewise", "a\nb\nc", (0, 0), "d2$", "c"),
        ("delete percentage count is linewise", "a\nb\nc\nd", (0, 0), "d50%", "c\nd"),
        ("change backward excludes cursor char", "alpha beta", (0, 6), "cbX<esc>", "Xbeta"),
        ("delete char find is inclusive", "abc def", (0, 0), "dfc", " def"),
        ("delete till find is inclusive to previous char", "abc def", (0, 0), "td", "abc def"),
        ("delete failed find is noop", "abc def", (0, 0), "dz", "abc def"),
        ("yank char range pastes charwise", "alpha beta", (0, 0), "yw$p", "alpha betaalpha "),
        ("yank line count pastes linewise", "one\ntwo\nthree", (0, 0), "2yyGp", "one\ntwo\nthree\none\ntwo"),
        ("empty line delete records line register", "one\n\ntwo", (1, 0), "ddP", "one\n\ntwo"),
    ];

    for (name, text, start, keys, expected) in cases {
        let mut harness = VimHarness::normal_at(text, start.0, start.1);
        harness.keys(keys);
        assert_eq!(harness.text(), expected, "{name}");
    }
}

#[test]
fn normal_edits_cover_insert_positions_substitute_join_replace_paste_and_indent() {
    let cases = [
        ("append after cursor", "abc", (0, 0), "aX<esc>", "aXbc"),
        ("insert at first nonblank", "  abc", (0, 4), "IX<esc>", "  Xabc"),
        ("append at line end", "abc", (0, 0), "AX<esc>", "abcX"),
        ("open below", "alpha\nbeta", (0, 0), "oX<esc>", "alpha\nX\nbeta"),
        ("open above", "alpha\nbeta", (1, 0), "OX<esc>", "alpha\nX\nbeta"),
        ("delete under cursor", "abc", (0, 0), "x", "bc"),
        ("delete count under cursor", "abcd", (0, 0), "2x", "cd"),
        ("delete before cursor", "abcd", (0, 2), "X", "acd"),
        ("substitute one char", "abc", (0, 0), "sZ<esc>", "Zbc"),
        ("substitute counted chars", "abcd", (0, 0), "2sZ<esc>", "Zcd"),
        ("change to end", "abc def", (0, 4), "CXYZ<esc>", "abc XYZ"),
        ("change line preserves indentation", "  abc\nnext", (0, 2), "Snew<esc>", "  new\nnext"),
        ("join one following line", "alpha\n beta", (0, 0), "J", "alpha beta"),
        ("join counted lines", "a\n b\n c\nd", (0, 0), "3J", "a b c\nd"),
        ("replace char", "abc", (0, 0), "rx", "xbc"),
        ("replace counted chars", "abcd", (0, 0), "3rx", "xxxd"),
        ("indent current line", "alpha", (0, 0), ">>", "  alpha"),
        ("outdent current line", "  alpha", (0, 0), "<<", "alpha"),
        ("line yank paste after target", "one\ntwo\nthree", (0, 0), "yyGp", "one\ntwo\nthree\none"),
        ("line yank paste before target", "one\ntwo\nthree", (2, 0), "yyggP", "three\none\ntwo\nthree"),
    ];

    for (name, text, start, keys, expected) in cases {
        let mut harness = VimHarness::normal_at(text, start.0, start.1);
        harness.keys(keys);
        assert_eq!(harness.text(), expected, "{name}");
        harness.expect_mode(vim::Mode::Normal);
    }
}

#[test]
fn normal_edits_cover_counts_boundaries_empty_lines_and_noops() {
    let cases = [
        ("append at empty line", "", (0, 0), "aX<esc>", "X"),
        ("insert on empty line", "", (0, 0), "iX<esc>", "X"),
        ("open below inherits indentation", "  alpha", (0, 2), "oX<esc>", "  alpha\n  X"),
        ("open above inherits indentation", "  alpha", (0, 2), "OX<esc>", "  X\n  alpha"),
        ("delete beyond eol clamps", "abc", (0, 1), "9x", "a"),
        ("delete before bol is noop", "abc", (0, 0), "X", "abc"),
        ("substitute on empty line inserts", "", (0, 0), "sX<esc>", "X"),
        ("delete to end at eol deletes current char", "abc", (0, 2), "D", "ab"),
        ("change to end at eol changes current char", "abc", (0, 2), "CX<esc>", "abX"),
        ("join at last line is noop", "abc", (0, 0), "J", "abc"),
        ("replace on empty line is noop", "", (0, 0), "rx", ""),
        ("counted indent lines", "a\nb\nc", (0, 0), "2>>", "  a\n  b\nc"),
        ("counted outdent lines", "  a\n  b\nc", (0, 0), "2<<", "a\nb\nc"),
    ];

    for (name, text, start, keys, expected) in cases {
        let mut harness = VimHarness::normal_at(text, start.0, start.1);
        harness.keys(keys);
        assert_eq!(harness.text(), expected, "{name}");
    }
}

#[test]
fn visual_mode_covers_charwise_linewise_text_objects_case_and_indentation() {
    let cases = [
        ("delete inner word", "alpha beta", (0, 0), "viwd", " beta"),
        ("change inner word", "alpha beta", (0, 0), "viwcX<esc>", "X beta"),
        ("lowercase selection", "ALPHA beta", (0, 0), "viwu", "alpha beta"),
        ("uppercase selection", "alpha beta", (0, 0), "viwU", "ALPHA beta"),
        ("visual line delete", "alpha\nbeta\ngamma", (0, 0), "Vjd", "gamma"),
        ("visual line change", "alpha\nbeta\ngamma", (0, 0), "VjcX<esc>", "X\ngamma"),
        ("visual line indent", "alpha\nbeta\ngamma", (0, 0), "Vj>", "  alpha\n  beta\ngamma"),
        ("visual line outdent", "  alpha\n  beta\ngamma", (0, 0), "Vj<", "alpha\nbeta\ngamma"),
        ("visual text object selects quotes", "a \"two words\" z", (0, 4), "vi\"U", "a \"TWO WORDS\" z"),
    ];

    for (name, text, start, keys, expected) in cases {
        let mut harness = VimHarness::normal_at(text, start.0, start.1);
        harness.keys(keys);
        assert_eq!(harness.text(), expected, "{name}");
        harness.expect_mode(vim::Mode::Normal);
    }
}

#[test]
fn visual_mode_covers_counts_reverse_selection_search_repeat_and_viewport() {
    let mut harness = VimHarness::normal_at("alpha beta gamma", 0, 0);
    harness.keys("v2w");
    harness.expect_selection("alpha beta g");
    harness.keys("y$p");
    harness.expect_text("alpha beta gammaalpha beta g");

    let mut harness = VimHarness::normal_at("alpha beta gamma", 0, 11);
    harness.keys("vbU");
    harness.expect_text("alpha BETA Gamma");

    let mut harness = VimHarness::normal_at("abc abc abc", 0, 0);
    harness.keys("vfc;");
    harness.expect_selection("abc abc");
    harness.keys(",");
    harness.expect_selection("abc");

    let mut harness = VimHarness::normal_at("alpha beta alpha beta", 0, 0);
    harness.keys("v/beta<enter>n");
    harness.expect_mode(vim::Mode::Visual);
    harness.expect_selection("alpha beta alpha b");
    harness.keys("N");
    harness.expect_selection("alpha b");

    let text = (0..12).map(|line| format!("line {line}")).collect::<Vec<_>>().join("\n");
    let mut harness = VimHarness::normal_at(&text, 0, 0);
    harness.model.set_viewport_rows(7);
    harness.model.set_viewport_top(4);
    harness.keys("vH");
    harness.expect_mode(vim::Mode::Visual);
    harness.expect_selection("line 0\nline 1\nline 2\nline 3\nl");
    harness.keys("<esc>");
    harness.expect_mode(vim::Mode::Normal);

    let mut harness = VimHarness::normal_at(&text, 2, 0);
    harness.model.set_viewport_rows(4);
    harness.keys("v<C-f>");
    harness.expect_mode(vim::Mode::Visual);
    harness.expect_selection("line 2\nline 3\nl");
}

#[test]
fn search_commands_cover_word_search_find_panel_and_visual_stepping() {
    let mut harness = VimHarness::normal_at("foo bar foo baz foo", 0, 0);
    harness.keys("*");
    harness.expect_cursor(0, 8);
    harness.keys("n");
    harness.expect_cursor(0, 16);
    harness.keys("N");
    harness.expect_cursor(0, 8);
    harness.keys("#");
    harness.expect_cursor(0, 0);

    let mut harness = VimHarness::normal_at("alpha beta alpha", 0, 0);
    harness.keys("/alpha<enter>n");
    harness.expect_cursor(0, 11);
    assert_eq!(harness.focus, FocusTarget::Editor);

    let mut harness = VimHarness::normal_at("alpha beta alpha", 0, 0);
    harness.keys("v/beta<enter>");
    harness.expect_mode(vim::Mode::Visual);
    assert_eq!(harness.model.active_tab().selected_text().as_deref(), Some("alpha b"));
}

#[test]
fn search_commands_cover_wrap_empty_words_and_find_query_editing() {
    let mut harness = VimHarness::normal_at("foo bar foo", 0, 8);
    harness.keys("n");
    harness.expect_cursor(0, 8);
    harness.keys("/foo<enter>N");
    harness.expect_cursor(0, 0);
    harness.keys("N");
    harness.expect_cursor(0, 8);

    let mut harness = VimHarness::normal_at("foo bar foo", 0, 4);
    harness.keys("*");
    harness.expect_cursor(0, 4);
    harness.keys("#");
    harness.expect_cursor(0, 4);

    let mut harness = VimHarness::normal_at("foo bar baz", 0, 0);
    harness.keys("/baq<bs>r<enter>");
    assert_eq!(harness.model.find().query, "bar");
    harness.expect_cursor(0, 4);
}

#[test]
fn text_objects_cover_words_paragraphs_pairs_quotes_counts_and_escapes() {
    let cases = [
        ("change a word consumes following space", "alpha beta", (0, 0), "cawX<esc>", "Xbeta"),
        ("change inner big word", "alpha+beta gamma", (0, 3), "ciWX<esc>", "X gamma"),
        ("change a big word", "alpha+beta gamma", (0, 3), "caWX<esc>", "Xgamma"),
        ("word object count extends through following words", "one two three four", (0, 0), "d2aw", "three four"),
        ("inner paragraph is linewise", "one\ntwo\n\nthree", (0, 0), "dip", "\nthree"),
        ("a paragraph includes following blank line", "one\ntwo\n\nthree", (0, 0), "dap", "three"),
        ("paren inner via close delimiter", "call(alpha)", (0, 10), "ci)X<esc>", "call(X)"),
        ("paren a-object alias b", "call(alpha)", (0, 6), "dab", "call"),
        ("brace inner via B alias", "fn { alpha }", (0, 5), "ciBX<esc>", "fn {X}"),
        ("brace a-object", "fn { alpha } tail", (0, 5), "da}", "fn  tail"),
        ("bracket inner via close delimiter", "items[alpha]", (0, 8), "ci]X<esc>", "items[X]"),
        ("angle inner via close delimiter", "tag<alpha>", (0, 5), "ci>X<esc>", "tag<X>"),
        ("single quote object", "let 'alpha' tail", (0, 6), "ci'X<esc>", "let 'X' tail"),
        ("backtick object", "let `alpha` tail", (0, 6), "ca`X<esc>", "let X tail"),
        ("escaped quote stays inside quote object", "let \"a\\\"b\" tail", (0, 7), "ci\"X<esc>", "let \"X\" tail"),
    ];

    for (name, text, start, keys, expected) in cases {
        let mut harness = VimHarness::normal_at(text, start.0, start.1);
        harness.keys(keys);
        assert_eq!(harness.text(), expected, "{name}");
    }
}

#[test]
fn surround_commands_cover_motion_text_object_and_delimiter_variants() {
    let cases = [
        ("surround inner word with brackets", "hello", (0, 0), "ysiw]", "[hello]"),
        ("surround a word includes trailing space", "hello world", (0, 0), "ysaw)", "(hello )world"),
        ("surround to line end", "hello world", (0, 0), "ys$\"", "\"hello world\""),
        ("change parens to braces", "(hello)", (0, 1), "cs){", "{hello}"),
        ("delete brackets", "[hello]", (0, 1), "ds[", "hello"),
        ("change quotes to backticks", "\"hello\"", (0, 1), "cs\"`", "`hello`"),
        ("delete backticks", "`hello`", (0, 1), "ds`", "hello"),
    ];

    for (name, text, start, keys, expected) in cases {
        let mut harness = VimHarness::normal_at(text, start.0, start.1);
        harness.keys(keys);
        assert_eq!(harness.text(), expected, "{name}");
    }
}

#[test]
fn surround_commands_cover_all_delimiters_aliases_motion_counts_and_noops() {
    let cases = [
        ("paren b alias", "hello", (0, 0), "ysiwb", "(hello)"),
        ("brace B alias", "hello", (0, 0), "ysiwB", "{hello}"),
        ("angle delimiter", "hello", (0, 0), "ysiw>", "<hello>"),
        ("single quote delimiter", "hello", (0, 0), "ysiw'", "'hello'"),
        ("backtick delimiter", "hello", (0, 0), "ysiw`", "`hello`"),
        ("counted motion surround", "one two three", (0, 0), "ys2w]", "[one two ]three"),
        ("delete paren alias b", "(hello)", (0, 1), "dsb", "hello"),
        ("delete brace alias B", "{hello}", (0, 1), "dsB", "hello"),
        ("change angle to quote", "<hello>", (0, 1), "cs>\"", "\"hello\""),
        ("missing surround delete is noop", "hello", (0, 0), "ds)", "hello"),
        ("invalid surround delimiter is noop", "hello", (0, 0), "ysiww", "hello"),
    ];

    for (name, text, start, keys, expected) in cases {
        let mut harness = VimHarness::normal_at(text, start.0, start.1);
        harness.keys(keys);
        assert_eq!(harness.text(), expected, "{name}");
    }
}

#[test]
fn registers_preserve_charwise_and_linewise_paste_placement() {
    let mut harness = VimHarness::normal_at("alpha beta", 0, 0);
    harness.keys("yiw$p");
    harness.expect_text("alpha betaalpha");

    let mut harness = VimHarness::normal_at("alpha beta", 0, 6);
    harness.keys("diwP");
    harness.expect_text("alphabeta ");

    let mut harness = VimHarness::normal_at("one\ntwo\nthree", 1, 0);
    harness.keys("ddP");
    harness.expect_text("one\ntwo\nthree");
}

#[test]
fn paste_placement_covers_charwise_linewise_before_after_and_empty_registers() {
    let cases = [
        ("empty paste after is noop", "alpha", (0, 0), "p", "alpha"),
        ("empty paste before is noop", "alpha", (0, 0), "P", "alpha"),
        ("char delete paste after cursor", "alpha beta", (0, 0), "dw$p", "betaalpha "),
        ("char delete paste before cursor", "alpha beta", (0, 0), "dwP", "alpha beta"),
        ("line delete paste after last", "one\ntwo", (0, 0), "ddGp", "two\none"),
        ("line delete paste before first", "one\ntwo", (1, 0), "ddggP", "two\none"),
    ];

    for (name, text, start, keys, expected) in cases {
        let mut harness = VimHarness::normal_at(text, start.0, start.1);
        harness.keys(keys);
        assert_eq!(harness.text(), expected, "{name}");
    }
}

#[test]
fn undo_redo_and_last_edit_jump_track_vim_edits() {
    let mut harness = VimHarness::new("");
    harness.keys("abc<esc>u<cmd-r>");
    harness.expect_text("abc");

    let mut harness = VimHarness::normal_at("alpha\nbeta", 0, 0);
    harness.keys("A!<esc>ggg;");
    harness.expect_cursor(0, 6);
    harness.keys("gi?<esc>");
    harness.expect_text("alpha!?\nbeta");
}

#[test]
fn viewport_commands_emit_reveal_effects_and_move_to_visible_rows() {
    let text = (0..24).map(|line| format!("line {line}")).collect::<Vec<_>>().join("\n");
    let mut harness = VimHarness::normal_at(&text, 0, 0);
    harness.model.set_viewport_rows(11);
    harness.model.set_viewport_top(10);
    harness.clear_effects();

    harness.keys("H");
    harness.expect_cursor(10, 0);
    harness.keys("M");
    harness.expect_cursor(15, 0);
    harness.keys("L");
    harness.expect_cursor(20, 0);

    harness.clear_effects();
    harness.keys("zzztzb");
    let effects = harness.take_effects();
    assert!(effects.contains(&EditorEffect::Reveal(RevealIntent::Center)));
    assert!(effects.contains(&EditorEffect::Reveal(RevealIntent::Top)));
    assert!(effects.contains(&EditorEffect::Reveal(RevealIntent::Bottom)));
}

#[test]
fn viewport_page_motions_preserve_visual_state() {
    let text = (0..16).map(|line| format!("line {line}")).collect::<Vec<_>>().join("\n");
    let mut harness = VimHarness::normal_at(&text, 3, 0);
    harness.model.set_viewport_rows(6);
    harness.keys("v<C-d>");
    harness.expect_mode(vim::Mode::Visual);
    harness.expect_selection("line 3\nline 4\nline 5\nl");
    harness.keys("<C-u>");
    harness.expect_mode(vim::Mode::Visual);
    harness.expect_selection("l");
    harness.keys("<esc>");

    harness.clear_effects();
    harness.keys("<pagedown><pageup>");
    let effects = harness.take_effects();
    assert!(effects.contains(&EditorEffect::Reveal(RevealIntent::NearestEdge)));
}
