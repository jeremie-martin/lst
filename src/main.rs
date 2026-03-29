mod find;
mod highlight;
mod style;
mod tab;
mod vim;

use find::FindState;
use style::{flat_btn, solid_bg, EDITOR_FONT, FONT_SIZE, LINE_HEIGHT_PX};
use tab::{EditKind, Tab};

use iced::event;
use iced::keyboard;
use iced::mouse;
use iced::widget::{
    button, column, container, mouse_area, opaque, responsive, right, row, scrollable, stack, text,
    text_editor, text_input, Space,
};
use iced::{
    Background, Border, Color, Element, Font, Length, Padding, Pixels, Point, Subscription, Task,
    Theme,
};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

fn editor_id() -> iced::widget::Id {
    iced::widget::Id::new("lst-editor")
}

static EDITOR_ID: LazyLock<iced::widget::Id> = LazyLock::new(editor_id);
static FIND_INPUT_ID: LazyLock<iced::widget::Id> =
    LazyLock::new(|| iced::widget::Id::new("lst-find"));
static GOTO_LINE_ID: LazyLock<iced::widget::Id> =
    LazyLock::new(|| iced::widget::Id::new("lst-goto-line"));

// ── App ──────────────────────────────────────────────────────────────────────

struct App {
    tabs: Vec<Tab>,
    active: usize,
    window_title: Option<String>,
    gutter_mouse_y: f32,
    find: FindState,
    word_wrap: bool,
    scratchpad_dir: PathBuf,
    needs_autosave: bool,
    shift_held: bool, // iced's Action::Click doesn't carry modifier state; track externally
    goto_line: Option<String>,
    vim: vim::VimState,
}

#[derive(Debug, Clone)]
enum Message {
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
}

#[derive(Debug, Clone)]
enum Error {
    DialogClosed,
    Io,
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

fn resolve_scratchpad_dir(cli_override: Option<PathBuf>) -> PathBuf {
    let dir = cli_override.unwrap_or_else(|| {
        let Some(home) = std::env::var_os("HOME") else {
            eprintln!("lst: HOME environment variable not set");
            std::process::exit(1);
        };
        PathBuf::from(home).join(".local/share/lst")
    });
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!(
            "lst: failed to create scratchpad directory {}: {e}",
            dir.display()
        );
        std::process::exit(1);
    }
    dir
}

fn generate_scratchpad_path(dir: &Path) -> PathBuf {
    use chrono::Local;
    let name = Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
    let path = dir.join(format!("{name}.md"));
    if !path.exists() {
        return path;
    }
    for i in 1.. {
        let path = dir.join(format!("{name}_{i}.md"));
        if !path.exists() {
            return path;
        }
    }
    unreachable!()
}

fn create_scratchpad_tab(dir: &Path) -> Tab {
    let path = generate_scratchpad_path(dir);
    Tab::new_scratchpad(path)
}

impl App {
    fn boot() -> (Self, Task<Message>) {
        let args = parse_args();
        let scratchpad_dir = resolve_scratchpad_dir(args.scratchpad_dir);

        let mut tabs: Vec<Tab> = args
            .files
            .into_iter()
            .filter_map(|path| {
                let body = std::fs::read_to_string(&path).ok()?;
                Some(Tab::from_path(path.canonicalize().unwrap_or(path), &body))
            })
            .collect();

        if tabs.is_empty() {
            tabs.push(create_scratchpad_tab(&scratchpad_dir));
        }

        (
            Self {
                tabs,
                active: 0,
                window_title: args.window_title,
                gutter_mouse_y: 0.0,
                find: FindState::new(),
                word_wrap: true,
                scratchpad_dir,
                needs_autosave: false,
                shift_held: false,
                goto_line: None,
                vim: vim::VimState::new(),
            },
            iced::widget::operation::focus(EDITOR_ID.clone()),
        )
    }

    fn title(&self) -> String {
        if let Some(title) = &self.window_title {
            return title.clone();
        }
        let tab = &self.tabs[self.active];
        match &tab.path {
            Some(p) => format!("{} \u{2014} lst", p.display()),
            None => format!("{} \u{2014} lst", tab.display_name()),
        }
    }

    fn theme(&self) -> Theme {
        Theme::CatppuccinMocha
    }

    fn close_tab(&mut self, i: usize) -> Task<Message> {
        if i >= self.tabs.len() {
            return Task::none();
        }
        let tab = &self.tabs[i];
        if tab.is_scratchpad && tab.content.text().trim().is_empty() {
            if let Some(p) = &tab.path {
                let _ = std::fs::remove_file(p);
            }
        }
        if self.tabs.len() == 1 {
            return self.exit_with_clipboard();
        }
        self.tabs.remove(i);
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        } else if self.active > i {
            self.active -= 1;
        }
        Task::none()
    }

    fn exit_with_clipboard(&self) -> Task<Message> {
        let text = self.tabs[self.active].content.text();
        if !text.trim().is_empty() {
            copy_to_clipboard(&text);
        }
        iced::exit()
    }

    fn refresh_find_matches(&mut self) {
        if self.find.visible {
            self.find
                .compute_matches(&self.tabs[self.active].content.text());
        }
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
        tab.modified = true;
        self.needs_autosave = true;
        self.refresh_find_matches();
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
    }

    fn vim_snapshot(&self) -> vim::TextSnapshot {
        let tab = &self.tabs[self.active];
        let text = tab.content.text();
        let cursor = tab.content.cursor().position;
        vim::TextSnapshot {
            lines: text.split('\n').map(String::from).collect(),
            cursor,
        }
    }

    fn open_find(&mut self, show_replace: bool) -> Task<Message> {
        self.find.visible = true;
        self.find.show_replace = show_replace;
        if let Some(sel) = self.tabs[self.active].content.selection() {
            if !sel.contains('\n') {
                self.find.query = sel;
            }
        }
        self.find
            .compute_matches(&self.tabs[self.active].content.text());
        if !self.find.matches.is_empty() {
            let pos = self.tabs[self.active].content.cursor().position;
            self.find.find_nearest(&pos);
        }
        iced::widget::operation::focus(FIND_INPUT_ID.clone())
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Edit(action) => {
                // Shift+Click: extend selection instead of placing cursor
                if let text_editor::Action::Click(point) = &action {
                    if self.shift_held {
                        self.tabs[self.active]
                            .content
                            .perform(text_editor::Action::Drag(*point));
                        return Task::none();
                    }
                }

                let is_edit = matches!(action, text_editor::Action::Edit(_));
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
                if is_edit {
                    self.tabs[self.active].modified = true;
                    self.needs_autosave = true;
                    self.refresh_find_matches();
                }
                Task::none()
            }

            Message::TabSelect(i) => {
                if i < self.tabs.len() {
                    self.active = i;
                }
                Task::none()
            }

            Message::TabClose(i) => self.close_tab(i),

            Message::CloseActiveTab => self.close_tab(self.active),

            Message::New => {
                self.tabs.push(create_scratchpad_tab(&self.scratchpad_dir));
                self.active = self.tabs.len() - 1;
                Task::none()
            }

            Message::Open => Task::perform(open_file(), Message::Opened),

            Message::Opened(Ok((path, body))) => {
                if self.tabs.len() == 1
                    && self.tabs[0].is_scratchpad
                    && self.tabs[0].content.text().trim().is_empty()
                {
                    if let Some(old_path) = &self.tabs[0].path {
                        let _ = std::fs::remove_file(old_path);
                    }
                    self.tabs[0] = Tab::from_path(path, &body);
                } else {
                    self.tabs.push(Tab::from_path(path, &body));
                    self.active = self.tabs.len() - 1;
                }
                Task::none()
            }
            Message::Opened(Err(_)) => Task::none(),

            Message::Save => {
                let tab = &self.tabs[self.active];
                let body = tab.content.text();
                match tab.path.clone() {
                    Some(path) => Task::perform(save_file(path, body), Message::Saved),
                    None => Task::perform(save_file_as(body), Message::Saved),
                }
            }

            Message::SaveAs => {
                let body = self.tabs[self.active].content.text();
                Task::perform(save_file_as(body), Message::Saved)
            }

            Message::Saved(Ok(path)) => {
                let tab = &mut self.tabs[self.active];
                tab.path = Some(path);
                tab.modified = false;
                tab.is_scratchpad = false;
                Task::none()
            }
            Message::Saved(Err(_)) => Task::none(),

            Message::AutosaveTick => {
                if !self.needs_autosave {
                    return Task::none();
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
                    Task::none()
                } else {
                    Task::batch(saves)
                }
            }

            Message::AutosaveComplete(Ok(path)) => {
                for tab in &mut self.tabs {
                    if tab.path.as_ref() == Some(&path) {
                        tab.modified = false;
                        break;
                    }
                }
                Task::none()
            }
            Message::AutosaveComplete(Err(e)) => {
                eprintln!("lst: autosave failed: {e:?}");
                Task::none()
            }

            Message::GutterMove(point) => {
                self.gutter_mouse_y = point.y;
                Task::none()
            }

            Message::GutterClick => {
                const TOP_PAD: f32 = 8.0;
                let line = ((self.gutter_mouse_y - TOP_PAD) / LINE_HEIGHT_PX).max(0.0) as usize;

                let tab = &mut self.tabs[self.active];
                let line = line.min(tab.content.line_count().saturating_sub(1));
                let y = line as f32 * LINE_HEIGHT_PX;
                tab.content
                    .perform(text_editor::Action::Click(Point::new(0.0, y)));
                tab.content.perform(text_editor::Action::SelectLine);

                iced::widget::operation::focus(EDITOR_ID.clone())
            }

            Message::Quit => self.exit_with_clipboard(),

            // ── Undo / Redo ──────────────────────────────────────────────
            Message::Undo => {
                self.tabs[self.active].undo();
                self.refresh_find_matches();
                Task::none()
            }

            Message::Redo => {
                self.tabs[self.active].redo();
                self.refresh_find_matches();
                Task::none()
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
                tab.modified = true;
                self.needs_autosave = true;
                self.refresh_find_matches();
                Task::none()
            }

            // ── Find & Replace ───────────────────────────────────────────
            Message::FindOpen => {
                if self.find.visible {
                    self.find.visible = false;
                    return iced::widget::operation::focus(EDITOR_ID.clone());
                }
                self.open_find(false)
            }
            Message::FindOpenReplace => {
                if self.find.visible && self.find.show_replace {
                    self.find.visible = false;
                    return iced::widget::operation::focus(EDITOR_ID.clone());
                }
                self.open_find(true)
            }

            Message::FindClose => {
                if self.find.visible {
                    self.find.visible = false;
                    return iced::widget::operation::focus(EDITOR_ID.clone());
                }
                Task::none()
            }

            Message::FindQueryChanged(q) => {
                if q == self.find.query {
                    return Task::none();
                }
                self.find.query = q;
                self.find
                    .compute_matches(&self.tabs[self.active].content.text());
                if !self.find.matches.is_empty() {
                    let pos = self.tabs[self.active].content.cursor().position;
                    self.find.find_nearest(&pos);
                    self.find
                        .navigate_to_current(&mut self.tabs[self.active].content);
                }
                Task::none()
            }

            Message::FindReplaceChanged(r) => {
                self.find.replacement = r;
                Task::none()
            }

            Message::FindNext => {
                self.find.next();
                self.find
                    .navigate_to_current(&mut self.tabs[self.active].content);
                iced::widget::operation::focus(EDITOR_ID.clone())
            }

            Message::FindPrev => {
                self.find.prev();
                self.find
                    .navigate_to_current(&mut self.tabs[self.active].content);
                iced::widget::operation::focus(EDITOR_ID.clone())
            }

            Message::ReplaceOne => {
                if self.find.matches.is_empty() {
                    return Task::none();
                }
                let tab = &mut self.tabs[self.active];
                tab.push_undo_snapshot(EditKind::Other, true);
                self.find.navigate_to_current(&mut tab.content);
                let replacement = Arc::new(self.find.replacement.clone());
                tab.content
                    .perform(text_editor::Action::Edit(text_editor::Edit::Paste(
                        replacement,
                    )));
                tab.modified = true;
                self.needs_autosave = true;
                // Advance cursor past the replacement so we don't re-match it
                let cursor_after = tab.content.cursor().position;
                self.find.compute_matches(&tab.content.text());
                if !self.find.matches.is_empty() {
                    self.find.find_nearest(&cursor_after);
                    self.find
                        .navigate_to_current(&mut self.tabs[self.active].content);
                }
                Task::none()
            }

            Message::ReplaceAll => {
                if self.find.matches.is_empty() || self.find.query.is_empty() {
                    return Task::none();
                }
                let tab = &mut self.tabs[self.active];
                tab.push_undo_snapshot(EditKind::Other, true);
                let cursor_pos = tab.content.cursor().position;
                let new_text = tab
                    .content
                    .text()
                    .replace(&self.find.query, &self.find.replacement);
                self.rebuild_content(&new_text, cursor_pos.line, cursor_pos.column);
                Task::none()
            }

            // ── Word Wrap ────────────────────────────────────────────────
            Message::ToggleWordWrap => {
                self.word_wrap = !self.word_wrap;
                Task::none()
            }

            // ── Tab Reorder ──────────────────────────────────────────────
            Message::MoveTabLeft => {
                if self.active > 0 {
                    self.tabs.swap(self.active, self.active - 1);
                    self.active -= 1;
                }
                Task::none()
            }

            Message::MoveTabRight => {
                if self.active + 1 < self.tabs.len() {
                    self.tabs.swap(self.active, self.active + 1);
                    self.active += 1;
                }
                Task::none()
            }

            // ── Line Operations ─────────────────────────────────────────
            Message::DeleteLine => {
                let tab = &mut self.tabs[self.active];
                tab.push_undo_snapshot(EditKind::Other, true);
                let pos = tab.content.cursor().position;
                let full = tab.content.text();
                let mut lines: Vec<&str> = full.split('\n').collect();
                let target = pos.line.min(lines.len().saturating_sub(1));
                lines.remove(target);
                if lines.is_empty() {
                    lines.push("");
                }
                let new_text = lines.join("\n");
                self.rebuild_content(&new_text, target, pos.column);
                Task::none()
            }

            Message::MoveLineUp => {
                let tab = &mut self.tabs[self.active];
                let pos = tab.content.cursor().position;
                if pos.line == 0 {
                    return Task::none();
                }
                tab.push_undo_snapshot(EditKind::Other, true);
                let full = tab.content.text();
                let mut lines: Vec<&str> = full.split('\n').collect();
                let target = pos.line.min(lines.len().saturating_sub(1));
                lines.swap(target, target - 1);
                let new_text = lines.join("\n");
                self.rebuild_content(&new_text, target - 1, pos.column);
                Task::none()
            }

            Message::MoveLineDown => {
                let tab = &mut self.tabs[self.active];
                let pos = tab.content.cursor().position;
                let full = tab.content.text();
                let mut lines: Vec<&str> = full.split('\n').collect();
                let target = pos.line.min(lines.len().saturating_sub(1));
                if target + 1 >= lines.len() {
                    return Task::none();
                }
                tab.push_undo_snapshot(EditKind::Other, true);
                lines.swap(target, target + 1);
                let new_text = lines.join("\n");
                self.rebuild_content(&new_text, target + 1, pos.column);
                Task::none()
            }

            Message::DuplicateLine => {
                let tab = &mut self.tabs[self.active];
                tab.push_undo_snapshot(EditKind::Other, true);
                let pos = tab.content.cursor().position;
                let full = tab.content.text();
                let mut lines: Vec<&str> = full.split('\n').collect();
                let target = pos.line.min(lines.len().saturating_sub(1));
                let dup = lines[target].to_string();
                lines.insert(target + 1, &dup);
                let new_text = lines.join("\n");
                self.rebuild_content(&new_text, target + 1, pos.column);
                Task::none()
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
                Task::none()
            }

            Message::PageDown(lines, select) => {
                let pos = self.tabs[self.active].content.cursor().position;
                let last = self.tabs[self.active]
                    .content
                    .line_count()
                    .saturating_sub(1);
                self.jump_to_line((pos.line + lines).min(last), select);
                Task::none()
            }

            // ── Tab Cycling ─────────────────────────────────────────────
            Message::NextTab => {
                if self.tabs.len() > 1 {
                    self.active = (self.active + 1) % self.tabs.len();
                }
                Task::none()
            }

            Message::PrevTab => {
                if self.tabs.len() > 1 {
                    self.active = if self.active == 0 {
                        self.tabs.len() - 1
                    } else {
                        self.active - 1
                    };
                }
                Task::none()
            }

            // ── Go to Line ──────────────────────────────────────────────
            Message::GotoLineOpen => {
                if self.goto_line.is_some() {
                    self.goto_line = None;
                    return iced::widget::operation::focus(EDITOR_ID.clone());
                }
                self.goto_line = Some(String::new());
                iced::widget::operation::focus(GOTO_LINE_ID.clone())
            }

            Message::GotoLineClose => {
                if self.goto_line.is_some() {
                    self.goto_line = None;
                    return iced::widget::operation::focus(EDITOR_ID.clone());
                }
                // Also close find bar (Escape from subscription closes topmost overlay)
                if self.find.visible {
                    self.find.visible = false;
                    return iced::widget::operation::focus(EDITOR_ID.clone());
                }
                // Vim: Escape cascades into mode transitions
                let snapshot = self.vim_snapshot();
                let cursor = snapshot.cursor;
                let commands = self.vim.enter_normal_from_escape(cursor, &snapshot);
                self.execute_vim_commands(commands)
            }

            Message::GotoLineChanged(s) => {
                self.goto_line = Some(s);
                Task::none()
            }

            Message::GotoLineSubmit => {
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
                    }
                }
                self.goto_line = None;
                iced::widget::operation::focus(EDITOR_ID.clone())
            }

            // ── Modifier Tracking ───────────────────────────────────────
            Message::ModifiersChanged(mods) => {
                self.shift_held = mods.shift();
                Task::none()
            }

            // ── Vim ────────────────────────────────────────────────────
            Message::VimKey(ref key, mods) => {
                let snapshot = self.vim_snapshot();
                let commands = self.vim.handle_key(key, mods, &snapshot);
                self.execute_vim_commands(commands)
            }
        }
    }

    fn execute_vim_commands(&mut self, commands: Vec<vim::VimCommand>) -> Task<Message> {
        use vim::VimCommand;
        let mut task = Task::none();
        for cmd in commands {
            match cmd {
                VimCommand::Noop => {}
                VimCommand::MoveTo(p) => {
                    self.tabs[self.active].content.move_to(text_editor::Cursor {
                        position: p,
                        selection: None,
                    });
                }
                VimCommand::Select { anchor, head } => {
                    self.tabs[self.active].content.move_to(text_editor::Cursor {
                        position: head,
                        selection: Some(anchor),
                    });
                }
                VimCommand::DeleteRange { from, to } => {
                    let deleted = self.vim_delete_range(from, to);
                    self.vim.register = vim::Register::Char(deleted);
                }
                VimCommand::DeleteLines { first, last } => {
                    let deleted = self.vim_delete_lines(first, last);
                    self.vim.register = vim::Register::Line(deleted);
                }
                VimCommand::ChangeRange { from, to } => {
                    let deleted = self.vim_delete_range(from, to);
                    self.vim.register = vim::Register::Char(deleted);
                    self.vim.mode = vim::Mode::Insert;
                }
                VimCommand::ChangeLines { first, last } => {
                    let deleted = self.vim_change_lines(first, last);
                    self.vim.register = vim::Register::Line(deleted);
                    self.vim.mode = vim::Mode::Insert;
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
                    self.vim.mode = vim::Mode::Insert;
                }
                VimCommand::EnterNormal => {
                    self.vim.mode = vim::Mode::Normal;
                }
                VimCommand::PasteAfter => self.vim_paste(false),
                VimCommand::PasteBefore => self.vim_paste(true),
                VimCommand::OpenLineBelow => self.vim_open_line(false),
                VimCommand::OpenLineAbove => self.vim_open_line(true),
                VimCommand::JoinLines { count } => self.vim_join_lines(count),
                VimCommand::ReplaceChar(c) => self.vim_replace_char(c),
                VimCommand::Undo => {
                    self.tabs[self.active].undo();
                    self.refresh_find_matches();
                }
                VimCommand::Redo => {
                    self.tabs[self.active].redo();
                    self.refresh_find_matches();
                }
                VimCommand::OpenFind => {
                    task = self.open_find(false);
                }
                VimCommand::FindNext => {
                    self.find.next();
                    self.find
                        .navigate_to_current(&mut self.tabs[self.active].content);
                    task = iced::widget::operation::focus(EDITOR_ID.clone());
                }
                VimCommand::FindPrev => {
                    self.find.prev();
                    self.find
                        .navigate_to_current(&mut self.tabs[self.active].content);
                    task = iced::widget::operation::focus(EDITOR_ID.clone());
                }
            }
        }
        task
    }

    // ── Vim helpers ─────────────────────────────────────────────────────

    fn vim_delete_range(
        &mut self,
        from: text_editor::Position,
        to: text_editor::Position,
    ) -> String {
        let tab = &mut self.tabs[self.active];
        tab.push_undo_snapshot(EditKind::Other, true);
        let full = tab.content.text();
        let mut lines: Vec<String> = full.split('\n').map(String::from).collect();
        let deleted = extract_text_range(&lines, &from, &to);
        remove_text_range(&mut lines, &from, &to);
        let new_text = lines.join("\n");
        let cursor_col = from.column.min(
            lines
                .get(from.line)
                .map_or(0, |l| l.chars().count().saturating_sub(1)),
        );
        self.rebuild_content(&new_text, from.line, cursor_col);
        deleted
    }

    fn vim_delete_lines(&mut self, first: usize, last: usize) -> String {
        let tab = &mut self.tabs[self.active];
        tab.push_undo_snapshot(EditKind::Other, true);
        let full = tab.content.text();
        let mut lines: Vec<String> = full.split('\n').map(String::from).collect();
        let first = first.min(lines.len().saturating_sub(1));
        let last = last.min(lines.len().saturating_sub(1));
        let deleted: String = lines[first..=last].join("\n");
        lines.drain(first..=last);
        if lines.is_empty() {
            lines.push(String::new());
        }
        let new_text = lines.join("\n");
        let cursor_line = first.min(lines.len().saturating_sub(1));
        self.rebuild_content(&new_text, cursor_line, 0);
        deleted
    }

    fn vim_change_lines(&mut self, first: usize, last: usize) -> String {
        let tab = &mut self.tabs[self.active];
        tab.push_undo_snapshot(EditKind::Other, true);
        let full = tab.content.text();
        let mut lines: Vec<String> = full.split('\n').map(String::from).collect();
        let first = first.min(lines.len().saturating_sub(1));
        let last = last.min(lines.len().saturating_sub(1));
        let indent: String = lines[first]
            .chars()
            .take_while(|c| c.is_whitespace())
            .collect();
        let deleted: String = lines[first..=last].join("\n");
        lines.drain(first..=last);
        lines.insert(first, indent.clone());
        let new_text = lines.join("\n");
        self.rebuild_content(&new_text, first, indent.chars().count());
        deleted
    }

    fn vim_extract_range(&self, from: text_editor::Position, to: text_editor::Position) -> String {
        let full = self.tabs[self.active].content.text();
        let lines: Vec<String> = full.split('\n').map(String::from).collect();
        extract_text_range(&lines, &from, &to)
    }

    fn vim_extract_lines(&self, first: usize, last: usize) -> String {
        let full = self.tabs[self.active].content.text();
        let lines: Vec<String> = full.split('\n').map(String::from).collect();
        let first = first.min(lines.len().saturating_sub(1));
        let last = last.min(lines.len().saturating_sub(1));
        lines[first..=last].join("\n")
    }

    fn vim_paste(&mut self, before: bool) {
        let register = self.vim.register.clone();
        match register {
            vim::Register::Empty => {}
            vim::Register::Char(ref paste_text) => {
                let tab = &mut self.tabs[self.active];
                tab.push_undo_snapshot(EditKind::Other, true);
                let cursor = tab.content.cursor().position;
                let full = tab.content.text();
                let mut lines: Vec<String> = full.split('\n').map(String::from).collect();
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
                    let cursor_col = insert_col + paste_lines[0].chars().count().saturating_sub(1);
                    let new_text = lines.join("\n");
                    self.rebuild_content(&new_text, cursor.line, cursor_col);
                } else {
                    let first_new = format!("{prefix}{}", paste_lines[0]);
                    let last_new = format!("{}{suffix}", paste_lines.last().unwrap_or(&""));
                    let mut new_lines: Vec<String> = lines[..cursor.line].to_vec();
                    new_lines.push(first_new);
                    for pl in &paste_lines[1..paste_lines.len() - 1] {
                        new_lines.push(pl.to_string());
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
                    let new_text = new_lines.join("\n");
                    self.rebuild_content(&new_text, cursor_line, cursor_col);
                }
            }
            vim::Register::Line(ref paste_text) => {
                let tab = &mut self.tabs[self.active];
                tab.push_undo_snapshot(EditKind::Other, true);
                let cursor = tab.content.cursor().position;
                let full = tab.content.text();
                let mut lines: Vec<String> = full.split('\n').map(String::from).collect();
                let insert_at = if before { cursor.line } else { cursor.line + 1 };
                lines.splice(
                    insert_at..insert_at,
                    paste_text.split('\n').map(String::from),
                );
                let new_text = lines.join("\n");
                let indent = lines
                    .get(insert_at)
                    .map_or(0, |l| l.chars().take_while(|c| c.is_whitespace()).count());
                self.rebuild_content(&new_text, insert_at, indent);
            }
        }
    }

    fn vim_open_line(&mut self, above: bool) {
        let tab = &mut self.tabs[self.active];
        tab.push_undo_snapshot(EditKind::Other, true);
        let pos = tab.content.cursor().position;
        let full = tab.content.text();
        let mut lines: Vec<String> = full.split('\n').map(String::from).collect();
        let indent: String = lines.get(pos.line).map_or(String::new(), |l| {
            l.chars().take_while(|c| c.is_whitespace()).collect()
        });
        let idx = if above { pos.line } else { pos.line + 1 };
        lines.insert(idx, indent.clone());
        let new_text = lines.join("\n");
        self.rebuild_content(&new_text, idx, indent.chars().count());
        self.vim.mode = vim::Mode::Insert;
    }

    fn vim_join_lines(&mut self, count: usize) {
        let tab = &mut self.tabs[self.active];
        let pos = tab.content.cursor().position;
        let full = tab.content.text();
        let mut lines: Vec<String> = full.split('\n').map(String::from).collect();
        if pos.line + 1 >= lines.len() {
            return;
        }
        tab.push_undo_snapshot(EditKind::Other, true);
        let mut join_col = 0;
        for _ in 0..count {
            if pos.line + 1 >= lines.len() {
                break;
            }
            let current_trimmed = lines[pos.line].trim_end().to_string();
            join_col = current_trimmed.chars().count();
            let next = lines[pos.line + 1].trim_start().to_string();
            lines[pos.line] = if next.is_empty() {
                current_trimmed
            } else {
                format!("{current_trimmed} {next}")
            };
            lines.remove(pos.line + 1);
        }
        let new_text = lines.join("\n");
        self.rebuild_content(&new_text, pos.line, join_col);
    }

    fn vim_replace_char(&mut self, c: char) {
        let tab = &mut self.tabs[self.active];
        let pos = tab.content.cursor().position;
        let full = tab.content.text();
        let mut lines: Vec<String> = full.split('\n').map(String::from).collect();
        let chars: Vec<char> = lines
            .get(pos.line)
            .map_or(Vec::new(), |l| l.chars().collect());
        if pos.column >= chars.len() {
            return;
        }
        tab.push_undo_snapshot(EditKind::Other, true);
        let mut new_chars = chars;
        new_chars[pos.column] = c;
        lines[pos.line] = new_chars.into_iter().collect();
        let new_text = lines.join("\n");
        self.rebuild_content(&new_text, pos.line, pos.column);
    }

    fn view(&self) -> Element<'_, Message> {
        let tab = &self.tabs[self.active];

        let theme = self.theme();
        let p = theme.extended_palette();
        let bg_base = p.background.base.color;
        let bg_weak = p.background.weak.color;
        let bg_strong = p.background.strong.color;
        let text_main = p.background.base.text;
        let text_muted = p.background.strong.text;
        let primary = p.primary.base.color;
        let editor_font = *EDITOR_FONT;
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
            let match_label = if self.find.matches.is_empty() {
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

            let line_num_text: String = (1..=n_lines)
                .map(|i| format!("{i:>4} "))
                .collect::<Vec<_>>()
                .join("\n");

            let line_numbers = mouse_area(
                container(
                    text(line_num_text)
                        .size(FONT_SIZE)
                        .font(editor_font)
                        .color(text_muted)
                        .line_height(Pixels(LINE_HEIGHT_PX)),
                )
                .padding(Padding {
                    top: 8.0,
                    bottom: 8.0 + overscroll,
                    left: 4.0,
                    right: 0.0,
                }),
            )
            .on_move(Message::GutterMove)
            .on_press(Message::GutterClick);

            let gutter_line = container(Space::new())
                .width(1)
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
                    top: 8.0,
                    bottom: 8.0 + overscroll,
                    left: 8.0,
                    right: 16.0,
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
                    if let keyboard::Key::Named(named) = key {
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
                            key.clone(),
                            mods,
                        )));
                    }

                    // Phase 3: Insert mode — existing iced behavior
                    if let keyboard::Key::Named(named) = key {
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
                    selection: Color { a: 0.3, ..primary },
                });

            scrollable(row![line_numbers, gutter_line, editor].width(Length::Fill))
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

        let mode_label = match self.vim.mode {
            vim::Mode::Normal => "NORMAL",
            vim::Mode::Insert => "INSERT",
            vim::Mode::Visual => "VISUAL",
            vim::Mode::VisualLine => "V-LINE",
        };
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

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            event::listen_with(handle_key),
            iced::time::every(Duration::from_millis(500)).map(|_| Message::AutosaveTick),
        ])
    }
}

// ── Vim text‑range helpers ──────────────────────────────────────────────────

fn extract_text_range(
    lines: &[String],
    from: &text_editor::Position,
    to: &text_editor::Position,
) -> String {
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

fn remove_text_range(
    lines: &mut Vec<String>,
    from: &text_editor::Position,
    to: &text_editor::Position,
) {
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

    if let iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Middle)) = &event {
        if let Some(text) = read_primary_selection() {
            if !text.is_empty() {
                return Some(Message::Edit(text_editor::Action::Edit(
                    text_editor::Edit::Paste(Arc::new(text)),
                )));
            }
        }
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

// ── Clipboard ───────────────────────────────────────────────────────────────

fn is_wayland() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
}

fn read_primary_selection() -> Option<String> {
    let output = if is_wayland() {
        Command::new("wl-paste")
            .arg("--primary")
            .arg("--no-newline")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .ok()?
    } else {
        Command::new("xclip")
            .args(["-selection", "primary", "-o"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .ok()?
    };
    if output.status.success() {
        String::from_utf8(output.stdout).ok()
    } else {
        None
    }
}

fn copy_to_clipboard(text: &str) {
    if is_wayland() {
        pipe_to_command("wl-copy", &[], text);
        pipe_to_command("wl-copy", &["--primary"], text);
    } else {
        pipe_to_command("xclip", &["-selection", "clipboard"], text);
        pipe_to_command("xclip", &["-selection", "primary"], text);
    }
}

fn pipe_to_command(program: &str, args: &[&str], text: &str) {
    match Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(mut child) => {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
            let _ = child.wait();
        }
        Err(e) => eprintln!("lst: clipboard: failed to run {program}: {e}"),
    }
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

// ── Entry point ──────────────────────────────────────────────────────────────

fn main() -> iced::Result {
    iced::application(App::boot, App::update, App::view)
        .title(App::title)
        .theme(App::theme)
        .subscription(App::subscription)
        .default_font(Font::MONOSPACE)
        .window_size(iced::Size::new(980.0, 680.0))
        .exit_on_close_request(false)
        .run()
}
