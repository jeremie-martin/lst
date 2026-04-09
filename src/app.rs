#[cfg(test)]
use crate::clipboard::NullClipboard;
use crate::clipboard::{Clipboard, RealClipboard};
use crate::editor_ops;
use crate::find::FindState;
#[cfg(test)]
use crate::fs::NullFilesystem;
use crate::fs::{Filesystem, RealFilesystem};
use crate::highlight;
use crate::style::{flat_btn, solid_bg, EDITOR_FONT, EDITOR_PAD, FONT_SIZE, LINE_HEIGHT_PX};
use crate::tab::{EditKind, Tab};
use crate::viewport::{self, RevealIntent, ViewportState};
use crate::vim;

use iced::event;
use iced::keyboard;
use iced::widget::{
    button, column, container, mouse_area, opaque, responsive, right, row, scrollable,
    scrollable::Viewport, stack, text, text_editor, text_input, Space,
};
use iced::{
    Background, Border, Color, Element, Length, Padding, Pixels, Point, Subscription, Task, Theme,
};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};
use std::time::Duration;
#[cfg(test)]
use std::time::Instant;

fn editor_id() -> iced::widget::Id {
    iced::widget::Id::new("lst-editor")
}

static EDITOR_ID: LazyLock<iced::widget::Id> = LazyLock::new(editor_id);
static SCROLLABLE_ID: LazyLock<iced::widget::Id> =
    LazyLock::new(|| iced::widget::Id::new("lst-scroll"));
static FIND_INPUT_ID: LazyLock<iced::widget::Id> =
    LazyLock::new(|| iced::widget::Id::new("lst-find"));
static GOTO_LINE_ID: LazyLock<iced::widget::Id> =
    LazyLock::new(|| iced::widget::Id::new("lst-goto-line"));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorPointerCell {
    pub column: usize,
    pub row: usize,
}

// ── ViewSnapshot (test-only) ─────────────────────────────────────────────────

/// All user-visible state at a point in time.
/// Tests assert against this instead of reaching into App fields.
#[derive(Debug)]
pub struct ViewSnapshot {
    // Document
    pub text: String,
    pub cursor_line: usize,
    pub cursor_column: usize,
    pub selection: Option<String>,
    // Tabs
    pub tab_count: usize,
    pub active_tab: usize,
    pub tab_titles: Vec<String>,
    pub title: String,
    pub modified: bool,
    // Find / replace
    pub find_visible: bool,
    pub find_replace_visible: bool,
    pub find_query: String,
    pub find_replacement: String,
    pub find_match_count: usize,
    pub find_current_match: usize,
    // Go-to-line
    pub goto_line_visible: bool,
    pub goto_line_text: String,
    // Editor state
    pub word_wrap: bool,
    pub vim_mode: String,
    pub vim_pending: String,
}

// ── App ──────────────────────────────────────────────────────────────────────

pub struct App {
    pub tabs: Vec<Tab>,
    pub active: usize,
    pub window_title: Option<String>,
    pub gutter_hover_line: Option<usize>,
    pub find: FindState,
    pub word_wrap: bool,
    pub scratchpad_dir: PathBuf,
    pub needs_autosave: bool,
    pub shift_held: bool, // iced's Action::Click doesn't carry modifier state; track externally
    pub multiclick_drag: bool, // Workaround for iced-rs/iced#3227 — remove when merged
    pub editor_pointer_cell: Option<EditorPointerCell>,
    pub goto_line: Option<String>,
    pub vim: vim::VimState,
    pub viewport: ViewportState,
    pub clipboard: Box<dyn Clipboard>,
    pub fs: Box<dyn Filesystem>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Edit(text_editor::Action),
    TabSelect(usize),
    TabClose(usize),
    CloseActiveTab,
    New,
    Open,
    Opened(Result<(PathBuf, String), Error>),
    Save,
    SaveAs,
    Saved(Result<PathBuf, Error>),
    AutosaveTick,
    AutosaveComplete(Result<PathBuf, Error>),
    GutterMove(Point),
    GutterClick,
    Quit,
    // Undo / redo
    Undo,
    Redo,
    AutoIndent,
    // Find & replace
    FindOpen,
    FindOpenReplace,
    FindClose,
    FindQueryChanged(String),
    FindReplaceChanged(String),
    FindNext,
    FindPrev,
    FindRefreshTick,
    ReplaceOne,
    ReplaceAll,
    // Word wrap
    ToggleWordWrap,
    // Tab reorder
    MoveTabLeft,
    MoveTabRight,
    // Line operations
    DeleteLine,
    MoveLineUp,
    MoveLineDown,
    DuplicateLine,
    ToggleComment,
    // Page movement
    PageUp(usize, bool),
    PageDown(usize, bool),
    // Tab cycling
    NextTab,
    PrevTab,
    // Go to line
    GotoLineOpen,
    GotoLineClose,
    GotoLineChanged(String),
    GotoLineSubmit,
    // Modifier tracking (for Shift+Click)
    ModifiersChanged(keyboard::Modifiers),
    // Vim
    VimKey(keyboard::Key, keyboard::Modifiers),
    // Workaround for iced-rs/iced#3227 — remove when merged
    EditorMouseMove(Point),
    MulticlickReleased,
    MiddleClickPaste,
    // Scroll tracking
    Scrolled(Viewport),
}

#[derive(Debug, Clone)]
pub enum Error {
    DialogClosed,
    Io,
}

pub struct UpdateResult {
    pub task: Task<Message>,
    pub reveal: RevealIntent,
}

impl UpdateResult {
    pub fn none() -> Self {
        Self {
            task: Task::none(),
            reveal: RevealIntent::None,
        }
    }

    pub fn task(task: Task<Message>) -> Self {
        Self {
            task,
            reveal: RevealIntent::None,
        }
    }

    pub fn reveal(task: Task<Message>) -> Self {
        Self {
            task,
            reveal: RevealIntent::RevealCaret,
        }
    }
}

struct CliArgs {
    window_title: Option<String>,
    files: Vec<PathBuf>,
    scratchpad_dir: Option<PathBuf>,
}

fn parse_args() -> CliArgs {
    let mut args = std::env::args().skip(1);
    let mut window_title = None;
    let mut files = Vec::new();
    let mut scratchpad_dir = None;

    while let Some(arg) = args.next() {
        if let Some(value) = arg.strip_prefix("--title=") {
            window_title = Some(value.to_owned());
            continue;
        }
        if arg == "--title" {
            let Some(value) = args.next() else {
                eprintln!("lst: missing value for --title");
                std::process::exit(2);
            };
            window_title = Some(value);
            continue;
        }
        if let Some(value) = arg.strip_prefix("--scratchpad-dir=") {
            scratchpad_dir = Some(PathBuf::from(value));
            continue;
        }
        if arg == "--scratchpad-dir" {
            let Some(value) = args.next() else {
                eprintln!("lst: missing value for --scratchpad-dir");
                std::process::exit(2);
            };
            scratchpad_dir = Some(PathBuf::from(value));
            continue;
        }
        files.push(PathBuf::from(arg));
    }

    CliArgs {
        window_title,
        files,
        scratchpad_dir,
    }
}

fn resolve_scratchpad_dir(cli_override: Option<PathBuf>, fs: &dyn Filesystem) -> PathBuf {
    let dir = cli_override.unwrap_or_else(|| {
        let Some(home) = std::env::var_os("HOME") else {
            eprintln!("lst: HOME environment variable not set");
            std::process::exit(1);
        };
        PathBuf::from(home).join(".local/share/lst")
    });
    if let Err(e) = fs.create_dir_all(&dir) {
        eprintln!(
            "lst: failed to create scratchpad directory {}: {e}",
            dir.display()
        );
        std::process::exit(1);
    }
    dir
}

fn generate_scratchpad_path(dir: &Path, fs: &dyn Filesystem) -> PathBuf {
    use chrono::Local;
    let name = Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
    let path = dir.join(format!("{name}.md"));
    if !fs.exists(&path) {
        return path;
    }
    for i in 1.. {
        let path = dir.join(format!("{name}_{i}.md"));
        if !fs.exists(&path) {
            return path;
        }
    }
    unreachable!()
}

impl App {
    pub fn boot() -> (Self, Task<Message>) {
        let args = parse_args();
        let fs: Box<dyn Filesystem> = Box::new(RealFilesystem);
        let scratchpad_dir = resolve_scratchpad_dir(args.scratchpad_dir, &*fs);

        let mut tabs: Vec<Tab> = args
            .files
            .into_iter()
            .filter_map(|path| {
                let body = fs.read_to_string(&path).ok()?;
                let canonical = fs.canonicalize(&path).unwrap_or(path);
                Some(Tab::from_path(canonical, &body))
            })
            .collect();

        if tabs.is_empty() {
            let path = generate_scratchpad_path(&scratchpad_dir, &*fs);
            tabs.push(Tab::new_scratchpad(path));
        }

        (
            Self {
                tabs,
                active: 0,
                window_title: args.window_title,
                gutter_hover_line: None,
                find: FindState::new(),
                word_wrap: true,
                scratchpad_dir,
                needs_autosave: false,
                shift_held: false,
                multiclick_drag: false,
                editor_pointer_cell: None,
                goto_line: None,
                vim: vim::VimState::new(),
                viewport: ViewportState::default(),
                clipboard: Box::new(RealClipboard),
                fs,
            },
            iced::widget::operation::focus(EDITOR_ID.clone()),
        )
    }

    fn create_scratchpad_tab(&self) -> Tab {
        let path = generate_scratchpad_path(&self.scratchpad_dir, &*self.fs);
        Tab::new_scratchpad(path)
    }

    pub fn test(text: &str) -> Self {
        use crate::clipboard::NullClipboard;
        use crate::fs::NullFilesystem;
        Self {
            tabs: vec![Tab::from_path(PathBuf::from("/tmp/test.txt"), text)],
            active: 0,
            window_title: None,
            gutter_hover_line: None,
            find: FindState::new(),
            word_wrap: true,
            scratchpad_dir: PathBuf::from("/tmp"),
            needs_autosave: false,
            shift_held: false,
            multiclick_drag: false,
            editor_pointer_cell: None,
            goto_line: None,
            vim: vim::VimState::new(),
            viewport: ViewportState::default(),
            clipboard: Box::new(NullClipboard),
            fs: Box::new(NullFilesystem),
        }
    }

    /// Returns a snapshot of all user-visible application state.
    /// This is the single observation point for black-box tests —
    /// assertions go through this, never through struct fields.
    pub fn snapshot(&self) -> ViewSnapshot {
        let tab = &self.tabs[self.active];
        let cursor = tab.content.cursor().position;
        ViewSnapshot {
            text: tab.content.text(),
            cursor_line: cursor.line,
            cursor_column: cursor.column,
            selection: selection_text(&tab.content, self.vim.mode),
            tab_count: self.tabs.len(),
            active_tab: self.active,
            tab_titles: self
                .tabs
                .iter()
                .map(|t| t.display_name().into_owned())
                .collect(),
            title: self.title(),
            modified: tab.modified,
            find_visible: self.find.visible,
            find_replace_visible: self.find.visible && self.find.show_replace,
            find_query: self.find.query.clone(),
            find_replacement: self.find.replacement.clone(),
            find_match_count: self.find.matches.len(),
            find_current_match: self.find.current,
            goto_line_visible: self.goto_line.is_some(),
            goto_line_text: self.goto_line.clone().unwrap_or_default(),
            word_wrap: self.word_wrap,
            vim_mode: self.vim.mode.label().to_string(),
            vim_pending: self.vim.pending_display(),
        }
    }

    pub fn title(&self) -> String {
        if let Some(title) = &self.window_title {
            return title.clone();
        }
        let tab = &self.tabs[self.active];
        match &tab.path {
            Some(p) => format!("{} \u{2014} lst", p.display()),
            None => format!("{} \u{2014} lst", tab.display_name()),
        }
    }

    pub fn theme(&self) -> Theme {
        Theme::CatppuccinMocha
    }

    fn close_tab(&mut self, i: usize) -> Task<Message> {
        if i >= self.tabs.len() {
            return Task::none();
        }
        let closed_active = i == self.active;
        let tab = &self.tabs[i];
        if tab.is_scratchpad && tab.content.text().trim().is_empty() {
            if let Some(p) = &tab.path {
                let _ = self.fs.remove_file(p);
            }
        }
        if self.tabs.len() == 1 {
            return self.exit_with_clipboard();
        }
        self.tabs.remove(i);
        if closed_active {
            let new_active = self.active.min(self.tabs.len() - 1);
            self.set_active_tab(new_active);
        } else if self.active > i {
            self.active -= 1;
        }
        Task::none()
    }

    fn exit_with_clipboard(&self) -> Task<Message> {
        let text = self.tabs[self.active].content.text();
        if !text.trim().is_empty() {
            self.clipboard.copy(&text);
        }
        iced::exit()
    }

    fn active_tab_revision(&self) -> u64 {
        self.tabs[self.active].revision()
    }

    fn active_wrap_cols(&self) -> Option<usize> {
        self.word_wrap
            .then(|| {
                wrapped_cols(
                    self.viewport.width(),
                    self.tabs[self.active].content.line_count().max(1),
                )
            })
            .flatten()
    }

    fn ensure_active_layout_cache(&mut self) {
        let Some(wrap_cols) = self.active_wrap_cols() else {
            return;
        };

        self.tabs[self.active].ensure_layout_cache(wrap_cols);
    }

    fn replace_active_lines(&mut self, lines: Vec<String>, cursor_line: usize, cursor_col: usize) {
        self.rebuild_content(&lines.join("\n"), cursor_line, cursor_col);
    }

    fn move_active_cursor(&mut self, cursor_line: usize, cursor_col: usize) {
        let tab = &mut self.tabs[self.active];
        let line = cursor_line.min(tab.content.line_count().saturating_sub(1));
        tab.content.move_to(text_editor::Cursor {
            position: text_editor::Position {
                line,
                column: cursor_col,
            },
            selection: None,
        });
        self.vim.clear_preferred_column();
    }

    fn apply_line_edit<R, F>(&mut self, edit: F) -> Option<R>
    where
        F: FnOnce(&mut Vec<String>) -> Option<(R, usize, usize)>,
    {
        let cached_lines = self.tabs[self.active].lines();
        let mut lines: Vec<String> = cached_lines.iter().cloned().collect();
        let (result, cursor_line, cursor_col) = edit(&mut lines)?;
        if lines.as_slice() == cached_lines.as_ref() {
            let cursor = self.tabs[self.active].content.cursor().position;
            if cursor.line == cursor_line && cursor.column == cursor_col {
                return None;
            }

            self.move_active_cursor(cursor_line, cursor_col);
            return Some(result);
        }

        self.tabs[self.active].push_undo_snapshot(EditKind::Other, true);
        self.replace_active_lines(lines, cursor_line, cursor_col);

        Some(result)
    }

    fn selected_find_match_start(&self) -> Option<text_editor::Position> {
        if self.find.query.is_empty() {
            return None;
        }

        let cursor = self.tabs[self.active].content.cursor();
        let anchor = cursor.selection?;
        let (start, end) = if anchor.line < cursor.position.line
            || (anchor.line == cursor.position.line && anchor.column <= cursor.position.column)
        {
            (anchor, cursor.position)
        } else {
            (cursor.position, anchor)
        };

        let query_len = self.find.query.chars().count();
        if start.line == end.line && end.column == start.column + query_len {
            Some(start)
        } else {
            None
        }
    }

    fn align_find_current_to_visible_match(&mut self) {
        if self.find.matches.is_empty() {
            return;
        }

        if let Some(start) = self.selected_find_match_start() {
            if self.find.select_exact(&start) {
                return;
            }
        }

        let pos = self.tabs[self.active].content.cursor().position;
        self.find.find_nearest(&pos);
    }

    fn reindex_find_matches(&mut self) {
        if self.find.query.is_empty() {
            self.find.clear_results();
            return;
        }

        let text = self.tabs[self.active].content.text();
        self.find.compute_matches(&text);
        self.find.finish_reindex(self.active_tab_revision());
    }

    fn reindex_find_matches_to_nearest(&mut self) {
        self.reindex_find_matches();

        if !self.find.matches.is_empty() {
            self.align_find_current_to_visible_match();
        }
    }

    fn find_matches_stale(&self) -> bool {
        self.find.is_stale(self.active_tab_revision())
    }

    fn ensure_find_matches_current(&mut self) -> bool {
        if self.find_matches_stale() {
            self.reindex_find_matches();
            true
        } else {
            false
        }
    }

    fn mark_find_dirty(&mut self) {
        if self.find.query.is_empty() {
            return;
        }

        self.find.mark_dirty();
    }

    fn should_refresh_find_idle(&self) -> bool {
        self.find.visible && !self.find.query.is_empty() && self.find.is_dirty()
    }

    fn should_poll_autosave(&self) -> bool {
        self.needs_autosave
    }

    fn mark_active_document_changed(&mut self) {
        self.tabs[self.active].modified = true;
        self.needs_autosave = true;
        self.mark_find_dirty();
        if self.word_wrap {
            self.ensure_active_layout_cache();
        }
    }

    fn mark_active_content_changed(&mut self) {
        self.tabs[self.active].touch_content();
        self.mark_active_document_changed();
    }

    fn rebuild_content(&mut self, new_text: &str, cursor_line: usize, cursor_col: usize) {
        let tab = &mut self.tabs[self.active];
        tab.content = text_editor::Content::with_text(new_text);
        let line = cursor_line.min(tab.content.line_count().saturating_sub(1));
        tab.content.move_to(text_editor::Cursor {
            position: text_editor::Position {
                line,
                column: cursor_col,
            },
            selection: None,
        });
        self.vim.clear_preferred_column();
        self.mark_active_content_changed();
    }

    fn set_active_tab(&mut self, index: usize) {
        self.active = index;
        self.vim.on_tab_switch();
        self.vim.clear_preferred_column();
        self.reindex_find_matches_to_nearest();
        self.ensure_active_layout_cache();
        self.apply_block_cursor_if_normal();
    }

    fn editor_selection_text(&self) -> Option<String> {
        selection_text(&self.tabs[self.active].content, self.vim.mode)
    }

    fn caret_reveal_target(&self) -> Option<f32> {
        if !self.viewport.can_reveal() {
            return None;
        }

        let cursor = self.tabs[self.active].content.cursor().position;
        let caret_top = self.caret_top(cursor);
        let margin = LINE_HEIGHT_PX * 2.0;
        self.viewport
            .with_content_height(self.estimated_content_height())
            .reveal_offset(caret_top, LINE_HEIGHT_PX, margin)
    }

    fn reveal_scroll_task(target: f32) -> Task<Message> {
        iced::widget::operation::scroll_to(
            SCROLLABLE_ID.clone(),
            iced::widget::operation::AbsoluteOffset::<Option<f32>> {
                x: None,
                y: Some(target),
            },
        )
    }

    fn caret_top(&self, cursor: text_editor::Position) -> f32 {
        if !self.word_wrap {
            return cursor.line as f32 * LINE_HEIGHT_PX + EDITOR_PAD;
        }

        let content = &self.tabs[self.active].content;
        let Some(wrap_cols) = self.active_wrap_cols() else {
            return cursor.line as f32 * LINE_HEIGHT_PX + EDITOR_PAD;
        };

        let rows_before = self.tabs[self.active]
            .layout_cache_for(wrap_cols)
            .and_then(|cache| cache.line_start_visual_row.get(cursor.line).copied())
            .unwrap_or_else(|| {
                (0..cursor.line)
                    .map(|line_idx| {
                        content.line(line_idx).map_or(1, |line| {
                            viewport::visual_line_count(line.text.as_ref(), wrap_cols)
                        })
                    })
                    .sum()
            });

        let visual_row = rows_before
            + content.line(cursor.line).map_or(0, |line| {
                viewport::cursor_visual_row_in_line(line.text.as_ref(), cursor.column, wrap_cols)
            });

        EDITOR_PAD + visual_row as f32 * LINE_HEIGHT_PX
    }

    fn estimated_content_height(&self) -> f32 {
        viewport::content_height(self.viewport.height(), self.visual_line_count())
    }

    fn visual_line_count(&self) -> usize {
        let content = &self.tabs[self.active].content;
        if !self.word_wrap {
            return content.line_count().max(1);
        }

        let Some(wrap_cols) = self.active_wrap_cols() else {
            return content.line_count().max(1);
        };

        self.tabs[self.active]
            .layout_cache_for(wrap_cols)
            .map(|cache| cache.total_visual_rows)
            .unwrap_or_else(|| {
                (0..content.line_count())
                    .map(|line_idx| {
                        content.line(line_idx).map_or(1, |line| {
                            viewport::visual_line_count(line.text.as_ref(), wrap_cols)
                        })
                    })
                    .sum::<usize>()
                    .max(1)
            })
    }

    pub(crate) fn finish_update(&mut self, result: UpdateResult) -> Task<Message> {
        match result.reveal {
            RevealIntent::None => result.task,
            RevealIntent::RevealCaret => {
                if let Some(target) = self.caret_reveal_target() {
                    self.viewport.set_scroll_y(target);
                    return Task::batch([result.task, Self::reveal_scroll_task(target)]);
                }
                result.task
            }
        }
    }

    fn jump_to_line(&mut self, target_line: usize, select: bool) {
        let tab = &mut self.tabs[self.active];
        let cursor = tab.content.cursor();
        let pos = cursor.position;
        tab.content.move_to(text_editor::Cursor {
            position: text_editor::Position {
                line: target_line,
                column: pos.column,
            },
            selection: if select {
                Some(cursor.selection.unwrap_or(pos))
            } else {
                None
            },
        });
        self.vim.clear_preferred_column();
        if !select {
            self.apply_block_cursor_if_normal();
        }
    }

    fn vim_snapshot(&mut self) -> vim::TextSnapshot {
        let tab = &mut self.tabs[self.active];
        let cursor = tab.content.cursor().position;
        vim::TextSnapshot {
            lines: tab.lines(),
            cursor,
        }
    }

    fn open_find(&mut self, show_replace: bool) -> Task<Message> {
        self.find.visible = true;
        self.find.show_replace = show_replace;
        if let Some(sel) = self.editor_selection_text() {
            if !sel.contains('\n') {
                self.find.query = sel;
            }
        }
        self.reindex_find_matches_to_nearest();
        iced::widget::operation::focus(FIND_INPUT_ID.clone())
    }

    fn close_find(&mut self) -> Task<Message> {
        self.find.visible = false;
        self.apply_block_cursor_if_normal();
        iced::widget::operation::focus(EDITOR_ID.clone())
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        let result = self.update_inner(message);
        self.finish_update(result)
    }

    pub fn update_inner(&mut self, message: Message) -> UpdateResult {
        match message {
            Message::Edit(action) => {
                let reveal = reveal_intent_for_edit_action(&action);
                self.vim.clear_preferred_column();
                // Workaround for iced-rs/iced#3227 — remove when merged
                match &action {
                    text_editor::Action::SelectWord | text_editor::Action::SelectLine => {
                        self.multiclick_drag = true;
                    }
                    text_editor::Action::Click(_) => {
                        self.multiclick_drag = false;
                    }
                    _ => {}
                }

                // Shift+Click: extend selection instead of placing cursor
                if let text_editor::Action::Click(point) = &action {
                    if self.shift_held {
                        self.tabs[self.active]
                            .content
                            .perform(text_editor::Action::Drag(*point));
                        return UpdateResult::none();
                    }
                }

                let is_edit = matches!(action, text_editor::Action::Edit(_));

                // ── Bracket auto-close (Insert mode only) ────────────
                let mut auto_close_char: Option<char> = None;
                let mut skip_close = false;
                let mut delete_pair = false;
                if is_edit && self.vim.mode == vim::Mode::Insert {
                    if let text_editor::Action::Edit(ref edit) = action {
                        match edit {
                            text_editor::Edit::Insert(c) => {
                                auto_close_char = match c {
                                    '(' => Some(')'),
                                    '{' => Some('}'),
                                    '[' => Some(']'),
                                    '"' => Some('"'),
                                    '\'' => Some('\''),
                                    _ => None,
                                };
                                // Overtype: typing a close bracket when next char matches
                                if matches!(c, ')' | '}' | ']') {
                                    let tab = &self.tabs[self.active];
                                    let pos = tab.content.cursor().position;
                                    if let Some(line) = tab.content.line(pos.line) {
                                        let chars: Vec<char> = line.text.chars().collect();
                                        if pos.column < chars.len() && chars[pos.column] == *c {
                                            skip_close = true;
                                            auto_close_char = None;
                                        }
                                    }
                                }
                                // Quote handling
                                if matches!(c, '"' | '\'') && !skip_close {
                                    let tab = &self.tabs[self.active];
                                    let pos = tab.content.cursor().position;
                                    if let Some(line) = tab.content.line(pos.line) {
                                        let chars: Vec<char> = line.text.chars().collect();
                                        // Overtype: next char is same quote
                                        if pos.column < chars.len() && chars[pos.column] == *c {
                                            skip_close = true;
                                            auto_close_char = None;
                                        }
                                        // Don't auto-close next to word chars
                                        if auto_close_char.is_some()
                                            && pos.column < chars.len()
                                            && (chars[pos.column].is_alphanumeric()
                                                || chars[pos.column] == '_')
                                        {
                                            auto_close_char = None;
                                        }
                                        // Don't auto-close after word chars
                                        if auto_close_char.is_some()
                                            && pos.column > 0
                                            && (chars[pos.column - 1].is_alphanumeric()
                                                || chars[pos.column - 1] == '_')
                                        {
                                            auto_close_char = None;
                                        }
                                    }
                                }
                            }
                            text_editor::Edit::Backspace => {
                                let tab = &self.tabs[self.active];
                                let pos = tab.content.cursor().position;
                                if pos.column > 0 {
                                    if let Some(line) = tab.content.line(pos.line) {
                                        let chars: Vec<char> = line.text.chars().collect();
                                        if pos.column < chars.len() {
                                            delete_pair = matches!(
                                                (chars[pos.column - 1], chars[pos.column]),
                                                ('(', ')')
                                                    | ('{', '}')
                                                    | ('[', ']')
                                                    | ('"', '"')
                                                    | ('\'', '\'')
                                            );
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }

                if skip_close {
                    self.tabs[self.active]
                        .content
                        .perform(text_editor::Action::Move(text_editor::Motion::Right));
                    return UpdateResult::reveal(Task::none());
                }

                if is_edit {
                    let (kind, boundary) = match &action {
                        text_editor::Action::Edit(edit) => match edit {
                            text_editor::Edit::Insert(c) => (EditKind::Insert, c.is_whitespace()),
                            text_editor::Edit::Backspace | text_editor::Edit::Delete => {
                                (EditKind::Delete, false)
                            }
                            _ => (EditKind::Other, true),
                        },
                        _ => unreachable!(),
                    };
                    self.tabs[self.active].push_undo_snapshot(kind, boundary);
                }
                self.tabs[self.active].content.perform(action);

                if let Some(closer) = auto_close_char {
                    self.tabs[self.active]
                        .content
                        .perform(text_editor::Action::Edit(text_editor::Edit::Insert(closer)));
                    self.tabs[self.active]
                        .content
                        .perform(text_editor::Action::Move(text_editor::Motion::Left));
                }
                if delete_pair {
                    self.tabs[self.active]
                        .content
                        .perform(text_editor::Action::Edit(text_editor::Edit::Delete));
                }

                if is_edit {
                    self.mark_active_content_changed();
                }
                self.apply_block_cursor_if_normal();
                UpdateResult {
                    task: Task::none(),
                    reveal,
                }
            }

            Message::TabSelect(i) => {
                if i < self.tabs.len() && i != self.active {
                    self.set_active_tab(i);
                    return UpdateResult::reveal(Task::none());
                }
                UpdateResult::none()
            }

            Message::TabClose(i) => {
                let reveal = if i < self.tabs.len() && i == self.active && self.tabs.len() > 1 {
                    RevealIntent::RevealCaret
                } else {
                    RevealIntent::None
                };
                UpdateResult {
                    task: self.close_tab(i),
                    reveal,
                }
            }

            Message::CloseActiveTab => UpdateResult {
                task: self.close_tab(self.active),
                reveal: if self.tabs.len() > 1 {
                    RevealIntent::RevealCaret
                } else {
                    RevealIntent::None
                },
            },

            Message::New => {
                self.tabs.push(self.create_scratchpad_tab());
                self.set_active_tab(self.tabs.len() - 1);
                UpdateResult::reveal(Task::none())
            }

            Message::Open => UpdateResult::task(Task::perform(open_file(), Message::Opened)),

            Message::Opened(Ok((path, body))) => {
                if self.tabs.len() == 1
                    && self.tabs[0].is_scratchpad
                    && self.tabs[0].content.text().trim().is_empty()
                {
                    if let Some(old_path) = &self.tabs[0].path {
                        let _ = self.fs.remove_file(old_path);
                    }
                    self.tabs[0] = Tab::from_path(path, &body);
                    self.vim.clear_preferred_column();
                    self.reindex_find_matches_to_nearest();
                    self.ensure_active_layout_cache();
                    self.apply_block_cursor_if_normal();
                } else {
                    self.tabs.push(Tab::from_path(path, &body));
                    self.set_active_tab(self.tabs.len() - 1);
                }
                UpdateResult::reveal(Task::none())
            }
            Message::Opened(Err(_)) => UpdateResult::none(),

            Message::Save => {
                let tab = &self.tabs[self.active];
                let body = tab.content.text();
                UpdateResult::task(match tab.path.clone() {
                    Some(path) => Task::perform(save_file(path, body), Message::Saved),
                    None => Task::perform(save_file_as(body), Message::Saved),
                })
            }

            Message::SaveAs => {
                let body = self.tabs[self.active].content.text();
                UpdateResult::task(Task::perform(save_file_as(body), Message::Saved))
            }

            Message::Saved(Ok(path)) => {
                let tab = &mut self.tabs[self.active];
                tab.path = Some(path);
                tab.modified = false;
                tab.is_scratchpad = false;
                UpdateResult::none()
            }
            Message::Saved(Err(_)) => UpdateResult::none(),

            Message::AutosaveTick => {
                if !self.needs_autosave {
                    return UpdateResult::none();
                }
                self.needs_autosave = false;

                let saves: Vec<Task<Message>> = self
                    .tabs
                    .iter()
                    .filter(|t| t.modified && t.path.is_some())
                    .map(|t| {
                        let path = t.path.clone().unwrap();
                        let body = t.content.text();
                        Task::perform(save_file(path, body), Message::AutosaveComplete)
                    })
                    .collect();

                if saves.is_empty() {
                    UpdateResult::none()
                } else {
                    UpdateResult::task(Task::batch(saves))
                }
            }

            Message::AutosaveComplete(result) => {
                match result {
                    Ok(path) => {
                        for tab in &mut self.tabs {
                            if tab.path.as_ref() == Some(&path) {
                                tab.modified = false;
                                break;
                            }
                        }
                    }
                    Err(e) => eprintln!("lst: autosave failed: {e:?}"),
                }
                UpdateResult::none()
            }

            Message::GutterMove(point) => {
                let line = gutter_line_at(point);
                if self.gutter_hover_line == Some(line) {
                    return UpdateResult::none();
                }
                self.gutter_hover_line = Some(line);
                UpdateResult::none()
            }

            Message::GutterClick => {
                self.vim.clear_preferred_column();
                let y = self.gutter_hover_line.unwrap_or(0) as f32 * LINE_HEIGHT_PX;
                let vim_mode = self.vim.mode;
                let tab = &mut self.tabs[self.active];
                tab.content
                    .perform(text_editor::Action::Click(Point::new(0.0, y)));
                tab.content.perform(text_editor::Action::SelectLine);
                if let Some(sel) = selection_text(&tab.content, vim_mode) {
                    self.clipboard.copy_primary(&sel);
                }

                UpdateResult::task(iced::widget::operation::focus(EDITOR_ID.clone()))
            }

            // Workaround for iced-rs/iced#3227 — remove when merged
            Message::EditorMouseMove(point) => {
                let cell = editor_pointer_cell(point);
                if self.editor_pointer_cell == Some(cell) {
                    return UpdateResult::none();
                }
                self.editor_pointer_cell = Some(cell);
                if self.multiclick_drag {
                    self.vim.clear_preferred_column();
                    self.tabs[self.active]
                        .content
                        .perform(text_editor::Action::Drag(content_point_for_cell(cell)));
                    return UpdateResult::none();
                }
                UpdateResult::none()
            }
            Message::MulticlickReleased => {
                self.multiclick_drag = false;
                if let Some(sel) = self.editor_selection_text() {
                    self.clipboard.copy_primary(&sel);
                }
                UpdateResult::none()
            }
            Message::MiddleClickPaste => {
                if let Some(text) = self.clipboard.read_primary() {
                    if !text.is_empty() {
                        self.vim.clear_preferred_column();
                        let tab = &mut self.tabs[self.active];
                        tab.push_undo_snapshot(EditKind::Other, true);
                        let point = self
                            .editor_pointer_cell
                            .map(content_point_for_cell)
                            .unwrap_or(Point::ORIGIN);
                        tab.content.perform(text_editor::Action::Click(point));
                        tab.content
                            .perform(text_editor::Action::Edit(text_editor::Edit::Paste(
                                Arc::new(text),
                            )));
                        self.mark_active_content_changed();
                        self.apply_block_cursor_if_normal();
                        return UpdateResult::reveal(Task::none());
                    }
                }
                UpdateResult::none()
            }

            Message::Quit => UpdateResult::task(self.exit_with_clipboard()),

            // ── Undo / Redo ──────────────────────────────────────────────
            Message::Undo => {
                if self.tabs[self.active].undo() {
                    self.vim.clear_preferred_column();
                    self.mark_active_document_changed();
                }
                UpdateResult::reveal(Task::none())
            }

            Message::Redo => {
                if self.tabs[self.active].redo() {
                    self.vim.clear_preferred_column();
                    self.mark_active_document_changed();
                }
                UpdateResult::reveal(Task::none())
            }

            Message::AutoIndent => {
                let tab = &mut self.tabs[self.active];
                tab.push_undo_snapshot(EditKind::Other, true);

                let line_idx = tab.content.cursor().position.line;
                let indent: String = tab
                    .content
                    .line(line_idx)
                    .map(|l| {
                        let t = &*l.text;
                        let ws = t.len() - t.trim_start().len();
                        t[..ws].to_string()
                    })
                    .unwrap_or_default();

                tab.content
                    .perform(text_editor::Action::Edit(text_editor::Edit::Enter));
                for c in indent.chars() {
                    tab.content
                        .perform(text_editor::Action::Edit(text_editor::Edit::Insert(c)));
                }
                self.vim.clear_preferred_column();
                self.mark_active_content_changed();
                UpdateResult::reveal(Task::none())
            }

            // ── Find & Replace ───────────────────────────────────────────
            Message::FindOpen => {
                if self.find.visible {
                    return UpdateResult::task(self.close_find());
                }
                UpdateResult::task(self.open_find(false))
            }
            Message::FindOpenReplace => {
                if self.find.visible && self.find.show_replace {
                    return UpdateResult::task(self.close_find());
                }
                UpdateResult::task(self.open_find(true))
            }

            Message::FindClose => {
                if self.find.visible {
                    return UpdateResult::task(self.close_find());
                }
                UpdateResult::none()
            }

            Message::FindQueryChanged(q) => {
                if q == self.find.query {
                    return UpdateResult::none();
                }
                self.vim.clear_preferred_column();
                self.find.query = q;
                if self.find.query.is_empty() {
                    self.find.clear_results();
                } else {
                    self.reindex_find_matches_to_nearest();
                    if !self.find.matches.is_empty() {
                        self.find
                            .navigate_to_current(&mut self.tabs[self.active].content);
                    }
                }
                UpdateResult::none()
            }

            Message::FindReplaceChanged(r) => {
                self.find.replacement = r;
                UpdateResult::none()
            }

            Message::FindNext => {
                self.vim.clear_preferred_column();
                let was_stale = self.ensure_find_matches_current();
                if was_stale {
                    let cursor = self.tabs[self.active].content.cursor().position;
                    let _ = self.find.vim_next_from_cursor(&cursor);
                } else {
                    self.find.next();
                }
                self.find
                    .navigate_to_current(&mut self.tabs[self.active].content);
                UpdateResult::reveal(iced::widget::operation::focus(EDITOR_ID.clone()))
            }

            Message::FindPrev => {
                self.vim.clear_preferred_column();
                let was_stale = self.ensure_find_matches_current();
                if was_stale {
                    let cursor = self.tabs[self.active].content.cursor().position;
                    let _ = self.find.vim_prev_from_cursor(&cursor);
                } else {
                    self.find.prev();
                }
                self.find
                    .navigate_to_current(&mut self.tabs[self.active].content);
                UpdateResult::reveal(iced::widget::operation::focus(EDITOR_ID.clone()))
            }

            Message::FindRefreshTick => {
                if !self.should_refresh_find_idle() {
                    return UpdateResult::none();
                }

                let Some(dirty_since) = self.find.dirty_since() else {
                    return UpdateResult::none();
                };

                if dirty_since.elapsed() < Duration::from_millis(200) {
                    return UpdateResult::none();
                }

                self.reindex_find_matches_to_nearest();
                UpdateResult::none()
            }

            Message::ReplaceOne => {
                let was_stale = self.ensure_find_matches_current();
                if self.find.matches.is_empty() {
                    return UpdateResult::none();
                }
                if was_stale {
                    self.align_find_current_to_visible_match();
                }
                let cursor_after = {
                    let tab = &mut self.tabs[self.active];
                    tab.push_undo_snapshot(EditKind::Other, true);
                    self.find.navigate_to_current(&mut tab.content);
                    let replacement = Arc::new(self.find.replacement.clone());
                    tab.content
                        .perform(text_editor::Action::Edit(text_editor::Edit::Paste(
                            replacement,
                        )));
                    tab.content.cursor().position
                };
                self.vim.clear_preferred_column();
                self.mark_active_content_changed();
                self.reindex_find_matches();
                if !self.find.matches.is_empty() {
                    self.find.find_nearest(&cursor_after);
                    self.find
                        .navigate_to_current(&mut self.tabs[self.active].content);
                }
                UpdateResult::reveal(Task::none())
            }

            Message::ReplaceAll => {
                if self.find.query.is_empty() {
                    return UpdateResult::none();
                }
                let cursor_pos = self.tabs[self.active].content.cursor().position;
                let query = self.find.query.clone();
                let replacement = self.find.replacement.clone();

                if self
                    .apply_line_edit(|lines| {
                        let new_lines: Vec<String> = lines
                            .iter()
                            .map(|line| line.replace(&query, &replacement))
                            .collect();

                        if new_lines == *lines {
                            return None;
                        }

                        *lines = new_lines;
                        Some(((), cursor_pos.line, cursor_pos.column))
                    })
                    .is_none()
                {
                    self.reindex_find_matches_to_nearest();
                    return UpdateResult::none();
                }
                self.reindex_find_matches_to_nearest();
                UpdateResult::reveal(Task::none())
            }

            // ── Word Wrap ────────────────────────────────────────────────
            Message::ToggleWordWrap => {
                self.word_wrap = !self.word_wrap;
                self.ensure_active_layout_cache();
                UpdateResult::reveal(Task::none())
            }

            // ── Tab Reorder ──────────────────────────────────────────────
            Message::MoveTabLeft => {
                if self.active > 0 {
                    self.tabs.swap(self.active, self.active - 1);
                    self.active -= 1;
                }
                UpdateResult::none()
            }

            Message::MoveTabRight => {
                if self.active + 1 < self.tabs.len() {
                    self.tabs.swap(self.active, self.active + 1);
                    self.active += 1;
                }
                UpdateResult::none()
            }

            // ── Line Operations ─────────────────────────────────────────
            Message::DeleteLine => {
                let pos = self.tabs[self.active].content.cursor().position;
                let _ = self.apply_line_edit(|lines| {
                    let line = editor_ops::delete_line(lines, pos.line);
                    Some(((), line, pos.column))
                });
                UpdateResult::reveal(Task::none())
            }

            Message::MoveLineUp => {
                let pos = self.tabs[self.active].content.cursor().position;
                let changed = self.apply_line_edit(|lines| {
                    let line = editor_ops::move_line_up(lines, pos.line)?;
                    Some(((), line, pos.column))
                });
                if changed.is_none() {
                    return UpdateResult::none();
                }
                UpdateResult::reveal(Task::none())
            }

            Message::MoveLineDown => {
                let pos = self.tabs[self.active].content.cursor().position;
                let changed = self.apply_line_edit(|lines| {
                    let line = editor_ops::move_line_down(lines, pos.line)?;
                    Some(((), line, pos.column))
                });

                if changed.is_none() {
                    return UpdateResult::none();
                }
                UpdateResult::reveal(Task::none())
            }

            Message::DuplicateLine => {
                let pos = self.tabs[self.active].content.cursor().position;
                let _ = self.apply_line_edit(|lines| {
                    let line = editor_ops::duplicate_line(lines, pos.line);
                    Some(((), line, pos.column))
                });
                UpdateResult::reveal(Task::none())
            }

            Message::ToggleComment => {
                let prefix = self.tabs[self.active]
                    .path
                    .as_ref()
                    .and_then(|p| p.extension())
                    .and_then(|e| editor_ops::comment_prefix(e.to_string_lossy().as_ref()))
                    .unwrap_or("//");
                let cursor = self.tabs[self.active].content.cursor();
                let sel_anchor = cursor.selection.unwrap_or(cursor.position);
                let first = cursor.position.line.min(sel_anchor.line);
                let last = cursor.position.line.max(sel_anchor.line);
                let cursor_line = cursor.position.line;
                let cursor_col = cursor.position.column;
                let _ = self.apply_line_edit(|lines| {
                    let (line, col) = editor_ops::toggle_comment(
                        lines,
                        first,
                        last,
                        cursor_line,
                        cursor_col,
                        prefix,
                    );
                    Some(((), line, col))
                });
                self.apply_block_cursor_if_normal();
                UpdateResult::reveal(Task::none())
            }

            // ── Page Movement ───────────────────────────────────────
            Message::PageUp(lines, select) => {
                let target = self.tabs[self.active]
                    .content
                    .cursor()
                    .position
                    .line
                    .saturating_sub(lines);
                self.jump_to_line(target, select);
                UpdateResult::reveal(Task::none())
            }

            Message::PageDown(lines, select) => {
                let pos = self.tabs[self.active].content.cursor().position;
                let last = self.tabs[self.active]
                    .content
                    .line_count()
                    .saturating_sub(1);
                self.jump_to_line((pos.line + lines).min(last), select);
                UpdateResult::reveal(Task::none())
            }

            // ── Tab Cycling ─────────────────────────────────────────────
            Message::NextTab => {
                if self.tabs.len() > 1 {
                    self.set_active_tab((self.active + 1) % self.tabs.len());
                    return UpdateResult::reveal(Task::none());
                }
                UpdateResult::none()
            }

            Message::PrevTab => {
                if self.tabs.len() > 1 {
                    let prev = if self.active == 0 {
                        self.tabs.len() - 1
                    } else {
                        self.active - 1
                    };
                    self.set_active_tab(prev);
                    return UpdateResult::reveal(Task::none());
                }
                UpdateResult::none()
            }

            // ── Go to Line ──────────────────────────────────────────────
            Message::GotoLineOpen => {
                if self.goto_line.is_some() {
                    self.goto_line = None;
                    return UpdateResult::task(iced::widget::operation::focus(EDITOR_ID.clone()));
                }
                self.goto_line = Some(String::new());
                UpdateResult::task(iced::widget::operation::focus(GOTO_LINE_ID.clone()))
            }

            Message::GotoLineClose => {
                if self.goto_line.is_some() {
                    self.goto_line = None;
                    return UpdateResult::task(iced::widget::operation::focus(EDITOR_ID.clone()));
                }
                // Also close find bar (Escape from subscription closes topmost overlay)
                if self.find.visible {
                    return UpdateResult::task(self.close_find());
                }
                // Vim: Escape cascades into mode transitions
                let snapshot = self.vim_snapshot();
                let cursor = snapshot.cursor;
                let commands = self.vim.enter_normal_from_escape(cursor, &snapshot);
                self.execute_vim_commands(commands)
            }

            Message::GotoLineChanged(s) => {
                self.goto_line = Some(s);
                UpdateResult::none()
            }

            Message::GotoLineSubmit => {
                let mut reveal = RevealIntent::None;
                if let Some(ref text) = self.goto_line {
                    if let Ok(line_num) = text.trim().parse::<usize>() {
                        let tab = &mut self.tabs[self.active];
                        let target = line_num.saturating_sub(1);
                        let target = target.min(tab.content.line_count().saturating_sub(1));
                        tab.content.move_to(text_editor::Cursor {
                            position: text_editor::Position {
                                line: target,
                                column: 0,
                            },
                            selection: None,
                        });
                        self.vim.clear_preferred_column();
                        self.apply_block_cursor_if_normal();
                        reveal = RevealIntent::RevealCaret;
                    }
                }
                self.goto_line = None;
                UpdateResult {
                    task: iced::widget::operation::focus(EDITOR_ID.clone()),
                    reveal,
                }
            }

            // ── Modifier Tracking ───────────────────────────────────────
            Message::ModifiersChanged(mods) => {
                self.shift_held = mods.shift();
                UpdateResult::none()
            }

            // ── Scroll Tracking ─────────────────────────────────────────
            Message::Scrolled(viewport) => {
                let previous_wrap_cols = self.active_wrap_cols();
                self.viewport.update(viewport);
                if self.active_wrap_cols() != previous_wrap_cols {
                    self.ensure_active_layout_cache();
                }
                UpdateResult::none()
            }

            // ── Vim ────────────────────────────────────────────────────
            Message::VimKey(ref key, mods) => {
                let snapshot = self.vim_snapshot();
                let commands = self.vim.handle_key(key, mods, &snapshot);
                self.execute_vim_commands(commands)
            }
        }
    }

    fn execute_vim_commands(&mut self, commands: Vec<vim::VimCommand>) -> UpdateResult {
        use vim::VimCommand;
        let mut task = iced::widget::operation::focus(EDITOR_ID.clone());
        let mut reveal = RevealIntent::None;
        for cmd in commands {
            match cmd {
                VimCommand::Noop => {}
                VimCommand::MoveTo(p) => {
                    self.tabs[self.active].content.move_to(text_editor::Cursor {
                        position: p,
                        selection: None,
                    });
                    reveal = RevealIntent::RevealCaret;
                }
                VimCommand::Select { anchor, head } => {
                    self.apply_vim_select(anchor, head);
                    reveal = RevealIntent::RevealCaret;
                }
                VimCommand::DeleteRange { from, to } => {
                    let deleted = self.vim_delete_range(from, to);
                    self.vim.register = vim::Register::Char(deleted);
                    reveal = RevealIntent::RevealCaret;
                }
                VimCommand::DeleteLines { first, last } => {
                    let deleted = self.vim_delete_lines(first, last);
                    self.vim.register = vim::Register::Line(deleted);
                    reveal = RevealIntent::RevealCaret;
                }
                VimCommand::ChangeRange { from, to } => {
                    let deleted = self.vim_delete_range(from, to);
                    self.vim.register = vim::Register::Char(deleted);
                    reveal = RevealIntent::RevealCaret;
                }
                VimCommand::ChangeLines { first, last } => {
                    let deleted = self.vim_change_lines(first, last);
                    self.vim.register = vim::Register::Line(deleted);
                    reveal = RevealIntent::RevealCaret;
                }
                VimCommand::YankRange { from, to } => {
                    let yanked = self.vim_extract_range(from, to);
                    self.vim.register = vim::Register::Char(yanked);
                }
                VimCommand::YankLines { first, last } => {
                    let yanked = self.vim_extract_lines(first, last);
                    self.vim.register = vim::Register::Line(yanked);
                }
                VimCommand::EnterInsert => {
                    collapse_selection_to_caret(&mut self.tabs[self.active].content);
                    self.vim.mode = vim::Mode::Insert;
                }
                VimCommand::PasteAfter => {
                    self.vim_paste(false);
                    reveal = RevealIntent::RevealCaret;
                }
                VimCommand::PasteBefore => {
                    self.vim_paste(true);
                    reveal = RevealIntent::RevealCaret;
                }
                VimCommand::OpenLineBelow => {
                    self.vim_open_line(false);
                    reveal = RevealIntent::RevealCaret;
                }
                VimCommand::OpenLineAbove => {
                    self.vim_open_line(true);
                    reveal = RevealIntent::RevealCaret;
                }
                VimCommand::JoinLines { count } => {
                    self.vim_join_lines(count);
                    reveal = RevealIntent::RevealCaret;
                }
                VimCommand::ReplaceChar { ch, count } => {
                    self.vim_replace_char(ch, count);
                    reveal = RevealIntent::RevealCaret;
                }
                VimCommand::Undo => {
                    if self.tabs[self.active].undo() {
                        self.vim.clear_preferred_column();
                        self.mark_active_document_changed();
                        reveal = RevealIntent::RevealCaret;
                    }
                }
                VimCommand::Redo => {
                    if self.tabs[self.active].redo() {
                        self.vim.clear_preferred_column();
                        self.mark_active_document_changed();
                        reveal = RevealIntent::RevealCaret;
                    }
                }
                VimCommand::OpenFind => {
                    if self.vim.mode == vim::Mode::Normal {
                        collapse_selection_to_caret(&mut self.tabs[self.active].content);
                    }
                    task = self.open_find(false);
                }
                VimCommand::FindNext => {
                    self.ensure_find_matches_current();
                    let cursor = self.tabs[self.active].content.cursor().position;
                    if let Some(target) = self.find.vim_next_from_cursor(&cursor) {
                        self.move_to_vim_search_target(target);
                        reveal = RevealIntent::RevealCaret;
                    }
                    task = iced::widget::operation::focus(EDITOR_ID.clone());
                }
                VimCommand::FindPrev => {
                    self.ensure_find_matches_current();
                    let cursor = self.tabs[self.active].content.cursor().position;
                    if let Some(target) = self.find.vim_prev_from_cursor(&cursor) {
                        self.move_to_vim_search_target(target);
                        reveal = RevealIntent::RevealCaret;
                    }
                    task = iced::widget::operation::focus(EDITOR_ID.clone());
                }
                VimCommand::SearchWordUnderCursor { word, forward } => {
                    self.find.query = word;
                    self.reindex_find_matches();
                    let cursor = self.tabs[self.active].content.cursor().position;
                    let target = if forward {
                        self.find.vim_next_from_cursor(&cursor)
                    } else {
                        self.find.vim_prev_from_cursor(&cursor)
                    };
                    if let Some(target) = target {
                        self.move_to_vim_search_target(target);
                        reveal = RevealIntent::RevealCaret;
                    }
                    task = iced::widget::operation::focus(EDITOR_ID.clone());
                }
                VimCommand::TransformCaseRange {
                    from,
                    to,
                    uppercase,
                } => {
                    self.vim_transform_case_range(from, to, uppercase);
                    reveal = RevealIntent::RevealCaret;
                }
                VimCommand::TransformCaseLines {
                    first,
                    last,
                    uppercase,
                } => {
                    self.vim_transform_case_lines(first, last, uppercase);
                    reveal = RevealIntent::RevealCaret;
                }
            }
        }
        self.apply_block_cursor_if_normal();
        UpdateResult { task, reveal }
    }

    fn apply_block_cursor_if_normal(&mut self) {
        if self.vim.mode == vim::Mode::Normal {
            self.apply_block_cursor();
        }
    }

    fn apply_vim_select(&mut self, anchor: text_editor::Position, head: text_editor::Position) {
        self.tabs[self.active].content.move_to(text_editor::Cursor {
            position: head,
            selection: Some(anchor),
        });
    }

    fn apply_block_cursor(&mut self) {
        let tab = &mut self.tabs[self.active];
        let pos = tab.content.cursor().position;
        let line_len = tab
            .content
            .line(pos.line)
            .map(|l| l.text.chars().count())
            .unwrap_or(0);
        let selection = if pos.column < line_len {
            text_editor::Position {
                line: pos.line,
                column: pos.column + 1,
            }
        } else {
            pos
        };
        tab.content.move_to(text_editor::Cursor {
            position: pos,
            selection: Some(selection),
        });
    }

    fn move_to_vim_search_target(&mut self, target: text_editor::Position) {
        match self.vim.mode {
            vim::Mode::Visual | vim::Mode::VisualLine => {
                // Search repeat is a motion in Visual mode, so reuse the Vim
                // selection logic instead of rebuilding the range here.
                let snapshot = self.vim_snapshot();
                if let vim::VimCommand::Select { anchor, head } =
                    self.vim.selection_command(target, &snapshot)
                {
                    self.apply_vim_select(anchor, head);
                }
            }
            _ => {
                self.tabs[self.active].content.move_to(text_editor::Cursor {
                    position: target,
                    selection: None,
                });
            }
        }
    }

    // ── Vim helpers ─────────────────────────────────────────────────────

    fn vim_delete_range(
        &mut self,
        from: text_editor::Position,
        to: text_editor::Position,
    ) -> String {
        self.apply_line_edit(|lines| {
            let deleted = extract_text_range(lines, &from, &to);
            remove_text_range(lines, &from, &to);
            let cursor_col = from.column.min(
                lines
                    .get(from.line)
                    .map_or(0, |line| line.chars().count().saturating_sub(1)),
            );
            Some((deleted, from.line, cursor_col))
        })
        .unwrap_or_default()
    }

    fn vim_delete_lines(&mut self, first: usize, last: usize) -> String {
        self.apply_line_edit(|lines| {
            let first = first.min(lines.len().saturating_sub(1));
            let last = last.min(lines.len().saturating_sub(1));
            let deleted = lines[first..=last].join("\n");
            lines.drain(first..=last);
            if lines.is_empty() {
                lines.push(String::new());
            }
            let cursor_line = first.min(lines.len().saturating_sub(1));
            Some((deleted, cursor_line, 0))
        })
        .unwrap_or_default()
    }

    fn vim_change_lines(&mut self, first: usize, last: usize) -> String {
        self.apply_line_edit(|lines| {
            let first = first.min(lines.len().saturating_sub(1));
            let last = last.min(lines.len().saturating_sub(1));
            let indent: String = lines[first]
                .chars()
                .take_while(|c| c.is_whitespace())
                .collect();
            let deleted = lines[first..=last].join("\n");
            lines.drain(first..=last);
            lines.insert(first, indent.clone());
            Some((deleted, first, indent.chars().count()))
        })
        .unwrap_or_default()
    }

    fn vim_extract_range(
        &mut self,
        from: text_editor::Position,
        to: text_editor::Position,
    ) -> String {
        let lines = self.tabs[self.active].lines();
        extract_text_range(lines.as_ref(), &from, &to)
    }

    fn vim_extract_lines(&mut self, first: usize, last: usize) -> String {
        let lines = self.tabs[self.active].lines();
        let first = first.min(lines.len().saturating_sub(1));
        let last = last.min(lines.len().saturating_sub(1));
        lines[first..=last].join("\n")
    }

    fn vim_paste(&mut self, before: bool) {
        let register = self.vim.register.clone();
        match register {
            vim::Register::Empty => {}
            vim::Register::Char(ref paste_text) => {
                let cursor = self.tabs[self.active].content.cursor().position;
                let _ = self.apply_line_edit(|lines| {
                    let line_chars: Vec<char> = lines[cursor.line].chars().collect();
                    let insert_col = if before {
                        cursor.column.min(line_chars.len())
                    } else {
                        (cursor.column + 1).min(line_chars.len())
                    };
                    let prefix: String = line_chars[..insert_col].iter().collect();
                    let suffix: String = line_chars[insert_col..].iter().collect();

                    let paste_lines: Vec<&str> = paste_text.split('\n').collect();
                    if paste_lines.len() == 1 {
                        lines[cursor.line] = format!("{prefix}{}{suffix}", paste_lines[0]);
                        let cursor_col =
                            insert_col + paste_lines[0].chars().count().saturating_sub(1);
                        return Some(((), cursor.line, cursor_col));
                    }

                    let first_new = format!("{prefix}{}", paste_lines[0]);
                    let last_new = format!("{}{suffix}", paste_lines.last().unwrap_or(&""));
                    let mut new_lines: Vec<String> = lines[..cursor.line].to_vec();
                    new_lines.push(first_new);
                    for paste_line in &paste_lines[1..paste_lines.len() - 1] {
                        new_lines.push((*paste_line).to_string());
                    }
                    new_lines.push(last_new);
                    new_lines.extend(lines[cursor.line + 1..].iter().cloned());

                    let cursor_line = cursor.line + paste_lines.len() - 1;
                    let cursor_col = paste_lines
                        .last()
                        .unwrap_or(&"")
                        .chars()
                        .count()
                        .saturating_sub(1);

                    *lines = new_lines;
                    Some(((), cursor_line, cursor_col))
                });
            }
            vim::Register::Line(ref paste_text) => {
                let cursor = self.tabs[self.active].content.cursor().position;
                let _ = self.apply_line_edit(|lines| {
                    let insert_at = if before { cursor.line } else { cursor.line + 1 };
                    lines.splice(
                        insert_at..insert_at,
                        paste_text.split('\n').map(String::from),
                    );
                    let indent = lines.get(insert_at).map_or(0, |line| {
                        line.chars().take_while(|c| c.is_whitespace()).count()
                    });
                    Some(((), insert_at, indent))
                });
            }
        }
    }

    fn vim_open_line(&mut self, above: bool) {
        let pos = self.tabs[self.active].content.cursor().position;
        let _ = self.apply_line_edit(|lines| {
            let indent: String = lines.get(pos.line).map_or(String::new(), |line| {
                line.chars().take_while(|c| c.is_whitespace()).collect()
            });
            let idx = if above { pos.line } else { pos.line + 1 };
            lines.insert(idx, indent.clone());
            Some(((), idx, indent.chars().count()))
        });
    }

    fn vim_join_lines(&mut self, count: usize) {
        let pos = self.tabs[self.active].content.cursor().position;
        let _ = self.apply_line_edit(|lines| {
            if pos.line + 1 >= lines.len() {
                return None;
            }

            let join_end = (pos.line + count).min(lines.len() - 1);
            let mut joined = lines[pos.line].trim_end().to_string();
            let join_col = joined.chars().count();
            for line in lines.drain((pos.line + 1)..=join_end) {
                let trimmed = line.trim_start();
                if !trimmed.is_empty() {
                    joined.push(' ');
                    joined.push_str(trimmed);
                }
            }
            lines[pos.line] = joined;
            Some(((), pos.line, join_col))
        });
    }

    fn vim_replace_char(&mut self, c: char, count: usize) {
        let pos = self.tabs[self.active].content.cursor().position;
        let _ = self.apply_line_edit(|lines| {
            let chars: Vec<char> = lines
                .get(pos.line)
                .map_or(Vec::new(), |line| line.chars().collect());
            if pos.column + count > chars.len() {
                return None;
            }

            let mut new_chars = chars;
            for i in 0..count {
                new_chars[pos.column + i] = c;
            }
            lines[pos.line] = new_chars.into_iter().collect();
            Some(((), pos.line, pos.column + count - 1))
        });
    }

    fn vim_transform_case_range(
        &mut self,
        from: text_editor::Position,
        to: text_editor::Position,
        uppercase: bool,
    ) {
        let _ = self.apply_line_edit(|lines| {
            editor_ops::transform_case_range(
                lines,
                from.line,
                from.column,
                to.line,
                to.column,
                uppercase,
            );
            Some(((), from.line, from.column))
        });
    }

    fn vim_transform_case_lines(&mut self, first: usize, last: usize, uppercase: bool) {
        let _ = self.apply_line_edit(|lines| {
            if lines.is_empty() {
                return None;
            }

            let first = first.min(lines.len().saturating_sub(1));
            let last = last.min(lines.len().saturating_sub(1));
            for line in &mut lines[first..=last] {
                *line = if uppercase {
                    line.to_uppercase()
                } else {
                    line.to_lowercase()
                };
            }
            Some(((), first, 0))
        });
    }

    pub fn view(&self) -> Element<'_, Message> {
        let tab = &self.tabs[self.active];

        let theme = self.theme();
        let p = theme.extended_palette();
        let bg_base = p.background.base.color;
        let bg_weak = p.background.weak.color;
        let bg_strong = p.background.strong.color;
        let text_main = p.background.base.text;
        let text_muted = p.background.strong.text;
        let primary = p.primary.base.color;
        let editor_font = EDITOR_FONT.font;
        let vim_mode = self.vim.mode;

        // ── Tab bar ──────────────────────────────────────────────────────
        let tab_buttons: Vec<Element<Message>> = self
            .tabs
            .iter()
            .enumerate()
            .flat_map(|(i, t)| {
                let is_active = i == self.active;
                let name = t.display_name();
                let label = format!(" {name} ");

                let bg = if is_active { bg_strong } else { bg_weak };
                let fg = if is_active { text_main } else { text_muted };

                let tab_btn: Element<Message> = button(text(label).size(13).color(fg))
                    .style(flat_btn(bg))
                    .padding(Padding {
                        top: 8.0,
                        bottom: 8.0,
                        left: 12.0,
                        right: 4.0,
                    })
                    .on_press(Message::TabSelect(i))
                    .into();

                let close_btn: Element<Message> =
                    button(text("\u{00d7}").size(14).color(text_muted))
                        .style(flat_btn(bg))
                        .padding(Padding {
                            top: 8.0,
                            bottom: 8.0,
                            left: 2.0,
                            right: 8.0,
                        })
                        .on_press(Message::TabClose(i))
                        .into();

                [tab_btn, close_btn]
            })
            .collect();

        let new_tab_btn: Element<Message> = button(text("+").size(14).color(text_muted))
            .style(flat_btn(bg_weak))
            .padding(Padding {
                top: 8.0,
                bottom: 8.0,
                left: 10.0,
                right: 10.0,
            })
            .on_press(Message::New)
            .into();

        let mut tab_row_items = tab_buttons;
        tab_row_items.push(new_tab_btn);

        let tab_bar = container(row(tab_row_items))
            .width(Length::Fill)
            .style(solid_bg(bg_weak));

        // ── Find bar (conditional) ───────────────────────────────────────
        let find_bar = if self.find.visible {
            let match_label = if self.find.is_dirty() && !self.find.query.is_empty() {
                "Updating...".to_string()
            } else if self.find.matches.is_empty() {
                if self.find.query.is_empty() {
                    String::new()
                } else {
                    "No matches".into()
                }
            } else {
                format!("{}/{}", self.find.current + 1, self.find.matches.len())
            };

            let find_row = row![
                text_input("Find\u{2026}", &self.find.query)
                    .id(FIND_INPUT_ID.clone())
                    .on_input(Message::FindQueryChanged)
                    .on_submit(Message::FindNext)
                    .font(editor_font)
                    .size(13)
                    .width(220),
                text(match_label).size(12).color(text_muted),
                button(text("\u{25b2}").size(10).color(text_muted))
                    .style(flat_btn(bg_weak))
                    .on_press(Message::FindPrev)
                    .padding(Padding::from([4, 8])),
                button(text("\u{25bc}").size(10).color(text_muted))
                    .style(flat_btn(bg_weak))
                    .on_press(Message::FindNext)
                    .padding(Padding::from([4, 8])),
                button(text("\u{00d7}").size(14).color(text_muted))
                    .style(flat_btn(bg_weak))
                    .on_press(Message::FindClose)
                    .padding(Padding::from([4, 8])),
            ]
            .spacing(4)
            .align_y(iced::Alignment::Center);

            let mut find_col: iced::widget::Column<'_, Message> = column![find_row].spacing(4);

            if self.find.show_replace {
                let replace_row = row![
                    text_input("Replace\u{2026}", &self.find.replacement)
                        .on_input(Message::FindReplaceChanged)
                        .on_submit(Message::ReplaceOne)
                        .font(editor_font)
                        .size(13)
                        .width(220),
                    button(text("Replace").size(12).color(text_muted))
                        .style(flat_btn(bg_weak))
                        .on_press(Message::ReplaceOne)
                        .padding(Padding::from([4, 8])),
                    button(text("All").size(12).color(text_muted))
                        .style(flat_btn(bg_weak))
                        .on_press(Message::ReplaceAll)
                        .padding(Padding::from([4, 8])),
                ]
                .spacing(4)
                .align_y(iced::Alignment::Center);

                find_col = find_col.push(replace_row);
            }

            Some(
                container(find_col)
                    .padding(Padding::from([4, 8]))
                    .style(solid_bg(bg_strong)),
            )
        } else {
            None
        };

        // ── Editor area ──────────────────────────────────────────────────
        let n_lines = tab.content.line_count().max(1);
        let cursor_line = tab.content.cursor().position.line;
        let last_line = n_lines.saturating_sub(1);
        let content_ref = &tab.content;
        let word_wrap = self.word_wrap;
        let scroll_y = self.viewport.scroll_y();
        // Escape is handled by the subscription (handle_key), not key_binding,
        // so find_visible / goto_visible are not needed in the closure.
        let highlight_ext = tab
            .path
            .as_ref()
            .and_then(|p| p.extension())
            .map(|e| e.to_string_lossy().into_owned());

        let editor_area = responsive(move |size| {
            let vh = size.height;
            let overscroll = vh * 0.4;
            let (line_num_text, top_gap, bottom_gap) = if word_wrap {
                let wrap_cols = wrapped_cols(size.width, n_lines).unwrap_or(1);
                let temporary_layout_cache;
                let layout_cache = if let Some(cache) = tab.layout_cache_for(wrap_cols) {
                    cache
                } else {
                    temporary_layout_cache = tab.build_layout_cache(wrap_cols);
                    &temporary_layout_cache
                };
                let visible_rows =
                    viewport::visible_row_range(scroll_y, vh, layout_cache.total_visual_rows);

                (
                    layout_cache.visible_gutter_text(n_lines, visible_rows.start, visible_rows.end),
                    visible_rows.start as f32 * LINE_HEIGHT_PX,
                    layout_cache
                        .total_visual_rows
                        .saturating_sub(visible_rows.end) as f32
                        * LINE_HEIGHT_PX,
                )
            } else {
                let visible_rows = viewport::visible_row_range(scroll_y, vh, n_lines);

                (
                    tab.visible_unwrapped_gutter_text(visible_rows.start, visible_rows.end),
                    visible_rows.start as f32 * LINE_HEIGHT_PX,
                    n_lines.saturating_sub(visible_rows.end) as f32 * LINE_HEIGHT_PX,
                )
            };

            let line_numbers = mouse_area(
                container(column![
                    Space::new().height(top_gap),
                    text(line_num_text)
                        .size(FONT_SIZE)
                        .font(editor_font)
                        .color(text_muted)
                        .line_height(Pixels(LINE_HEIGHT_PX))
                        .wrapping(iced::widget::text::Wrapping::None),
                    Space::new().height(bottom_gap),
                ])
                .padding(Padding {
                    top: EDITOR_PAD,
                    bottom: EDITOR_PAD + overscroll,
                    left: viewport::LINE_NUMBER_LEFT_PAD,
                    right: 0.0,
                }),
            )
            .on_move(Message::GutterMove)
            .on_press(Message::GutterClick);

            let gutter_line = container(Space::new())
                .width(viewport::GUTTER_SEPARATOR_WIDTH)
                .height(Length::Fill)
                .style(solid_bg(bg_strong));

            let wrapping = if word_wrap {
                iced::widget::text::Wrapping::Word
            } else {
                iced::widget::text::Wrapping::None
            };

            let editor = text_editor(content_ref)
                .id(EDITOR_ID.clone())
                .on_action(Message::Edit)
                .font(editor_font)
                .size(FONT_SIZE)
                .line_height(Pixels(LINE_HEIGHT_PX))
                .wrapping(wrapping)
                .padding(Padding {
                    top: EDITOR_PAD,
                    bottom: EDITOR_PAD + overscroll,
                    left: EDITOR_PAD,
                    right: viewport::EDITOR_RIGHT_PAD,
                })
                .height(Length::Shrink)
                .min_height(vh)
                .highlight_with::<highlight::LstHighlighter>(
                    highlight::Settings {
                        extension: highlight_ext.clone(),
                    },
                    highlight::format,
                )
                .key_binding(move |key_press| {
                    let key = &key_press.key;
                    let modified_key = &key_press.modified_key;
                    let mods = key_press.modifiers;

                    // Phase 1: Ctrl/Cmd shortcuts — always active, all modes
                    if mods.command() {
                        match key {
                            keyboard::Key::Named(named) => match named {
                                keyboard::key::Named::Tab => {
                                    return Some(text_editor::Binding::Custom(if mods.shift() {
                                        Message::PrevTab
                                    } else {
                                        Message::NextTab
                                    }));
                                }
                                keyboard::key::Named::Backspace => {
                                    return Some(text_editor::Binding::Sequence(vec![
                                        text_editor::Binding::Select(text_editor::Motion::WordLeft),
                                        text_editor::Binding::Backspace,
                                    ]));
                                }
                                keyboard::key::Named::Delete => {
                                    return Some(text_editor::Binding::Sequence(vec![
                                        text_editor::Binding::Select(
                                            text_editor::Motion::WordRight,
                                        ),
                                        text_editor::Binding::Delete,
                                    ]));
                                }
                                keyboard::key::Named::PageUp if mods.shift() => {
                                    return Some(text_editor::Binding::Custom(
                                        Message::MoveTabLeft,
                                    ));
                                }
                                keyboard::key::Named::PageDown if mods.shift() => {
                                    return Some(text_editor::Binding::Custom(
                                        Message::MoveTabRight,
                                    ));
                                }
                                _ => {}
                            },
                            keyboard::Key::Character(c) => match c.as_str() {
                                "z" if mods.shift() => {
                                    return Some(text_editor::Binding::Custom(Message::Redo));
                                }
                                "z" => {
                                    return Some(text_editor::Binding::Custom(Message::Undo));
                                }
                                "k" if mods.shift() => {
                                    return Some(text_editor::Binding::Custom(Message::DeleteLine));
                                }
                                "d" if mods.shift() => {
                                    return Some(text_editor::Binding::Custom(
                                        Message::DuplicateLine,
                                    ));
                                }
                                "/" => {
                                    return Some(text_editor::Binding::Custom(
                                        Message::ToggleComment,
                                    ));
                                }
                                "l" => {
                                    return Some(text_editor::Binding::SelectLine);
                                }
                                "g" => {
                                    return Some(text_editor::Binding::Custom(
                                        Message::GotoLineOpen,
                                    ));
                                }
                                // Vim: Ctrl+R = redo in Normal mode
                                "r" if matches!(
                                    vim_mode,
                                    vim::Mode::Normal | vim::Mode::Visual | vim::Mode::VisualLine
                                ) =>
                                {
                                    return Some(text_editor::Binding::Custom(Message::VimKey(
                                        key.clone(),
                                        mods,
                                    )));
                                }
                                _ => {}
                            },
                            _ => {}
                        }
                    }

                    // PageUp/PageDown — all modes (vim doesn't handle these keys)
                    if let keyboard::Key::Named(named) = modified_key {
                        let page = ((vh / LINE_HEIGHT_PX) as usize).saturating_sub(2);
                        match named {
                            keyboard::key::Named::PageUp => {
                                return Some(text_editor::Binding::Custom(Message::PageUp(
                                    page,
                                    mods.shift(),
                                )));
                            }
                            keyboard::key::Named::PageDown => {
                                return Some(text_editor::Binding::Custom(Message::PageDown(
                                    page,
                                    mods.shift(),
                                )));
                            }
                            _ => {}
                        }
                    }

                    // Phase 2: Vim Normal/Visual — intercept all non-Ctrl keys
                    if matches!(
                        vim_mode,
                        vim::Mode::Normal | vim::Mode::Visual | vim::Mode::VisualLine
                    ) && !mods.command()
                    {
                        return Some(text_editor::Binding::Custom(Message::VimKey(
                            modified_key.clone(),
                            mods,
                        )));
                    }

                    // Phase 3: Insert mode — existing iced behavior
                    if let keyboard::Key::Named(named) = modified_key {
                        match named {
                            keyboard::key::Named::Tab => {
                                if mods.shift() {
                                    return Some(text_editor::Binding::Custom(Message::Edit(
                                        text_editor::Action::Edit(text_editor::Edit::Unindent),
                                    )));
                                }
                                return Some(text_editor::Binding::Custom(Message::Edit(
                                    text_editor::Action::Edit(text_editor::Edit::Indent),
                                )));
                            }
                            keyboard::key::Named::Enter
                                if !mods.command() && !mods.shift() && !mods.alt() =>
                            {
                                return Some(text_editor::Binding::Custom(Message::AutoIndent));
                            }
                            keyboard::key::Named::ArrowUp
                                if mods.alt() && !mods.command() && !mods.shift() =>
                            {
                                return Some(text_editor::Binding::Custom(Message::MoveLineUp));
                            }
                            keyboard::key::Named::ArrowDown
                                if mods.alt() && !mods.command() && !mods.shift() =>
                            {
                                return Some(text_editor::Binding::Custom(Message::MoveLineDown));
                            }
                            keyboard::key::Named::ArrowUp if !mods.alt() && !mods.command() => {
                                if cursor_line == 0 {
                                    return Some(if mods.shift() {
                                        text_editor::Binding::Select(text_editor::Motion::Home)
                                    } else {
                                        text_editor::Binding::Move(text_editor::Motion::Home)
                                    });
                                }
                            }
                            keyboard::key::Named::ArrowDown if !mods.alt() && !mods.command() => {
                                if cursor_line >= last_line {
                                    return Some(if mods.shift() {
                                        text_editor::Binding::Select(text_editor::Motion::End)
                                    } else {
                                        text_editor::Binding::Move(text_editor::Motion::End)
                                    });
                                }
                            }
                            _ => {}
                        }
                    }

                    text_editor::Binding::from_key_press(key_press)
                })
                .style(move |_theme, _status| text_editor::Style {
                    background: Background::Color(bg_base),
                    border: Border::default().width(0),
                    placeholder: text_muted,
                    value: text_main,
                    selection: Color {
                        a: if vim_mode == vim::Mode::Normal {
                            0.5
                        } else {
                            0.3
                        },
                        ..primary
                    },
                });

            // Workaround for iced-rs/iced#3227 — remove when merged
            let editor = mouse_area(editor)
                .on_move(Message::EditorMouseMove)
                .on_release(Message::MulticlickReleased)
                .on_middle_press(Message::MiddleClickPaste);

            scrollable(row![line_numbers, gutter_line, editor].width(Length::Fill))
                .id(SCROLLABLE_ID.clone())
                .on_scroll(Message::Scrolled)
                .height(Length::Fill)
                .width(Length::Fill)
                .into()
        });

        // ── Status bar ───────────────────────────────────────────────────
        let cursor = tab.content.cursor();
        let ln = cursor.position.line + 1;
        let col = cursor.position.column + 1;
        let name = tab.display_name();

        let file_label = name.into_owned();

        let wrap_label = if self.word_wrap { "Wrap" } else { "NoWrap" };

        let mode_label = self.vim.mode.label();
        let pending = self.vim.pending_display();

        let mut status_row = row![].align_y(iced::Alignment::Center);

        // Mode indicator (accent color when not in Insert mode)
        if self.vim.mode != vim::Mode::Insert {
            status_row = status_row
                .push(text(mode_label).size(12).color(primary))
                .push(text("  \u{00b7}  ").size(12).color(text_muted));
        }

        status_row = status_row.push(text(file_label).size(12).color(text_muted));
        status_row = status_row.push(iced::widget::space::horizontal());

        // Pending vim command
        if !pending.is_empty() {
            status_row = status_row
                .push(text(pending).size(12).color(primary))
                .push(text("  ").size(12));
        }

        status_row = status_row
            .push(
                text(format!("Ln {ln}, Col {col}"))
                    .size(12)
                    .color(text_muted),
            )
            .push(text("  \u{00b7}  ").size(12).color(text_muted))
            .push(
                button(text(wrap_label).size(12).color(text_muted))
                    .style(flat_btn(bg_weak))
                    .on_press(Message::ToggleWordWrap)
                    .padding(0),
            )
            .push(text("  \u{00b7}  ").size(12).color(text_muted))
            .push(text("UTF-8").size(12).color(text_muted));

        let status_bar = container(status_row)
            .padding(Padding {
                top: 4.0,
                bottom: 4.0,
                left: 14.0,
                right: 14.0,
            })
            .width(Length::Fill)
            .style(solid_bg(bg_weak));

        // ── Go-to-line bar (conditional) ────────────────────────────────
        let goto_bar = self.goto_line.as_ref().map(|goto_text| {
            container(
                row![
                    text("Go to line:").size(13).color(text_muted),
                    text_input("Line number", goto_text)
                        .id(GOTO_LINE_ID.clone())
                        .on_input(Message::GotoLineChanged)
                        .on_submit(Message::GotoLineSubmit)
                        .font(editor_font)
                        .size(13)
                        .width(100),
                    button(text("\u{00d7}").size(14).color(text_muted))
                        .style(flat_btn(bg_weak))
                        .on_press(Message::GotoLineClose)
                        .padding(Padding::from([4, 8])),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            )
            .padding(Padding::from([4, 8]))
            .style(solid_bg(bg_strong))
        });

        // ── Root ─────────────────────────────────────────────────────────
        let mut overlay_col: iced::widget::Column<'_, Message> = column![].spacing(0);
        if let Some(bar) = find_bar {
            overlay_col = overlay_col.push(bar);
        }
        if let Some(bar) = goto_bar {
            overlay_col = overlay_col.push(bar);
        }

        let content_area = column![editor_area, status_bar];
        let stacked = stack![content_area, right(opaque(overlay_col))];

        column![tab_bar, stacked].into()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = vec![event::listen_with(handle_key)];

        if self.should_poll_autosave() {
            subscriptions
                .push(iced::time::every(Duration::from_millis(500)).map(|_| Message::AutosaveTick));
        }

        if self.should_refresh_find_idle() {
            subscriptions.push(
                iced::time::every(Duration::from_millis(50)).map(|_| Message::FindRefreshTick),
            );
        }

        Subscription::batch(subscriptions)
    }
}

pub(crate) fn wrapped_cols(viewport_width: f32, line_count: usize) -> Option<usize> {
    if viewport_width <= 0.0 {
        return None;
    }

    Some(viewport::wrap_columns(
        viewport_width,
        EDITOR_FONT.char_width,
        line_count,
    ))
}

// ── Vim text‑range helpers ──────────────────────────────────────────────────

pub(crate) fn extract_text_range(
    lines: &[String],
    from: &text_editor::Position,
    to: &text_editor::Position,
) -> String {
    if from.line >= lines.len() || to.line >= lines.len() {
        return String::new();
    }
    if from.line == to.line {
        let chars: Vec<char> = lines[from.line].chars().collect();
        let start = from.column.min(chars.len());
        let end = (to.column + 1).min(chars.len());
        if start >= end {
            return String::new();
        }
        chars[start..end].iter().collect()
    } else {
        let mut result = String::new();
        let first: Vec<char> = lines[from.line].chars().collect();
        result.extend(&first[from.column.min(first.len())..]);
        for line in lines.iter().take(to.line).skip(from.line + 1) {
            result.push('\n');
            result.push_str(line);
        }
        result.push('\n');
        let last: Vec<char> = lines[to.line].chars().collect();
        result.extend(&last[..(to.column + 1).min(last.len())]);
        result
    }
}

pub(crate) fn remove_text_range(
    lines: &mut Vec<String>,
    from: &text_editor::Position,
    to: &text_editor::Position,
) {
    if from.line >= lines.len() || to.line >= lines.len() {
        return;
    }
    if from.line == to.line {
        let chars: Vec<char> = lines[from.line].chars().collect();
        let start = from.column.min(chars.len());
        let end = (to.column + 1).min(chars.len());
        let remaining: String = chars[..start].iter().chain(chars[end..].iter()).collect();
        lines[from.line] = remaining;
    } else {
        let first: Vec<char> = lines[from.line].chars().collect();
        let last: Vec<char> = lines[to.line].chars().collect();
        let prefix: String = first[..from.column.min(first.len())].iter().collect();
        let suffix: String = last[(to.column + 1).min(last.len())..].iter().collect();
        lines[from.line] = format!("{prefix}{suffix}");
        if from.line < to.line {
            lines.drain((from.line + 1)..=to.line);
        }
    }
}

pub(crate) fn selection_text(
    content: &text_editor::Content,
    vim_mode: vim::Mode,
) -> Option<String> {
    if vim_mode == vim::Mode::Normal && is_block_cursor_selection(&content.cursor()) {
        return None;
    }
    content
        .selection()
        .filter(|selection| !selection.is_empty())
}

pub(crate) fn is_block_cursor_selection(cursor: &text_editor::Cursor) -> bool {
    let Some(selection) = cursor.selection else {
        return false;
    };
    selection.line == cursor.position.line && selection.column == cursor.position.column + 1
}

pub(crate) fn collapse_selection_to_caret(content: &mut text_editor::Content) {
    let pos = content.cursor().position;
    // Iced keeps the old selection when moved with `selection: None`; use an
    // empty selection to clear the synthetic Normal-mode block cursor.
    content.move_to(text_editor::Cursor {
        position: pos,
        selection: Some(pos),
    });
}

pub(crate) fn reveal_intent_for_edit_action(action: &text_editor::Action) -> RevealIntent {
    match action {
        text_editor::Action::Move(_)
        | text_editor::Action::Select(_)
        | text_editor::Action::Edit(_) => RevealIntent::RevealCaret,
        text_editor::Action::SelectWord
        | text_editor::Action::SelectLine
        | text_editor::Action::SelectAll
        | text_editor::Action::Click(_)
        | text_editor::Action::Drag(_)
        | text_editor::Action::Scroll { .. } => RevealIntent::None,
    }
}

// ── Keyboard shortcuts ───────────────────────────────────────────────────────

fn handle_key(event: iced::Event, status: event::Status, _id: iced::window::Id) -> Option<Message> {
    if matches!(
        event,
        iced::Event::Window(iced::window::Event::CloseRequested)
    ) {
        return Some(Message::Quit);
    }

    // Track modifier state regardless of whether a widget consumed the event
    if let iced::Event::Keyboard(keyboard::Event::ModifiersChanged(mods)) = event {
        return Some(Message::ModifiersChanged(mods));
    }

    // Escape closes overlays even if a text_input captured the event
    if let iced::Event::Keyboard(keyboard::Event::KeyPressed {
        key: keyboard::Key::Named(keyboard::key::Named::Escape),
        ..
    }) = &event
    {
        return Some(Message::GotoLineClose);
    }

    if status != event::Status::Ignored {
        return None;
    }

    let iced::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) = event else {
        return None;
    };

    // Alt+Z — word wrap toggle (VS Code convention)
    if modifiers.alt() && !modifiers.command() && !modifiers.shift() {
        if let keyboard::Key::Character(ref c) = key {
            if c.as_str() == "z" {
                return Some(Message::ToggleWordWrap);
            }
        }
    }

    // Ctrl / Cmd shortcuts
    if !modifiers.command() {
        return None;
    }

    match &key {
        keyboard::Key::Character(c) => match c.as_str() {
            "n" => Some(Message::New),
            "o" => Some(Message::Open),
            "s" if modifiers.shift() => Some(Message::SaveAs),
            "s" => Some(Message::Save),
            "w" => Some(Message::CloseActiveTab),
            "q" => Some(Message::Quit),
            "f" => Some(Message::FindOpen),
            "h" => Some(Message::FindOpenReplace),
            "z" if modifiers.shift() => Some(Message::Redo),
            "z" => Some(Message::Undo),
            "g" => Some(Message::GotoLineOpen),
            "/" => Some(Message::ToggleComment),
            _ => None,
        },
        keyboard::Key::Named(keyboard::key::Named::Tab) => Some(if modifiers.shift() {
            Message::PrevTab
        } else {
            Message::NextTab
        }),
        _ => None,
    }
}

fn gutter_line_at(point: Point) -> usize {
    ((point.y - EDITOR_PAD).max(0.0) / LINE_HEIGHT_PX).floor() as usize
}

pub(crate) fn editor_pointer_cell(point: Point) -> EditorPointerCell {
    let x = (point.x - EDITOR_PAD).max(0.0);
    let y = (point.y - EDITOR_PAD).max(0.0);

    EditorPointerCell {
        column: (x / EDITOR_FONT.char_width).floor() as usize,
        row: (y / LINE_HEIGHT_PX).floor() as usize,
    }
}

pub(crate) fn content_point_for_cell(cell: EditorPointerCell) -> Point {
    Point::new(
        cell.column as f32 * EDITOR_FONT.char_width,
        cell.row as f32 * LINE_HEIGHT_PX,
    )
}

// ── File I/O ─────────────────────────────────────────────────────────────────

async fn open_file() -> Result<(PathBuf, String), Error> {
    let handle = rfd::AsyncFileDialog::new()
        .add_filter(
            "Text",
            &["txt", "md", "rs", "py", "toml", "yaml", "json", "sh"],
        )
        .add_filter("All files", &["*"])
        .pick_file()
        .await
        .ok_or(Error::DialogClosed)?;

    let path = handle.path().to_path_buf();
    let body = std::fs::read_to_string(&path).map_err(|_| Error::Io)?;
    Ok((path, body))
}

async fn save_file(path: PathBuf, body: String) -> Result<PathBuf, Error> {
    std::fs::write(&path, &body).map_err(|_| Error::Io)?;
    Ok(path)
}

async fn save_file_as(body: String) -> Result<PathBuf, Error> {
    let handle = rfd::AsyncFileDialog::new()
        .set_file_name("untitled.txt")
        .save_file()
        .await
        .ok_or(Error::DialogClosed)?;

    let path = handle.path().to_path_buf();
    std::fs::write(&path, &body).map_err(|_| Error::Io)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(line: usize, column: usize) -> text_editor::Position {
        text_editor::Position { line, column }
    }

    // Removed: collapse_selection_to_caret_prevents_insert_replacement
    // → covered by tests/selection.rs::insert_from_normal_does_not_replace_block_cursor

    // Removed: selection_text_ignores_block_cursor_but_keeps_real_selections
    // → covered by tests/selection.rs::normal_mode_selection_is_none + visual_mode_shows_selection

    #[test]
    fn transform_case_range_handles_multiline_spans() {
        let mut lines = vec!["ABC".to_string(), "DeF".to_string()];
        editor_ops::transform_case_range(&mut lines, 0, 1, 1, 1, false);
        assert_eq!(lines, vec!["Abc".to_string(), "deF".to_string()]);
    }

    #[test]
    fn scroll_actions_do_not_request_reveal_but_moves_do() {
        assert_eq!(
            reveal_intent_for_edit_action(&text_editor::Action::Scroll { lines: 3 }),
            RevealIntent::None
        );
        assert_eq!(
            reveal_intent_for_edit_action(&text_editor::Action::Move(text_editor::Motion::Down)),
            RevealIntent::RevealCaret
        );
    }

    #[test]
    fn page_down_requests_reveal() {
        let mut app = App::test("one\ntwo\nthree\nfour\nfive");

        let result = app.update_inner(Message::PageDown(2, false));

        assert_eq!(result.reveal, RevealIntent::RevealCaret);
        assert_eq!(app.tabs[0].content.cursor().position.line, 2);
    }

    #[test]
    fn noop_tab_actions_do_not_request_reveal() {
        let mut app = App::test("one");

        assert_eq!(
            app.update_inner(Message::TabSelect(0)).reveal,
            RevealIntent::None
        );
        assert_eq!(
            app.update_inner(Message::NextTab).reveal,
            RevealIntent::None
        );
        assert_eq!(
            app.update_inner(Message::PrevTab).reveal,
            RevealIntent::None
        );
    }

    #[test]
    fn goto_line_submit_requests_reveal_on_valid_input() {
        let mut app = App::test("one\ntwo\nthree");
        app.goto_line = Some("3".to_string());

        let result = app.update_inner(Message::GotoLineSubmit);

        assert_eq!(result.reveal, RevealIntent::RevealCaret);
        assert_eq!(app.tabs[0].content.cursor().position, pos(2, 0));
    }

    #[test]
    fn find_next_requests_reveal() {
        let mut app = App::test("foo\nbar\nfoo");
        app.find.query = "foo".to_string();
        app.find.compute_matches("foo\nbar\nfoo");

        let result = app.update_inner(Message::FindNext);

        assert_eq!(result.reveal, RevealIntent::RevealCaret);
        assert_eq!(app.tabs[0].content.cursor().position, pos(2, 3));
    }

    #[test]
    fn edits_mark_find_matches_dirty_until_idle_refresh() {
        let mut app = App::test("foo\nbar\nfoo");
        app.find.visible = true;
        app.find.query = "foo".to_string();
        app.reindex_find_matches();

        let result = app.update_inner(Message::Edit(text_editor::Action::Edit(
            text_editor::Edit::Insert('x'),
        )));

        assert_eq!(result.reveal, RevealIntent::RevealCaret);
        assert!(app.find.is_dirty());
        assert!(app.find_matches_stale());

        app.find
            .mark_dirty_at(Instant::now() - Duration::from_millis(250));

        let refresh = app.update_inner(Message::FindRefreshTick);

        assert_eq!(refresh.reveal, RevealIntent::None);
        assert!(!app.find.is_dirty());
        assert_eq!(app.find.indexed_revision(), Some(app.tabs[0].revision()));
    }

    #[test]
    fn idle_find_refresh_preserves_selected_match() {
        let mut app = App::test("foo\nbar\nfoo");
        app.find.visible = true;
        app.find.query = "foo".to_string();
        app.reindex_find_matches();
        app.find.current = 1;
        app.find.navigate_to_current(&mut app.tabs[0].content);
        app.find
            .mark_dirty_at(Instant::now() - Duration::from_millis(250));

        let result = app.update_inner(Message::FindRefreshTick);

        assert_eq!(result.reveal, RevealIntent::None);
        assert_eq!(app.find.current, 1);
        assert_eq!(app.tabs[0].content.cursor().position, pos(2, 3));
    }

    #[test]
    fn replace_one_with_stale_matches_uses_visible_selection() {
        let mut app = App::test("foo foo");
        app.find.query = "foo".to_string();
        app.find.replacement = "bar".to_string();
        app.reindex_find_matches();
        app.find.current = 1;
        app.find.navigate_to_current(&mut app.tabs[0].content);
        app.find.finish_reindex(0);
        app.find.mark_dirty();

        let result = app.update_inner(Message::ReplaceOne);

        assert_eq!(result.reveal, RevealIntent::RevealCaret);
        assert_eq!(app.tabs[0].content.text(), "foo bar");
    }

    #[test]
    fn timers_only_run_when_background_work_exists() {
        let mut app = App::test("foo");

        assert!(!app.should_poll_autosave());
        assert!(!app.should_refresh_find_idle());

        app.needs_autosave = true;
        assert!(app.should_poll_autosave());

        app.needs_autosave = false;
        app.find.visible = true;
        app.find.query = "foo".to_string();
        app.find.mark_dirty();
        assert!(app.should_refresh_find_idle());

        app.find.visible = false;
        assert!(!app.should_refresh_find_idle());
    }

    #[test]
    fn vim_snapshot_reuses_cached_lines_until_revision_changes() {
        let mut app = App::test("one\ntwo");

        let first = app.vim_snapshot().lines;

        app.jump_to_line(1, false);
        let second = app.vim_snapshot().lines;

        assert!(Arc::ptr_eq(&first, &second));

        app.tabs[0].content = text_editor::Content::with_text("one\ntwo\nthree");
        app.tabs[0].touch_content();

        let third = app.vim_snapshot().lines;

        assert!(!Arc::ptr_eq(&first, &third));
    }

    #[test]
    fn pointer_state_only_changes_after_crossing_snapped_boundaries() {
        let mut app = App::test("one\ntwo");

        app.update_inner(Message::GutterMove(Point::new(
            8.0,
            EDITOR_PAD + LINE_HEIGHT_PX * 0.25,
        )));
        assert_eq!(app.gutter_hover_line, Some(0));

        app.update_inner(Message::GutterMove(Point::new(
            24.0,
            EDITOR_PAD + LINE_HEIGHT_PX * 0.9,
        )));
        assert_eq!(app.gutter_hover_line, Some(0));

        app.update_inner(Message::EditorMouseMove(Point::new(
            EDITOR_PAD + EDITOR_FONT.char_width * 0.2,
            EDITOR_PAD + LINE_HEIGHT_PX * 0.2,
        )));
        assert_eq!(
            app.editor_pointer_cell,
            Some(EditorPointerCell { column: 0, row: 0 })
        );

        app.update_inner(Message::EditorMouseMove(Point::new(
            EDITOR_PAD + EDITOR_FONT.char_width * 0.9,
            EDITOR_PAD + LINE_HEIGHT_PX * 0.8,
        )));
        assert_eq!(
            app.editor_pointer_cell,
            Some(EditorPointerCell { column: 0, row: 0 })
        );

        app.update_inner(Message::EditorMouseMove(Point::new(
            EDITOR_PAD + EDITOR_FONT.char_width * 1.2,
            EDITOR_PAD + LINE_HEIGHT_PX * 1.1,
        )));
        assert_eq!(
            app.editor_pointer_cell,
            Some(EditorPointerCell { column: 1, row: 1 })
        );
    }

    // Removed: move_line_down_with_identical_neighbors_still_moves_cursor
    // → covered by tests/line_ops.rs::move_line_down_with_identical_lines_still_moves_cursor

    // Removed: toggle_comment_on_blank_line_can_still_move_cursor_without_editing
    // → covered by tests/line_ops.rs::toggle_comment_on_blank_line_moves_cursor

    #[test]
    fn layout_cache_only_rebuilds_when_wrap_width_changes() {
        let mut app = App::test("abcdefghij");
        let height = 60.0;
        let width_narrow = viewport::line_number_gutter_width(1, EDITOR_FONT.char_width)
            + viewport::GUTTER_SEPARATOR_WIDTH
            + EDITOR_PAD
            + viewport::EDITOR_RIGHT_PAD
            + EDITOR_FONT.char_width * 4.25;
        let width_wide = viewport::line_number_gutter_width(1, EDITOR_FONT.char_width)
            + viewport::GUTTER_SEPARATOR_WIDTH
            + EDITOR_PAD
            + viewport::EDITOR_RIGHT_PAD
            + EDITOR_FONT.char_width * 6.25;

        app.viewport = ViewportState::from_metrics(
            width_narrow,
            height,
            viewport::content_height(height, 3),
            0.0,
        );
        let narrow_cols = app
            .active_wrap_cols()
            .expect("narrow viewport should produce wrap columns");
        app.ensure_active_layout_cache();
        assert!(app.tabs[0].layout_cache_for(narrow_cols).is_some());

        app.viewport = ViewportState::from_metrics(
            width_narrow,
            height,
            viewport::content_height(height, 3),
            40.0,
        );
        assert_eq!(app.active_wrap_cols(), Some(narrow_cols));
        assert!(app.tabs[0].layout_cache_for(narrow_cols).is_some());

        app.viewport = ViewportState::from_metrics(
            width_wide,
            height,
            viewport::content_height(height, 2),
            40.0,
        );
        let wide_cols = app
            .active_wrap_cols()
            .expect("wide viewport should produce wrap columns");
        assert_ne!(narrow_cols, wide_cols);

        app.ensure_active_layout_cache();

        assert!(app.tabs[0].layout_cache_for(wide_cols).is_some());
        assert!(app.tabs[0].layout_cache_for(narrow_cols).is_none());
    }

    #[test]
    fn vim_move_to_requests_reveal() {
        let mut app = App::test("one\ntwo\nthree");

        let result = app.execute_vim_commands(vec![vim::VimCommand::MoveTo(pos(2, 1))]);

        assert_eq!(result.reveal, RevealIntent::RevealCaret);
        assert_eq!(app.tabs[0].content.cursor().position, pos(2, 1));
    }

    #[test]
    fn wrapped_caret_reveal_target_matches_layout_math() {
        let mut app = App::test("abcdefghij");
        let width = viewport::line_number_gutter_width(1, EDITOR_FONT.char_width)
            + viewport::GUTTER_SEPARATOR_WIDTH
            + EDITOR_PAD
            + viewport::EDITOR_RIGHT_PAD
            + EDITOR_FONT.char_width * 4.25;
        let height = 60.0;
        app.viewport =
            ViewportState::from_metrics(width, height, viewport::content_height(height, 3), 0.0);
        app.tabs[0].content.move_to(text_editor::Cursor {
            position: pos(0, 9),
            selection: None,
        });

        assert_eq!(app.caret_reveal_target(), Some(40.0));
    }

    #[test]
    fn finish_update_syncs_viewport_scroll_for_reveal() {
        let mut app = App::test("abcdefghij");
        let width = viewport::line_number_gutter_width(1, EDITOR_FONT.char_width)
            + viewport::GUTTER_SEPARATOR_WIDTH
            + EDITOR_PAD
            + viewport::EDITOR_RIGHT_PAD
            + EDITOR_FONT.char_width * 4.25;
        let height = 60.0;
        app.viewport =
            ViewportState::from_metrics(width, height, viewport::content_height(height, 3), 0.0);
        app.tabs[0].content.move_to(text_editor::Cursor {
            position: pos(0, 9),
            selection: None,
        });

        let _ = app.finish_update(UpdateResult::reveal(Task::none()));

        assert_eq!(app.viewport.scroll_y(), 40.0);
    }

    #[test]
    fn tab_switch_reveal_uses_active_document_height() {
        let width = viewport::line_number_gutter_width(10, EDITOR_FONT.char_width)
            + viewport::GUTTER_SEPARATOR_WIDTH
            + EDITOR_PAD
            + viewport::EDITOR_RIGHT_PAD
            + EDITOR_FONT.char_width * 20.0;
        let height = 60.0;
        let mut app = App {
            tabs: vec![
                Tab::from_path(PathBuf::from("/tmp/short.txt"), "short"),
                Tab::from_path(
                    PathBuf::from("/tmp/long.txt"),
                    "1\n2\n3\n4\n5\n6\n7\n8\n9\n10",
                ),
            ],
            active: 0,
            window_title: None,
            gutter_hover_line: None,
            find: FindState::new(),
            word_wrap: true,
            scratchpad_dir: PathBuf::from("/tmp"),
            needs_autosave: false,
            shift_held: false,
            multiclick_drag: false,
            editor_pointer_cell: None,
            goto_line: None,
            vim: vim::VimState::new(),
            viewport: ViewportState::from_metrics(
                width,
                height,
                viewport::content_height(height, 1),
                0.0,
            ),
            clipboard: Box::new(NullClipboard),
            fs: Box::new(NullFilesystem),
        };
        app.tabs[1].content.move_to(text_editor::Cursor {
            position: pos(9, 0),
            selection: None,
        });

        let result = app.update_inner(Message::TabSelect(1));

        assert_eq!(result.reveal, RevealIntent::RevealCaret);
        assert_eq!(app.caret_reveal_target(), Some(180.0));
    }
}

// ── Entry point ──────────────────────────────────────────────────────────────
