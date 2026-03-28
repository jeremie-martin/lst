mod find;
mod highlight;
mod style;
mod tab;

use find::FindState;
use style::{flat_btn, solid_bg, EDITOR_FONT, FONT_SIZE, LINE_HEIGHT_PX};
use tab::{EditKind, Tab};

use iced::event;
use iced::keyboard;
use iced::widget::{
    button, column, container, mouse_area, responsive, row, scrollable, text, text_editor,
    text_input, Space,
};
use iced::{
    Background, Border, Color, Element, Font, Length, Padding, Pixels, Point, Subscription, Task,
    Theme,
};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

fn editor_id() -> iced::widget::Id {
    iced::widget::Id::new("lst-editor")
}

static EDITOR_ID: LazyLock<iced::widget::Id> = LazyLock::new(editor_id);
static FIND_INPUT_ID: LazyLock<iced::widget::Id> =
    LazyLock::new(|| iced::widget::Id::new("lst-find"));

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
        eprintln!("lst: failed to create scratchpad directory {}: {e}", dir.display());
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
    std::fs::write(&path, "").expect("failed to create scratchpad file");
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
                word_wrap: false,
                scratchpad_dir,
                needs_autosave: false,
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

    fn close_tab(&mut self, i: usize) {
        if i < self.tabs.len() && self.tabs.len() > 1 {
            self.tabs.remove(i);
            if self.active >= self.tabs.len() {
                self.active = self.tabs.len() - 1;
            } else if self.active > i {
                self.active -= 1;
            }
        }
    }

    fn refresh_find_matches(&mut self) {
        if self.find.visible {
            self.find
                .compute_matches(&self.tabs[self.active].content.text());
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

            Message::TabClose(i) => {
                self.close_tab(i);
                Task::none()
            }

            Message::CloseActiveTab => {
                self.close_tab(self.active);
                Task::none()
            }

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

            Message::Quit => iced::exit(),

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
            Message::FindOpen => self.open_find(false),
            Message::FindOpenReplace => self.open_find(true),

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
                tab.content = text_editor::Content::with_text(&new_text);
                // Restore cursor (clamped to new content bounds)
                let max_line = tab.content.line_count().saturating_sub(1);
                let line = cursor_pos.line.min(max_line);
                tab.content.move_to(text_editor::Cursor {
                    position: text_editor::Position {
                        line,
                        column: cursor_pos.column,
                    },
                    selection: None,
                });
                tab.modified = true;
                self.needs_autosave = true;
                self.find.compute_matches(&new_text);
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
        }
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

        // ── Tab bar ──────────────────────────────────────────────────────
        let tab_buttons: Vec<Element<Message>> = self
            .tabs
            .iter()
            .enumerate()
            .flat_map(|(i, t)| {
                let is_active = i == self.active;
                let name = t.display_name();
                let label = if t.modified {
                    format!(" {name} \u{25cf} ")
                } else {
                    format!(" {name} ")
                };

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
                    .width(Length::Fill)
                    .style(solid_bg(bg_strong)),
            )
        } else {
            None
        };

        // ── Editor area ──────────────────────────────────────────────────
        let n_lines = tab.content.line_count().max(1);
        let content_ref = &tab.content;
        let word_wrap = self.word_wrap;
        let find_visible = self.find.visible;
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
                    highlight::Settings { extension: highlight_ext.clone() },
                    highlight::format,
                )
                .key_binding(move |key_press| {
                    let key = &key_press.key;
                    let mods = key_press.modifiers;

                    match key {
                        keyboard::Key::Named(named) => match named {
                            keyboard::key::Named::Escape => {
                                if find_visible {
                                    return Some(text_editor::Binding::Custom(Message::FindClose));
                                }
                            }
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
                            _ => {}
                        },
                        keyboard::Key::Character(c) if mods.command() => match c.as_str() {
                            "z" if mods.shift() => {
                                return Some(text_editor::Binding::Custom(Message::Redo));
                            }
                            "z" => {
                                return Some(text_editor::Binding::Custom(Message::Undo));
                            }
                            _ => {}
                        },
                        _ => {}
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

        let file_label = if tab.modified {
            format!("{name} [modified]")
        } else {
            name.into_owned()
        };

        let wrap_label = if self.word_wrap { "Wrap" } else { "NoWrap" };

        let status_bar = container(
            row![
                text(file_label).size(12).color(text_muted),
                iced::widget::space::horizontal(),
                text(format!("Ln {ln}, Col {col}"))
                    .size(12)
                    .color(text_muted),
                text("  \u{00b7}  ").size(12).color(text_muted),
                button(text(wrap_label).size(12).color(text_muted))
                    .style(flat_btn(bg_weak))
                    .on_press(Message::ToggleWordWrap)
                    .padding(0),
                text("  \u{00b7}  ").size(12).color(text_muted),
                text("UTF-8").size(12).color(text_muted),
            ]
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding {
            top: 4.0,
            bottom: 4.0,
            left: 14.0,
            right: 14.0,
        })
        .width(Length::Fill)
        .style(solid_bg(bg_weak));

        // ── Root ─────────────────────────────────────────────────────────
        let mut layout = column![tab_bar];
        if let Some(bar) = find_bar {
            layout = layout.push(bar);
        }
        layout = layout.push(editor_area).push(status_bar);
        layout.into()
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            event::listen_with(handle_key),
            iced::time::every(Duration::from_millis(500)).map(|_| Message::AutosaveTick),
        ])
    }
}

// ── Keyboard shortcuts ───────────────────────────────────────────────────────

fn handle_key(event: iced::Event, status: event::Status, _id: iced::window::Id) -> Option<Message> {
    if status != event::Status::Ignored {
        return None;
    }

    let iced::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) = event else {
        return None;
    };

    // Non-modifier shortcuts
    if matches!(key, keyboard::Key::Named(keyboard::key::Named::Escape)) {
        return Some(Message::FindClose);
    }

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
            _ => None,
        },
        keyboard::Key::Named(named) => match named {
            keyboard::key::Named::PageUp if modifiers.shift() => Some(Message::MoveTabLeft),
            keyboard::key::Named::PageDown if modifiers.shift() => Some(Message::MoveTabRight),
            _ => None,
        },
        _ => None,
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
        .run()
}
