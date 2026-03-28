mod highlight;

use iced::event;
use iced::keyboard;
use iced::widget::{
    button, column, container, mouse_area, responsive, row, scrollable, text, text_editor, Space,
};
use iced::{
    Background, Border, Color, Element, Font, Length, Padding, Point, Subscription, Task, Theme,
};
use std::borrow::Cow;
use std::path::PathBuf;

const JB_MONO: Font = Font::with_name("JetBrains Mono");

fn editor_id() -> iced::widget::Id {
    iced::widget::Id::new("lst-editor")
}

// Lazy static-like pattern: call once, clone as needed
use std::sync::LazyLock;
static EDITOR_ID: LazyLock<iced::widget::Id> = LazyLock::new(editor_id);

// ── Tab ──────────────────────────────────────────────────────────────────────

struct Tab {
    path: Option<PathBuf>,
    content: text_editor::Content,
    modified: bool,
}

impl Tab {
    fn new() -> Self {
        Self {
            path: None,
            content: text_editor::Content::new(),
            modified: false,
        }
    }

    fn from_path(path: PathBuf, body: &str) -> Self {
        Self {
            path: Some(path),
            content: text_editor::Content::with_text(body),
            modified: false,
        }
    }

    fn display_name(&self) -> Cow<'_, str> {
        match &self.path {
            Some(p) => p.file_name().unwrap_or_default().to_string_lossy(),
            None => Cow::Borrowed("untitled"),
        }
    }
}

// ── App ──────────────────────────────────────────────────────────────────────

struct App {
    tabs: Vec<Tab>,
    active: usize,
    window_title: Option<String>,
    gutter_mouse_y: f32,
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
    Saved(Result<PathBuf, Error>),
    GutterMove(Point),
    GutterClick,
    Quit,
}

#[derive(Debug, Clone)]
enum Error {
    DialogClosed,
    Io,
}

struct CliArgs {
    window_title: Option<String>,
    files: Vec<PathBuf>,
}

fn parse_args() -> CliArgs {
    let mut args = std::env::args().skip(1);
    let mut window_title = None;
    let mut files = Vec::new();

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

        files.push(PathBuf::from(arg));
    }

    CliArgs {
        window_title,
        files,
    }
}

impl App {
    fn boot() -> (Self, Task<Message>) {
        let args = parse_args();

        let mut tabs: Vec<Tab> = args
            .files
            .into_iter()
            .filter_map(|path| {
                let body = std::fs::read_to_string(&path).ok()?;
                Some(Tab::from_path(path.canonicalize().unwrap_or(path), &body))
            })
            .collect();

        if tabs.is_empty() {
            tabs.push(Tab::new());
        }

        (
            Self {
                tabs,
                active: 0,
                window_title: args.window_title,
                gutter_mouse_y: 0.0,
            },
            Task::none(),
        )
    }

    fn title(&self) -> String {
        if let Some(title) = &self.window_title {
            return title.clone();
        }

        let tab = &self.tabs[self.active];
        match &tab.path {
            Some(p) => format!("{} — lst", p.display()),
            None => format!("{} — lst", tab.display_name()),
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

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Edit(action) => {
                let is_edit = matches!(action, text_editor::Action::Edit(_));
                self.tabs[self.active].content.perform(action);
                if is_edit {
                    self.tabs[self.active].modified = true;
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
                self.tabs.push(Tab::new());
                self.active = self.tabs.len() - 1;
                Task::none()
            }

            Message::Open => Task::perform(open_file(), Message::Opened),

            Message::Opened(Ok((path, body))) => {
                // If there's only one empty untitled tab, replace it
                if self.tabs.len() == 1
                    && self.tabs[0].path.is_none()
                    && self.tabs[0].content.text().trim().is_empty()
                {
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

            Message::Saved(Ok(path)) => {
                let tab = &mut self.tabs[self.active];
                tab.path = Some(path);
                tab.modified = false;
                Task::none()
            }
            Message::Saved(Err(_)) => Task::none(),

            Message::GutterMove(point) => {
                self.gutter_mouse_y = point.y;
                Task::none()
            }

            Message::GutterClick => {
                // Compute which line was clicked from the Y position.
                // The gutter has top padding of 8.0, and each line is ~20px tall
                // (14px font size + cosmic-text line spacing).
                const TOP_PAD: f32 = 8.0;
                const LINE_HEIGHT: f32 = 20.0;
                let line = ((self.gutter_mouse_y - TOP_PAD) / LINE_HEIGHT).max(0.0) as usize;

                let tab = &mut self.tabs[self.active];
                let line = line.min(tab.content.line_count().saturating_sub(1));
                let y = line as f32 * LINE_HEIGHT;
                tab.content
                    .perform(text_editor::Action::Click(Point::new(0.0, y)));
                tab.content.perform(text_editor::Action::SelectLine);

                // Focus the text editor so the selection is visible
                iced::widget::operation::focus(EDITOR_ID.clone())
            }

            Message::Quit => iced::exit(),
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let tab = &self.tabs[self.active];

        // Extract palette colors upfront (Color is Copy, avoids lifetime issues)
        let theme = self.theme();
        let p = theme.extended_palette();
        let bg_base = p.background.base.color;
        let bg_weak = p.background.weak.color;
        let bg_strong = p.background.strong.color;
        let text_main = p.background.base.text;
        let text_muted = p.background.strong.text;
        let primary = p.primary.base.color;

        // ── Tab bar ──────────────────────────────────────────────────────────
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

        // ── Editor area ──────────────────────────────────────────────────────
        let n_lines = tab.content.line_count().max(1);
        let content_ref = &tab.content;

        let editor_area = responsive(move |size| {
            let vh = size.height;
            let overscroll = vh * 0.4;

            let line_num_text: String = (1..=n_lines)
                .map(|i| format!("{i:>4} "))
                .collect::<Vec<_>>()
                .join("\n");

            let line_numbers = mouse_area(
                container(text(line_num_text).size(14).font(JB_MONO).color(text_muted)).padding(
                    Padding {
                        top: 8.0,
                        bottom: 8.0 + overscroll,
                        left: 4.0,
                        right: 0.0,
                    },
                ),
            )
            .on_move(Message::GutterMove)
            .on_press(Message::GutterClick);

            let gutter_line = container(Space::new())
                .width(1)
                .height(Length::Fill)
                .style(solid_bg(bg_strong));

            let editor = text_editor(content_ref)
                .id(EDITOR_ID.clone())
                .on_action(Message::Edit)
                .font(JB_MONO)
                .size(14)
                .padding(Padding {
                    top: 8.0,
                    bottom: 8.0 + overscroll,
                    left: 8.0,
                    right: 16.0,
                })
                .height(Length::Shrink)
                .min_height(vh)
                .highlight_with::<highlight::MdHighlighter>(highlight::Settings, highlight::format)
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

        // ── Status bar ───────────────────────────────────────────────────────
        let cursor = tab.content.cursor();
        let ln = cursor.position.line + 1;
        let col = cursor.position.column + 1;
        let name = tab.display_name();

        let file_label = if tab.modified {
            format!("{name} [modified]")
        } else {
            name.into_owned()
        };

        let status_bar = container(
            row![
                text(file_label).size(12).color(text_muted),
                iced::widget::space::horizontal(),
                text(format!("Ln {ln}, Col {col}"))
                    .size(12)
                    .color(text_muted),
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

        // ── Root ─────────────────────────────────────────────────────────────
        column![tab_bar, editor_area, status_bar].into()
    }

    fn subscription(&self) -> Subscription<Message> {
        event::listen_with(handle_key)
    }
}

// ── Style helpers ────────────────────────────────────────────────────────────

fn flat_btn(bg: Color) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_theme, _status| button::Style {
        background: Some(Background::Color(bg)),
        border: Border::default().rounded(0),
        ..button::Style::default()
    }
}

fn solid_bg(color: Color) -> impl Fn(&Theme) -> container::Style {
    move |_theme| container::Style {
        background: Some(Background::Color(color)),
        ..container::Style::default()
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

    if !modifiers.command() {
        return None;
    }

    let keyboard::Key::Character(ref c) = key else {
        return None;
    };

    match c.as_str() {
        "n" => Some(Message::New),
        "o" => Some(Message::Open),
        "s" => Some(Message::Save),
        "w" => Some(Message::CloseActiveTab),
        "q" => Some(Message::Quit),
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
        .font(include_bytes!(
            "/usr/share/fonts/jetbrains-mono/JetBrainsMono[wght].ttf"
        ))
        .default_font(Font::DEFAULT)
        .window_size(iced::Size::new(980.0, 680.0))
        .run()
}
