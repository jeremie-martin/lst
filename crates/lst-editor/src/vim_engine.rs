use crate::{
    document::{
        char_to_position, inclusive_position_to_exclusive_char, position_start_char, position_to_char, EditKind,
        UndoBoundary,
    },
    line_edit,
    selection::{
        self, cell_containing_char, cell_partition_by_char, cells_of_str, char_at_line_column, line_display_text,
        paragraph_range_at_char, vim_token_class, GraphemeCell, Position, Selection, TokenClass,
    },
    transaction::{EditRequest, SelectionAfter, TextChange, TextChangeSet},
    vim::{self, Key},
    EditorCommand, EditorModel, FocusTarget, RevealIntent,
};
use std::ops::Range;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Op {
    Delete,
    Change,
    Yank,
}

#[derive(Clone, Copy)]
struct Motion {
    at: usize,
    linewise: bool,
    inclusive: bool,
}

struct PositionRange {
    start: Position,
    end: Position,
}

impl EditorModel {
    pub(crate) fn vim_handle_key(&mut self, key: Key, mods: vim::Modifiers, _wrap_columns: usize) -> bool {
        if mods.command && key == Key::Character("r".into()) {
            self.execute(EditorCommand::Redo);
            self.vim_collapse_restored_selection();
            return true;
        }
        if mods.command {
            return false;
        }
        if !mods.control {
            if let Key::Named(named) = key {
                if self.vim.mode == vim::Mode::Insert {
                    return false;
                }
                if !self.vim.pending.is_empty() {
                    self.vim.pending.clear();
                }
                use vim::NamedKey::*;
                let motion = match named {
                    ArrowLeft => self.vim_motion_command(1, "h", false),
                    ArrowRight => self.vim_motion_command(1, "l", false),
                    ArrowUp => self.vim_motion_command(1, "k", false),
                    ArrowDown => self.vim_motion_command(1, "j", false),
                    Home => self.vim_motion_command(1, "0", false),
                    End => self.vim_motion_command(1, "$", false),
                    PageUp | PageDown => Some(self.vertical(
                        if named == PageDown {
                            self.viewport.page() as isize
                        } else {
                            -(self.viewport.page() as isize)
                        },
                        false,
                    )),
                    _ => None,
                };
                if let Some(motion) = motion {
                    self.vim_move_to(motion.at);
                    return true;
                }
                return false;
            }
        }
        let Key::Character(text) = key else { return false };
        let Some(ch) = text.chars().next() else { return false };
        if mods.control {
            return self.vim_control(ch);
        }
        if self.vim.mode == vim::Mode::Insert {
            return false;
        }
        self.vim.pending.push(ch);
        self.vim_dispatch_pending()
    }

    pub(crate) fn vim_escape(&mut self) -> bool {
        match self.vim.mode {
            vim::Mode::Insert => self.vim_normalize_insert_cursor(),
            vim::Mode::Visual | vim::Mode::VisualLine => {
                if let Some(state) = self.vim.visual {
                    self.active_tab_mut().set_cursor_position(state.head, None);
                }
            }
            vim::Mode::Normal => {}
        }
        self.vim_finish_normal();
        true
    }

    fn vim_dispatch_pending(&mut self) -> bool {
        let pending = self.vim.pending.clone();
        let (count, rest) = count_prefix(&pending);
        if rest.is_empty() {
            return true;
        }
        if self.vim.mode != vim::Mode::Normal {
            return self.vim_visual_command(count, rest);
        }
        if let Some(op) = match rest.as_bytes()[0] {
            b'd' => Some(Op::Delete),
            b'c' => Some(Op::Change),
            b'y' => Some(Op::Yank),
            _ => None,
        } {
            if self.vim_operator(count, rest, op) {
                self.vim.pending.clear();
            }
            return true;
        }
        let done = match rest {
            "i" => self.vim_insert_at(self.active_tab().cursor_char()),
            "a" => self.vim_insert_at(self.vim_append_offset()),
            "I" => self.vim_insert_at(self.line_first_nonblank(self.cursor().line)),
            "A" => self.vim_insert_at(self.line_end(self.cursor().line)),
            "o" | "O" => self.vim_open_line(rest == "o"),
            "v" => self.vim_toggle_visual(false),
            "V" => self.vim_toggle_visual(true),
            "u" => {
                self.execute(EditorCommand::Undo);
                self.vim_collapse_restored_selection();
                true
            }
            "D" => self.vim_apply_range(
                Op::Delete,
                self.cursor_offset()..self.next_grapheme(self.line_last(self.cursor().line)),
            ),
            "C" => self.vim_apply_range(
                Op::Change,
                self.cursor_offset()..self.next_grapheme(self.line_last(self.cursor().line)),
            ),
            "S" => self.vim_apply_lines_span(Op::Change, self.cursor().line, self.cursor().line),
            "x" | "X" => self.vim_delete_counted_chars(count, rest == "x"),
            "s" => self.vim_substitute(count),
            "J" => self.vim_join(count),
            "p" | "P" => self.vim_paste(rest == "p"),
            "r" => false,
            _ if rest.strip_prefix('r').and_then(|s| s.chars().next()).is_some() => {
                self.vim_replace_chars(count, rest.chars().nth(1).unwrap());
                true
            }
            _ if rest.starts_with(['f', 'F', 't', 'T']) && rest.chars().count() == 2 => {
                let mut chars = rest.chars();
                let cmd = chars.next().unwrap();
                let target = chars.next().unwrap();
                if let Some(motion) = self.vim_char_motion(count, cmd, target, false) {
                    self.vim_move_to(motion.at);
                }
                true
            }
            "*" | "#" => self.vim_word_search(rest == "*"),
            "n" | "N" => self.vim_search_step(rest == "n"),
            "/" | "?" => self.vim_open_search(rest == "?"),
            "gg" | "g;" | "gi" | "zz" | "zt" | "zb" | ">>" | "<<" => self.vim_special(count, rest),
            _ => {
                if let Some(motion) = self.vim_motion_command(count, rest, false) {
                    self.vim_move_to(motion.at);
                    true
                } else {
                    false
                }
            }
        };
        if done || !is_pending_prefix(rest) {
            self.vim.pending.clear();
        }
        true
    }

    fn vim_operator(&mut self, op_count: usize, rest: &str, op: Op) -> bool {
        let (motion_count, motion) = count_prefix(&rest[1..]);
        let count = op_count.saturating_mul(motion_count).max(1);
        if motion.is_empty() || matches!(motion, "g" | "i" | "a") {
            return false;
        }
        if motion == &rest[..1] {
            let first = self.cursor().line;
            return self.vim_apply_lines_span(op, first, first + count.saturating_sub(1));
        }
        if motion == "gg" {
            let explicit_count = op_count > 1 || rest.as_bytes()[1..].first().is_some_and(u8::is_ascii_digit);
            let target = if explicit_count { count.saturating_sub(1) } else { 0 };
            return self.vim_apply_lines_span(op, self.cursor().line, target);
        }
        if let Some(range) = self.vim_text_object(motion, count) {
            if matches!(motion, "ip" | "ap") {
                let start = char_to_position(self.active_tab().buffer(), range.start).line;
                let end_pos = char_to_position(self.active_tab().buffer(), range.end);
                let last = if end_pos.column == 0 && end_pos.line > start {
                    end_pos.line - 1
                } else {
                    end_pos.line
                };
                return self.vim_apply_lines_span(op, start, last);
            }
            return self.vim_apply_range(op, range);
        }
        if motion.starts_with(['f', 'F', 't', 'T']) && motion.chars().count() < 2 {
            return false;
        }
        let op_motion = match (op, motion) {
            (Op::Change, "w") => "e",
            (Op::Change, "W") => "E",
            _ => motion,
        };
        if motion == "$" && count > 1 {
            let first = self.cursor().line;
            let last = (first + count - 1).min(self.active_tab().line_count() - 1);
            if op == Op::Delete {
                return self.vim_apply_lines_span(op, first, last);
            }
            return self.vim_apply_range(op, self.line_start(first)..self.line_end(last));
        }
        if let Some(m) = self.vim_motion_command(count, op_motion, true) {
            return self.vim_apply_motion(op, m);
        }
        true
    }

    fn vim_visual_command(&mut self, count: usize, rest: &str) -> bool {
        let done = match rest {
            "v" => self.vim_toggle_visual(false),
            "V" => self.vim_toggle_visual(true),
            "d" => self.vim_apply_visual(Op::Delete),
            "c" | "s" => self.vim_apply_visual(Op::Change),
            "y" => self.vim_apply_visual(Op::Yank),
            "u" | "U" => self.vim_case_visual(rest == "U"),
            "p" | "P" => self.vim_visual_paste(rest == "p"),
            ">" | "<" => self.vim_indent_visual(rest == ">"),
            "/" | "?" => self.vim_open_search(rest == "?"),
            "n" | "N" => self.vim_search_step(rest == "n"),
            _ => {
                if let Some(range) = self.vim_text_object(rest, count) {
                    self.vim_set_visual_range(range);
                    true
                } else if let Some(motion) = self.vim_motion_command(count, rest, false) {
                    self.vim_extend_visual(motion.at);
                    true
                } else {
                    false
                }
            }
        };
        if done || !is_pending_prefix(rest) {
            self.vim.pending.clear();
        }
        true
    }

    fn vim_motion_command(&mut self, count: usize, cmd: &str, operator: bool) -> Option<Motion> {
        let n = count.max(1);
        let cur = self.cursor();
        let at = match cmd {
            "h" => repeat_offset(self, self.cursor_offset(), n, |s, off| s.prev_grapheme(off)),
            "l" => repeat_offset(self, self.cursor_offset(), n, |s, off| s.next_normal_grapheme(off)),
            "0" => self.line_start(cur.line),
            "^" => self.line_first_nonblank(cur.line),
            "$" => self.line_last(cur.line),
            "w" | "W" => repeat_offset(self, self.cursor_offset(), n, |s, off| s.word_forward(off, cmd == "W")),
            "e" | "E" => repeat_offset(self, self.cursor_offset(), n, |s, off| s.word_end(off, cmd == "E")),
            "b" | "B" => repeat_offset(self, self.cursor_offset(), n, |s, off| s.word_backward(off, cmd == "B")),
            "j" => return Some(self.vertical(n as isize, operator)),
            "k" => return Some(self.vertical(-(n as isize), operator)),
            "G" => {
                let at = self.line_col_offset(
                    if count == 1 {
                        self.active_tab().line_count() - 1
                    } else {
                        n - 1
                    },
                    cur.column,
                );
                return Some(Motion {
                    at,
                    linewise: operator,
                    inclusive: false,
                });
            }
            "%" => {
                let at = self.percent_or_match(n);
                if at == self.cursor_offset() && self.active_tab().buffer().get_char(at).and_then(bracket_dir).is_none()
                {
                    return None;
                }
                return Some(Motion {
                    at,
                    linewise: operator && count > 1,
                    inclusive: count == 1,
                });
            }
            "H" => return Some(self.screen_motion(self.viewport.screen_top_row())),
            "M" => return Some(self.screen_motion(self.viewport.screen_middle_row())),
            "L" => return Some(self.screen_motion(self.viewport.screen_bottom_row())),
            ";" | "," => return self.repeat_char_find(cmd == ";", operator),
            _ if cmd.starts_with(['f', 'F', 't', 'T']) && cmd.chars().count() == 2 => {
                let mut chars = cmd.chars();
                return self.vim_char_motion(n, chars.next().unwrap(), chars.next().unwrap(), operator);
            }
            _ => return None,
        };
        self.active_tab_mut().clear_preferred_column();
        Some(Motion {
            at,
            linewise: false,
            inclusive: matches!(cmd, "$" | "e" | "E" | "%"),
        })
    }

    fn vim_control(&mut self, ch: char) -> bool {
        let delta = match ch {
            'd' => self.viewport.half_page() as isize,
            'u' => -(self.viewport.half_page() as isize),
            'f' => self.viewport.page() as isize,
            'b' => -(self.viewport.page() as isize),
            _ => return false,
        };
        let motion = self.vertical(delta, false);
        if self.vim_in_visual() {
            self.vim_extend_visual(motion.at);
        } else {
            self.vim_move_to(motion.at);
        }
        self.queue_reveal(RevealIntent::NearestEdge);
        true
    }

    fn vim_special(&mut self, count: usize, rest: &str) -> bool {
        match rest {
            "gg" => self.vim_move_to(self.line_col_offset(count.saturating_sub(1), self.cursor().column)),
            "g;" => {
                if let Some(pos) = self.active_tab().last_edit_position() {
                    self.move_to_char(pos, false, None);
                }
            }
            "gi" => {
                let pos = self
                    .active_tab()
                    .last_edit_position()
                    .unwrap_or_else(|| self.cursor_offset());
                self.vim_insert_at(pos);
            }
            "zz" => self.queue_reveal(RevealIntent::Center),
            "zt" => self.queue_reveal(RevealIntent::Top),
            "zb" => self.queue_reveal(RevealIntent::Bottom),
            ">>" => self.vim_indent_lines(self.cursor().line, self.cursor().line + count.saturating_sub(1), true),
            "<<" => self.vim_indent_lines(self.cursor().line, self.cursor().line + count.saturating_sub(1), false),
            _ => {}
        }
        true
    }

    fn vim_apply_motion(&mut self, op: Op, motion: Motion) -> bool {
        if motion.linewise {
            let a = self
                .cursor()
                .line
                .min(char_to_position(self.active_tab().buffer(), motion.at).line);
            let b = self
                .cursor()
                .line
                .max(char_to_position(self.active_tab().buffer(), motion.at).line);
            return self.vim_apply_lines_span(op, a, b);
        }
        let cur = self.cursor_offset();
        let mut range = if motion.at >= cur {
            cur..motion.at
        } else {
            motion.at..cur
        };
        if motion.inclusive && motion.at >= cur {
            range.end = self.next_grapheme(range.end);
        }
        self.vim_apply_range(op, range)
    }

    fn vim_apply_range(&mut self, op: Op, range: Range<usize>) -> bool {
        if range.start >= range.end {
            if op == Op::Change {
                self.vim.mode = vim::Mode::Insert;
                self.clear_vim_transient_state();
            }
            return true;
        }
        let start = range.start;
        self.vim.register = vim::Register::Char(self.active_tab().buffer().slice(range.clone()).to_string());
        if op == Op::Yank {
            if let Some(state) = self.vim_in_visual().then_some(self.vim.visual).flatten() {
                self.active_tab_mut().set_cursor_position(state.anchor, None);
                self.vim_finish_normal();
            } else {
                self.active_tab_mut().move_to(start);
            }
            return true;
        }
        let cursor = char_to_position(self.active_tab().buffer(), start);
        let kind = if op == Op::Change {
            EditKind::Insert
        } else {
            EditKind::Delete
        };
        let selection_after = if op == Op::Change {
            SelectionAfter::CursorPosition(cursor)
        } else {
            SelectionAfter::CursorPositionBeforeLineEnd(cursor)
        };
        self.active_tab_mut().set_selection(Selection::collapsed(start));
        self.apply_active_edit_request(
            EditRequest::single(kind, UndoBoundary::Break, range, String::new()).with_selection_after(selection_after),
            Some(RevealIntent::NearestEdge),
        );
        self.vim_finish_normal();
        if op == Op::Change {
            self.vim.mode = vim::Mode::Insert;
        }
        true
    }

    fn vim_apply_lines_span(&mut self, op: Op, first: usize, last: usize) -> bool {
        let (first, last) = ordered_lines(first, last);
        let range = self.line_span(first, last);
        self.vim.register = vim::Register::Line(self.lines_text(first, last));
        if op == Op::Yank {
            return true;
        }
        let indent = if op == Op::Change {
            self.indent_of(first)
        } else {
            String::new()
        };
        let lines = if op == Op::Change {
            std::slice::from_ref(&indent)
        } else {
            &[]
        };
        let change = line_edit::replace_lines_change(self.active_tab(), first, last, lines).unwrap_or_else(|| {
            if op == Op::Change {
                TextChange::replace(range, indent.clone())
            } else {
                TextChange::delete(range)
            }
        });
        let cursor = Position::new(first, indent.chars().count());
        let request = if op == Op::Change {
            EditRequest::from_changes(EditKind::Insert, UndoBoundary::Break, TextChangeSet::single(change))
                .with_selection_after(SelectionAfter::CursorPosition(cursor))
        } else {
            EditRequest::single_other_at_position(change, cursor)
        };
        let start_char = self.line_start(first);
        self.active_tab_mut().set_selection(Selection::collapsed(start_char));
        self.apply_active_edit_request(request, Some(RevealIntent::NearestEdge));
        if op == Op::Change {
            self.vim.mode = vim::Mode::Insert;
            self.clear_vim_transient_state();
        }
        true
    }

    fn vim_apply_visual(&mut self, op: Op) -> bool {
        let Some(state) = self.vim.visual else { return false };
        if self.vim.mode == vim::Mode::VisualLine {
            let changed = self.vim_apply_lines_span(
                op,
                state.anchor.line.min(state.head.line),
                state.anchor.line.max(state.head.line),
            );
            if op != Op::Change {
                self.active_tab_mut().set_cursor_position(state.anchor, None);
                self.vim_finish_normal();
            }
            return changed;
        }
        self.vim_apply_range(op, self.active_tab().selected_range())
    }

    fn vim_paste(&mut self, after: bool) -> bool {
        match self.vim.register.clone() {
            vim::Register::Empty => true,
            vim::Register::Char(text) => {
                let at = if after {
                    self.vim_append_offset()
                } else {
                    self.cursor_offset()
                };
                let cursor = if text.contains('\n') {
                    0
                } else {
                    text.chars().count().saturating_sub(1)
                };
                self.apply_active_edit_request(
                    EditRequest::single(EditKind::Insert, UndoBoundary::Break, at..at, text.clone())
                        .with_selection_after(SelectionAfter::InsertedRange {
                            range: cursor..cursor,
                            reversed: false,
                        }),
                    Some(RevealIntent::NearestEdge),
                );
                true
            }
            vim::Register::Line(text) => {
                let lines: Vec<String> = text.split('\n').map(str::to_string).collect();
                let line = self.cursor().line + usize::from(after);
                if let Some(change) = line_edit::insert_lines_change(self.active_tab(), line, &lines) {
                    self.apply_active_edit_request(
                        EditRequest::single_other_at_position(change, Position::new(line, 0)),
                        Some(RevealIntent::NearestEdge),
                    );
                }
                true
            }
        }
    }

    fn vim_visual_paste(&mut self, capture_overwritten: bool) -> bool {
        match self.vim.register.clone() {
            vim::Register::Char(text) => {
                let range = self.active_tab().selected_range();
                let overwritten = self.active_tab().buffer().slice(range.clone()).to_string();
                let cursor = text.chars().count().saturating_sub(1);
                self.apply_active_edit_request(
                    EditRequest::single(EditKind::Insert, UndoBoundary::Break, range, text).with_selection_after(
                        SelectionAfter::InsertedRange {
                            range: cursor..cursor,
                            reversed: false,
                        },
                    ),
                    Some(RevealIntent::NearestEdge),
                );
                if capture_overwritten {
                    self.vim.register = vim::Register::Char(overwritten);
                }
            }
            vim::Register::Line(text) if self.vim.mode == vim::Mode::VisualLine => {
                let Some(state) = self.vim.visual else { return true };
                let first = state.anchor.line.min(state.head.line);
                let last = state.anchor.line.max(state.head.line);
                let overwritten = self.lines_text(first, last);
                let lines: Vec<String> = text.split('\n').map(str::to_string).collect();
                if let Some(change) = line_edit::replace_lines_change(self.active_tab(), first, last, &lines) {
                    self.apply_active_edit_request(
                        EditRequest::single_other_at_position(change, Position::new(first, 0)),
                        Some(RevealIntent::NearestEdge),
                    );
                    if capture_overwritten {
                        self.vim.register = vim::Register::Line(overwritten);
                    }
                }
            }
            _ => {}
        }
        self.vim_finish_normal();
        true
    }

    fn vim_insert_at(&mut self, at: usize) -> bool {
        self.move_to_char(at, false, None);
        self.vim.mode = vim::Mode::Insert;
        self.clear_vim_transient_state();
        true
    }

    fn vim_open_line(&mut self, below: bool) -> bool {
        let line = self.cursor().line;
        let indent = self.indent_of(line);
        let at = if below {
            self.line_end(line)
        } else {
            self.line_start(line)
        };
        let text = if below {
            format!("\n{indent}")
        } else {
            format!("{indent}\n")
        };
        let cursor = if below {
            Position::new(line + 1, indent.chars().count())
        } else {
            Position::new(line, indent.chars().count())
        };
        self.apply_active_edit_request(
            EditRequest::single(EditKind::Insert, UndoBoundary::Break, at..at, text)
                .with_selection_after(SelectionAfter::CursorPosition(cursor)),
            Some(RevealIntent::NearestEdge),
        );
        self.vim.mode = vim::Mode::Insert;
        self.clear_vim_transient_state();
        true
    }

    fn vim_begin_visual(&mut self, linewise: bool) -> bool {
        let pos = self.cursor();
        self.vim.mode = if linewise {
            vim::Mode::VisualLine
        } else {
            vim::Mode::Visual
        };
        self.vim.visual = Some(vim::VisualState { anchor: pos, head: pos });
        self.vim_sync_visual_selection();
        true
    }

    fn vim_toggle_visual(&mut self, linewise: bool) -> bool {
        match (linewise, self.vim.mode) {
            (false, vim::Mode::VisualLine) => self.vim.mode = vim::Mode::Visual,
            (true, vim::Mode::Visual) => self.vim.mode = vim::Mode::VisualLine,
            _ => return self.vim_begin_visual(linewise),
        }
        self.vim_sync_visual_selection();
        true
    }

    pub(crate) fn vim_extend_visual(&mut self, at: usize) {
        if let Some(mut state) = self.vim.visual {
            state.head = char_to_position(self.active_tab().buffer(), at);
            self.vim.visual = Some(state);
            self.vim_sync_visual_selection();
        }
    }

    fn vim_set_visual_range(&mut self, range: Range<usize>) {
        self.vim.mode = vim::Mode::Visual;
        let anchor = char_to_position(self.active_tab().buffer(), range.start);
        let head = char_to_position(self.active_tab().buffer(), range.end.saturating_sub(1));
        self.vim.visual = Some(vim::VisualState { anchor, head });
        self.assign_selection(Selection::from_range(range, false));
    }

    fn vim_sync_visual_selection(&mut self) {
        let Some(state) = self.vim.visual else { return };
        let buffer = self.active_tab().buffer();
        let selection = if self.vim.mode == vim::Mode::VisualLine {
            let first = state.anchor.line.min(state.head.line);
            let last = state.anchor.line.max(state.head.line);
            Selection::from_range(
                self.line_start(first)..self.line_end(last),
                state.head.line < state.anchor.line,
            )
        } else {
            let rev = (state.head.line, state.head.column) < (state.anchor.line, state.anchor.column);
            let start = position_start_char(buffer, if rev { state.head } else { state.anchor });
            let end = inclusive_position_to_exclusive_char(buffer, if rev { state.anchor } else { state.head });
            Selection::from_range(start..end, rev)
        };
        self.assign_selection(selection);
    }

    fn vim_case_visual(&mut self, upper: bool) -> bool {
        let range = self.active_tab().selected_range();
        let cursor = char_to_position(self.active_tab().buffer(), range.start);
        let text = self.active_tab().buffer().slice(range.clone()).to_string();
        let replacement = if upper {
            text.to_uppercase()
        } else {
            text.to_lowercase()
        };
        self.apply_active_edit_request(
            EditRequest::single(EditKind::Other, UndoBoundary::Break, range, replacement),
            Some(RevealIntent::NearestEdge),
        );
        self.active_tab_mut().set_cursor_position(cursor, None);
        self.vim_finish_normal();
        true
    }

    fn vim_indent_visual(&mut self, indent: bool) -> bool {
        if let Some(state) = self.vim.visual {
            let anchor = state.anchor;
            self.vim_indent_lines(
                state.anchor.line.min(state.head.line),
                state.anchor.line.max(state.head.line),
                indent,
            );
            self.active_tab_mut().set_cursor_position(anchor, None);
            self.vim_finish_normal();
        }
        true
    }

    fn vim_indent_lines(&mut self, first: usize, last: usize, indent: bool) {
        let last = last.min(self.active_tab().line_count() - 1);
        let req = if indent {
            line_edit::indent_request(self.active_tab(), first, last)
        } else {
            line_edit::outdent_request(self.active_tab(), first, last)
        };
        self.apply_optional_edit_request(req, Some(RevealIntent::NearestEdge));
    }

    fn vim_delete_counted_chars(&mut self, count: usize, forward: bool) -> bool {
        let cur = self.cursor_offset();
        let end = repeat_offset(self, cur, count, |s, off| {
            if forward {
                s.next_grapheme(off)
            } else {
                s.prev_grapheme(off)
            }
        });
        let range = end.min(cur)..end.max(cur);
        self.vim_apply_range(Op::Delete, range)
    }

    fn vim_substitute(&mut self, count: usize) -> bool {
        let cur = self.cursor_offset();
        let end = repeat_offset(self, cur, count, |s, off| s.next_grapheme(off));
        self.vim_apply_range(Op::Change, cur..end)
    }

    fn vim_replace_chars(&mut self, count: usize, ch: char) -> bool {
        let cur = self.cursor_offset();
        let mut end = cur;
        let mut replaced = 0usize;
        for _ in 0..count.max(1) {
            let next = self.next_grapheme(end);
            if next == end {
                break;
            }
            end = next;
            replaced += 1;
        }
        if cur == end {
            return true;
        }
        if replaced < count.max(1) {
            return true;
        }
        let replacement = ch.to_string().repeat(replaced);
        self.apply_active_edit_request(
            EditRequest::single(EditKind::Other, UndoBoundary::Break, cur..end, replacement).with_selection_after(
                SelectionAfter::CursorPosition(char_to_position(self.active_tab().buffer(), end.saturating_sub(1))),
            ),
            Some(RevealIntent::NearestEdge),
        );
        true
    }

    fn vim_join(&mut self, count: usize) -> bool {
        let first = self.cursor().line;
        let mut last = (first + count.max(2) - 1).min(self.active_tab().line_count() - 1);
        if first >= last {
            return true;
        }
        while last > first
            && self.line_start(last) == self.active_tab().len_chars()
            && line_display_text(self.active_tab().buffer(), last).is_empty()
        {
            last -= 1;
        }
        let joined = (first..=last)
            .map(|line| line_display_text(self.active_tab().buffer(), line).trim().to_string())
            .collect::<Vec<_>>()
            .join(" ")
            .trim_end()
            .to_string();
        let end = if last + 1 < self.active_tab().line_count()
            && self.line_start(last + 1) == self.active_tab().len_chars()
        {
            self.active_tab().len_chars()
        } else {
            self.line_end(last)
        };
        let first_len = line_display_text(self.active_tab().buffer(), first)
            .trim_end()
            .chars()
            .count();
        let cursor_col = if count <= 1 {
            first_len
        } else {
            first_len + (last - first)
        }
        .min(joined.chars().count().saturating_sub(1));
        let change = TextChange::replace(self.line_start(first)..end, joined);
        self.apply_active_edit_request(
            EditRequest::single_other_at_position(change, Position::new(first, cursor_col)),
            Some(RevealIntent::NearestEdge),
        );
        true
    }

    fn vim_open_search(&mut self, backward: bool) -> bool {
        self.find_submit = crate::FindSubmit::Vim { backward };
        self.vim.search_backward = backward;
        self.find.visible = true;
        self.find.show_replace = false;
        self.queue_focus(FocusTarget::FindQuery);
        true
    }

    fn vim_search_step(&mut self, same_direction: bool) -> bool {
        let backward = if same_direction {
            self.vim.search_backward
        } else {
            !self.vim.search_backward
        };
        if backward {
            self.find_step(false)
        } else {
            self.find_step(true)
        };
        true
    }

    fn vim_word_search(&mut self, forward: bool) -> bool {
        let range = selection::word_range_at_char(self.active_tab().buffer(), self.cursor_offset());
        if range.start == range.end {
            return true;
        }
        let word = self.active_tab().buffer().slice(range).to_string();
        let tab = self.active_tab().clone();
        if let Some(pos) = self.find.search_word_from(&tab, word, self.cursor(), forward) {
            self.vim.search_backward = !forward;
            self.active_tab_mut().set_cursor_position(pos, None);
            self.queue_reveal(RevealIntent::Center);
        }
        true
    }

    fn vim_text_object(&self, spec: &str, count: usize) -> Option<Range<usize>> {
        let mut chars = spec.chars();
        let kind = chars.next()?;
        if !matches!(kind, 'i' | 'a') {
            return None;
        }
        let obj = chars.next()?;
        if chars.next().is_some() {
            return None;
        }
        match obj {
            'w' | 'W' => self.word_object(obj == 'W', kind == 'a', count),
            'p' => Some(self.paragraph_object(kind == 'a')),
            _ => self.pair_object(obj, kind == 'a'),
        }
    }

    fn word_object(&self, big: bool, around: bool, count: usize) -> Option<Range<usize>> {
        let mut object = self.word_object_at(self.cursor(), !around, big)?;
        for _ in 1..count.max(1) {
            let Some(next_cursor) = self.advance_position(object.end) else {
                break;
            };
            let Some(next_object) = self.word_object_at(next_cursor, !around, big) else {
                break;
            };
            object.end = next_object.end;
        }
        let start = position_to_char(self.active_tab().buffer(), object.start);
        let end = inclusive_position_to_exclusive_char(self.active_tab().buffer(), object.end);
        Some(start..end.max(start))
    }

    fn word_object_at(&self, cursor: Position, inner: bool, big: bool) -> Option<PositionRange> {
        let tokens = TokenLine(cursor.line, self.line_cells(cursor.line), big);
        if tokens.1.is_empty() {
            return None;
        }
        let col = cursor.column.min(self.line_len(cursor.line).saturating_sub(1));
        let run = tokens.run(cell_containing_char(&tokens.1, col));
        let (start, end) = if inner {
            (run.start, run.end)
        } else if run.class == TokenClass::Whitespace {
            if let Some(next) = tokens.non_ws_run(run.end + 1, true) {
                (run.start, next.end)
            } else if let Some(prev) = tokens.non_ws_run(run.start, false) {
                (prev.start, run.end)
            } else {
                (run.start, run.end)
            }
        } else {
            let (mut start, mut end) = (run.start, run.end);
            if end + 1 < tokens.1.len() && vim_token_class(tokens.1[end + 1].repr, big) == TokenClass::Whitespace {
                end = tokens.run(end + 1).end;
            } else if start > 0 && vim_token_class(tokens.1[start - 1].repr, big) == TokenClass::Whitespace {
                start = tokens.run(start - 1).start;
            }
            (start, end)
        };
        let (start, end) = tokens.positions(start, end);
        Some(PositionRange { start, end })
    }

    fn paragraph_object(&self, around: bool) -> Range<usize> {
        let mut range = paragraph_range_at_char(self.active_tab().buffer(), self.cursor_offset());
        if around {
            let len = self.active_tab().len_chars();
            while range.end < len && self.active_tab().buffer().char(range.end).is_whitespace() {
                range.end += 1;
                if range.end < len && self.active_tab().buffer().char(range.end - 1) == '\n' {
                    break;
                }
            }
        }
        range
    }

    fn pair_object(&self, obj: char, around: bool) -> Option<Range<usize>> {
        let (open, close, quote) = pair_for(obj)?;
        let text = self.active_tab().buffer_text();
        let chars: Vec<char> = text.chars().collect();
        let cur = self.cursor_offset().min(chars.len());
        let open_ix = if quote {
            let quote_count = (0..cur)
                .filter(|&i| chars.get(i) == Some(&open) && (i == 0 || chars[i - 1] != '\\'))
                .count();
            let end = if chars.get(cur) == Some(&open) && quote_count % 2 == 1 {
                cur.saturating_sub(1)
            } else {
                cur
            };
            (0..=end)
                .rev()
                .find(|&i| chars.get(i) == Some(&open) && (i == 0 || chars[i - 1] != '\\'))?
        } else {
            find_pair(&chars, cur, open, close, -1, false)?
        };
        let close_ix = find_pair(&chars, open_ix + 1, open, close, 1, quote)?;
        let mut range = if around {
            open_ix..close_ix + 1
        } else {
            open_ix + 1..close_ix
        };
        if around && quote {
            while range.end < chars.len() && chars[range.end].is_whitespace() {
                range.end += 1;
            }
        }
        Some(range)
    }

    fn vim_char_motion(&mut self, count: usize, cmd: char, target: char, operator: bool) -> Option<Motion> {
        let find = vim::CharFind {
            target,
            forward: matches!(cmd, 'f' | 't'),
            till: matches!(cmd, 't' | 'T'),
        };
        self.vim.char_find = Some(find);
        self.char_find_motion(count, find, operator)
    }

    fn repeat_char_find(&mut self, same: bool, operator: bool) -> Option<Motion> {
        let mut find = self.vim.char_find?;
        if !same {
            find.forward = !find.forward;
        }
        self.char_find_motion(1, find, operator)
    }

    fn char_find_motion(&self, count: usize, find: vim::CharFind, operator: bool) -> Option<Motion> {
        let line = self.cursor().line;
        let body = line_display_text(self.active_tab().buffer(), line);
        let cur_col = self.cursor().column;
        let hits: Vec<_> = body
            .chars()
            .enumerate()
            .filter_map(|(i, ch)| (ch == find.target).then_some(i))
            .collect();
        let target = if find.forward {
            hits.into_iter().filter(|i| *i > cur_col).nth(count - 1)?
        } else {
            hits.into_iter().rev().filter(|i| *i < cur_col).nth(count - 1)?
        };
        let found = match (find.forward, find.till, operator) {
            (true, true, false) => target.saturating_sub(1),
            (false, true, _) => target + 1,
            _ => target,
        };
        Some(Motion {
            at: char_at_line_column(self.active_tab().buffer(), line, found),
            linewise: false,
            inclusive: !find.till,
        })
    }

    fn vertical(&mut self, delta: isize, operator: bool) -> Motion {
        let pos = self.cursor();
        let last = self.active_tab().line_count().saturating_sub(1);
        let line = if delta < 0 {
            pos.line.saturating_sub(delta.unsigned_abs())
        } else {
            (pos.line + delta as usize).min(last)
        };
        let goal = self.active_tab().preferred_column().unwrap_or(pos.column);
        let at = self.line_col_offset(line, goal);
        self.active_tab_mut().set_preferred_column(Some(goal));
        Motion {
            at,
            linewise: operator,
            inclusive: false,
        }
    }

    fn vim_collapse_restored_selection(&mut self) {
        if self.active_tab().has_selection() {
            let start = self.active_tab().selected_range().start;
            self.active_tab_mut().move_to(start);
        }
        self.vim_finish_normal();
    }

    fn screen_motion(&mut self, row: usize) -> Motion {
        let at = self.line_col_offset(row.min(self.active_tab().line_count() - 1), self.cursor().column);
        self.queue_reveal(RevealIntent::NearestEdge);
        Motion {
            at,
            linewise: false,
            inclusive: false,
        }
    }

    fn percent_or_match(&self, count: usize) -> usize {
        if count > 1 {
            let line = ((self.active_tab().line_count() * count).saturating_sub(1) / 100)
                .min(self.active_tab().line_count() - 1);
            return self.line_col_offset(line, self.cursor().column);
        }
        let text = self.active_tab().buffer_text();
        let chars: Vec<char> = text.chars().collect();
        let cur = self.cursor_offset();
        let Some(ch) = chars.get(cur).copied() else { return cur };
        let Some((open, close, dir)) = bracket_dir(ch) else {
            return cur;
        };
        find_pair(&chars, if dir > 0 { cur + 1 } else { cur }, open, close, dir, false).unwrap_or(cur)
    }

    fn vim_move_to(&mut self, at: usize) {
        self.move_to_char(at, false, self.active_tab().preferred_column());
    }

    fn vim_finish_normal(&mut self) {
        self.vim.mode = vim::Mode::Normal;
        self.clear_vim_transient_state();
    }

    pub(crate) fn vim_in_visual(&self) -> bool {
        matches!(self.vim.mode, vim::Mode::Visual | vim::Mode::VisualLine)
    }

    fn clear_vim_transient_state(&mut self) {
        self.vim.clear_transient();
    }

    pub(crate) fn move_to_vim_search_target(&mut self, target: Position) {
        if self.vim_in_visual() {
            self.vim_extend_visual(position_to_char(self.active_tab().buffer(), target));
        } else {
            self.active_tab_mut().set_cursor_position(target, None);
        }
    }

    fn vim_normalize_insert_cursor(&mut self) {
        let cursor = self.cursor_offset();
        if cursor > self.line_start(self.cursor().line) {
            self.move_to_char(self.prev_grapheme(cursor), false, None);
        }
    }

    fn cursor(&self) -> Position {
        self.visual_head()
            .unwrap_or_else(|| self.active_tab().cursor_position())
    }
    fn cursor_offset(&self) -> usize {
        self.visual_head().map_or_else(
            || self.active_tab().cursor_char(),
            |head| position_to_char(self.active_tab().buffer(), head),
        )
    }
    fn visual_head(&self) -> Option<Position> {
        self.vim_in_visual()
            .then_some(self.vim.visual)
            .flatten()
            .map(|state| state.head)
    }
    fn line_start(&self, line: usize) -> usize {
        self.active_tab()
            .buffer()
            .line_to_char(line.min(self.active_tab().line_count() - 1))
    }
    fn line_end(&self, line: usize) -> usize {
        self.line_start(line) + selection::display_line_char_len(self.active_tab().buffer(), line)
    }
    fn line_last(&self, line: usize) -> usize {
        let end = self.line_end(line);
        if end == self.line_start(line) {
            end
        } else {
            self.prev_grapheme(end)
        }
    }
    fn line_first_nonblank(&self, line: usize) -> usize {
        let body = line_display_text(self.active_tab().buffer(), line);
        self.line_start(line) + body.chars().take_while(|c| c.is_whitespace()).count()
    }
    fn line_col_offset(&self, line: usize, col: usize) -> usize {
        let line = line.min(self.active_tab().line_count() - 1);
        let len = selection::display_line_char_len(self.active_tab().buffer(), line);
        char_at_line_column(
            self.active_tab().buffer(),
            line,
            if len == 0 { 0 } else { col.min(len - 1) },
        )
    }
    fn line_span(&self, first: usize, last: usize) -> Range<usize> {
        let first = first.min(self.active_tab().line_count() - 1);
        let last = last.min(self.active_tab().line_count() - 1);
        crate::linewise_range_at_char(self.active_tab().buffer(), self.line_start(first)).start
            ..crate::linewise_range_at_char(self.active_tab().buffer(), self.line_start(last)).end
    }
    fn lines_text(&self, first: usize, last: usize) -> String {
        let first = first.min(self.active_tab().line_count() - 1);
        let last = last.min(self.active_tab().line_count() - 1);
        (first..=last)
            .map(|line| line_display_text(self.active_tab().buffer(), line))
            .collect::<Vec<_>>()
            .join("\n")
    }
    fn indent_of(&self, line: usize) -> String {
        line_display_text(self.active_tab().buffer(), line)
            .chars()
            .take_while(|c| c.is_whitespace())
            .collect()
    }
    fn next_grapheme(&self, at: usize) -> usize {
        selection::next_grapheme_boundary(self.active_tab().buffer(), at)
    }
    fn prev_grapheme(&self, at: usize) -> usize {
        selection::previous_grapheme_boundary(self.active_tab().buffer(), at)
    }
    fn next_normal_grapheme(&self, at: usize) -> usize {
        self.next_grapheme(at).min(self.line_last(self.cursor().line))
    }
    fn vim_append_offset(&self) -> usize {
        let line_end = self.line_end(self.cursor().line);
        match self.cursor_offset() < line_end {
            true => self.next_grapheme(self.cursor_offset()),
            false => self.cursor_offset(),
        }
    }

    fn word_forward(&self, at: usize, big: bool) -> usize {
        let pos = char_to_position(self.active_tab().buffer(), at);
        let (line, col) = self.word_forward_position(pos.line, pos.column, big);
        char_at_line_column(self.active_tab().buffer(), line, col)
    }
    fn word_end(&self, at: usize, big: bool) -> usize {
        let pos = char_to_position(self.active_tab().buffer(), at);
        let (line, col) = self.word_end_position(pos.line, pos.column, big);
        char_at_line_column(self.active_tab().buffer(), line, col)
    }
    fn word_backward(&self, at: usize, big: bool) -> usize {
        let pos = char_to_position(self.active_tab().buffer(), at);
        let (line, col) = self.word_backward_position(pos.line, pos.column, big);
        char_at_line_column(self.active_tab().buffer(), line, col)
    }

    fn word_forward_position(&self, mut line: usize, col: usize, big: bool) -> (usize, usize) {
        let mut tokens = TokenLine(line, self.line_cells(line), big);
        if tokens.1.is_empty() {
            if line + 1 < self.active_tab().line_count() {
                return (line + 1, 0);
            }
            return (line, 0);
        }
        let mut ix = if col >= self.line_len(line) {
            tokens.1.len()
        } else {
            cell_containing_char(&tokens.1, col)
        };
        if ix < tokens.1.len() && vim_token_class(tokens.1[ix].repr, big) != TokenClass::Whitespace {
            ix = tokens.run(ix).end + 1;
        }
        loop {
            if let Some(run) = tokens.non_ws_run(ix, true) {
                return (tokens.0, tokens.1[run.start].char_start);
            }
            if line + 1 < self.active_tab().line_count() {
                line += 1;
                tokens = TokenLine(line, self.line_cells(line), big);
                ix = 0;
                if tokens.1.is_empty() {
                    return (line, 0);
                }
            } else {
                return (line, self.line_len(line));
            }
        }
    }

    fn word_backward_position(&self, mut line: usize, col: usize, big: bool) -> (usize, usize) {
        let mut tokens = TokenLine(line, self.line_cells(line), big);
        let mut ix = if tokens.1.is_empty() {
            None
        } else {
            cell_partition_by_char(&tokens.1, col).checked_sub(1)
        };
        if ix.is_none() {
            if line == 0 {
                return (0, 0);
            }
            line -= 1;
            tokens = TokenLine(line, self.line_cells(line), big);
            if tokens.1.is_empty() {
                return (line, 0);
            }
            ix = Some(tokens.1.len() - 1);
        }
        let mut ix = ix.unwrap();
        loop {
            if let Some(run) = tokens.non_ws_run(ix + 1, false) {
                return (tokens.0, tokens.1[run.start].char_start);
            }
            if line == 0 {
                return (0, 0);
            }
            line -= 1;
            tokens = TokenLine(line, self.line_cells(line), big);
            if tokens.1.is_empty() {
                return (line, 0);
            }
            ix = tokens.1.len() - 1;
        }
    }

    fn word_end_position(&self, mut line: usize, col: usize, big: bool) -> (usize, usize) {
        let mut tokens = TokenLine(line, self.line_cells(line), big);
        let mut ix = if tokens.1.is_empty() {
            if line + 1 < self.active_tab().line_count() {
                line += 1;
                tokens = TokenLine(line, self.line_cells(line), big);
                0
            } else {
                return (line, 0);
            }
        } else {
            let containing = cell_containing_char(&tokens.1, col);
            if containing + 1 < tokens.1.len() {
                containing + 1
            } else if line + 1 < self.active_tab().line_count() {
                line += 1;
                tokens = TokenLine(line, self.line_cells(line), big);
                0
            } else {
                return (line, self.last_cluster_col(line));
            }
        };
        loop {
            if let Some(run) = tokens.non_ws_run(ix, true) {
                return (tokens.0, tokens.1[run.end].char_start);
            }
            if line + 1 < self.active_tab().line_count() {
                line += 1;
                tokens = TokenLine(line, self.line_cells(line), big);
                ix = 0;
            } else {
                return (line, self.last_cluster_col(line));
            }
        }
    }

    fn line_len(&self, line: usize) -> usize {
        if line >= self.active_tab().line_count() {
            return 0;
        }
        selection::display_line_char_len(self.active_tab().buffer(), line)
    }

    fn line_cells(&self, line: usize) -> Vec<GraphemeCell> {
        cells_of_str(&line_display_text(self.active_tab().buffer(), line))
    }

    fn last_cluster_col(&self, line: usize) -> usize {
        selection::last_grapheme_column(&line_display_text(self.active_tab().buffer(), line))
    }

    fn advance_position(&self, p: Position) -> Option<Position> {
        let line_len = self.line_len(p.line);
        if p.column + 1 < line_len {
            Some(Position::new(p.line, p.column + 1))
        } else if p.line + 1 < self.active_tab().line_count() {
            Some(Position::new(p.line + 1, 0))
        } else {
            None
        }
    }
}

fn count_prefix(s: &str) -> (usize, &str) {
    let bytes = s.as_bytes();
    let mut ix = 0;
    while ix < bytes.len() && bytes[ix].is_ascii_digit() && !(ix == 0 && bytes[ix] == b'0') {
        ix += 1;
    }
    (s[..ix].parse().unwrap_or(1), &s[ix..])
}
fn is_pending_prefix(rest: &str) -> bool {
    matches!(
        rest,
        "d" | "c" | "y" | "g" | "z" | "r" | "f" | "F" | "t" | "T" | ">" | "<" | "i" | "a"
    ) || rest.chars().last().is_some_and(|c| c.is_ascii_digit())
}
fn repeat_offset(
    model: &EditorModel,
    mut at: usize,
    count: usize,
    mut f: impl FnMut(&EditorModel, usize) -> usize,
) -> usize {
    for _ in 0..count.max(1) {
        let next = f(model, at);
        if next == at {
            break;
        }
        at = next;
    }
    at
}

struct TokenRun {
    start: usize,
    end: usize,
    class: TokenClass,
}

struct TokenLine(usize, Vec<GraphemeCell>, bool);

impl TokenLine {
    fn run(&self, ix: usize) -> TokenRun {
        let class = vim_token_class(self.1[ix].repr, self.2);
        let token_class = |i: usize| vim_token_class(self.1[i].repr, self.2);
        let start = (0..ix).rev().find(|&i| token_class(i) != class).map_or(0, |i| i + 1);
        let end = (ix + 1..self.1.len())
            .find(|&i| token_class(i) != class)
            .map_or(self.1.len() - 1, |i| i - 1);
        TokenRun { start, end, class }
    }

    fn non_ws_run(&self, start: usize, forward: bool) -> Option<TokenRun> {
        let class = |i: usize| vim_token_class(self.1[i].repr, self.2);
        let ix = if forward {
            (start..self.1.len()).find(|&i| class(i) != TokenClass::Whitespace)?
        } else {
            (0..start).rev().find(|&i| class(i) != TokenClass::Whitespace)?
        };
        Some(self.run(ix))
    }

    fn positions(&self, start: usize, end: usize) -> (Position, Position) {
        (
            Position::new(self.0, self.1[start].char_start),
            Position::new(self.0, self.1[end].char_start),
        )
    }
}

fn ordered_lines(a: usize, b: usize) -> (usize, usize) {
    (a.min(b), a.max(b))
}
fn pair_for(obj: char) -> Option<(char, char, bool)> {
    let pair = match obj {
        '(' | ')' | 'b' => ('(', ')'),
        '[' | ']' => ('[', ']'),
        '{' | '}' | 'B' => ('{', '}'),
        '<' | '>' => ('<', '>'),
        '"' | '\'' | '`' => (obj, obj),
        _ => return None,
    };
    Some((pair.0, pair.1, pair.0 == pair.1))
}
fn find_pair(chars: &[char], start: usize, open: char, close: char, dir: isize, quote: bool) -> Option<usize> {
    let mut depth = 0usize;
    let range: Box<dyn Iterator<Item = usize>> = if dir > 0 {
        Box::new(start..chars.len())
    } else {
        Box::new((0..=start.min(chars.len().saturating_sub(1))).rev())
    };
    for i in range {
        if quote && chars[i] == close && (i == 0 || chars[i - 1] != '\\') {
            return Some(i);
        }
        if chars[i] == if dir > 0 { open } else { close } {
            depth += 1;
        } else if chars[i] == if dir > 0 { close } else { open } {
            if dir > 0 {
                if depth == 0 {
                    return Some(i);
                }
                depth -= 1;
            } else {
                if depth <= 1 {
                    return Some(i);
                }
                depth -= 1;
            }
        }
    }
    None
}
fn bracket_dir(ch: char) -> Option<(char, char, isize)> {
    let key = [('(', ')'), ('[', ']'), ('{', '}')]
        .into_iter()
        .find(|(open, close)| ch == *open || ch == *close)?;
    Some((key.0, key.1, if ch == key.0 { 1 } else { -1 }))
}
