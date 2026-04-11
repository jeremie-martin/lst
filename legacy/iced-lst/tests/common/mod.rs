#![allow(dead_code)]

use iced::event;
use iced::keyboard;
use iced::widget::text_editor;
use iced::{window, Point};
use lst::app::{
    route_event, App, AppArgs, AppServices, Error, Message, RuntimeEffect, RuntimeMode,
};
use lst::clipboard::Clipboard;
use lst::clock::FixedClock;
use lst::dialogs::{DialogFuture, Dialogs};
use lst::fs::Filesystem;
use lst::style::{EDITOR_PAD, LINE_HEIGHT_PX};
use std::collections::{HashMap, HashSet, VecDeque};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

// ── Action helpers (drive the app through its message interface) ─────────────

pub fn type_text(app: &mut App, text: &str) {
    for ch in text.chars() {
        app.update_inner(Message::Edit(text_editor::Action::Edit(
            text_editor::Edit::Insert(ch),
        )));
    }
}

pub fn backspace(app: &mut App) {
    app.update_inner(Message::Edit(text_editor::Action::Edit(
        text_editor::Edit::Backspace,
    )));
}

pub fn move_to_end(app: &mut App) {
    app.update_inner(Message::Edit(text_editor::Action::Move(
        text_editor::Motion::End,
    )));
}

pub fn move_down(app: &mut App) {
    app.update_inner(Message::Edit(text_editor::Action::Move(
        text_editor::Motion::Down,
    )));
}

pub fn move_right(app: &mut App) {
    app.update_inner(Message::Edit(text_editor::Action::Move(
        text_editor::Motion::Right,
    )));
}

pub fn goto_line(app: &mut App, line: usize) {
    app.update_inner(Message::GotoLineOpen);
    app.update_inner(Message::GotoLineChanged(line.to_string()));
    app.update_inner(Message::GotoLineSubmit);
}

pub fn make_multiline_doc(lines: usize) -> String {
    (1..=lines)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n")
}

// ── Vim helpers ─────────────────────────────────────────────────────────────

/// Send Escape — cascades through: close goto-line → close find → vim mode transition.
/// In a clean test state (no overlays), this enters Normal mode from Insert.
pub fn escape(app: &mut App) {
    app.update_inner(Message::GotoLineClose);
}

/// Alias for `escape` — enters Normal mode from Insert (when no overlays are open).
pub fn enter_normal(app: &mut App) {
    escape(app);
}

/// Send a single character as a VimKey message (no modifiers).
pub fn vim_key(app: &mut App, c: char) {
    app.update_inner(Message::VimKey(
        keyboard::Key::Character(c.to_string().into()),
        keyboard::Modifiers::default(),
    ));
}

/// Send a sequence of characters as VimKey messages.
pub fn vim_keys(app: &mut App, keys: &str) {
    for c in keys.chars() {
        vim_key(app, c);
    }
}

/// Send a VimKey with Ctrl/Cmd modifier (e.g., Ctrl+R for redo).
pub fn vim_ctrl(app: &mut App, c: char) {
    app.update_inner(Message::VimKey(
        keyboard::Key::Character(c.to_string().into()),
        keyboard::Modifiers::COMMAND,
    ));
}

// ── MemoryClipboard ─────────────────────────────────────────────────────────

/// A test clipboard that stores text in memory for inspection.
/// Clone-able — the test keeps a clone sharing the same Arc buffers.
#[derive(Clone)]
pub struct MemoryClipboard {
    clipboard: Arc<Mutex<String>>,
    primary: Arc<Mutex<String>>,
}

impl MemoryClipboard {
    pub fn new() -> Self {
        Self {
            clipboard: Arc::new(Mutex::new(String::new())),
            primary: Arc::new(Mutex::new(String::new())),
        }
    }

    pub fn with_primary(text: &str) -> Self {
        Self {
            clipboard: Arc::new(Mutex::new(String::new())),
            primary: Arc::new(Mutex::new(text.to_string())),
        }
    }

    pub fn get_clipboard(&self) -> String {
        self.clipboard.lock().unwrap().clone()
    }

    pub fn get_primary(&self) -> String {
        self.primary.lock().unwrap().clone()
    }
}

impl Clipboard for MemoryClipboard {
    fn copy(&self, text: &str) {
        *self.clipboard.lock().unwrap() = text.to_string();
        self.copy_primary(text);
    }

    fn copy_primary(&self, text: &str) {
        *self.primary.lock().unwrap() = text.to_string();
    }

    fn read_primary(&self) -> Option<String> {
        let s = self.primary.lock().unwrap();
        if s.is_empty() {
            None
        } else {
            Some(s.clone())
        }
    }
}

// ── MemoryFilesystem ────────────────────────────────────────────────────────

#[derive(Default)]
struct FsState {
    files: HashMap<PathBuf, String>,
    removed_files: Vec<PathBuf>,
    created_dirs: Vec<PathBuf>,
    directories: HashSet<PathBuf>,
    read_failures: HashSet<PathBuf>,
    write_failures: HashSet<PathBuf>,
    create_dir_failures: HashSet<PathBuf>,
    canonical_paths: HashMap<PathBuf, PathBuf>,
}

#[derive(Clone, Default)]
pub struct MemoryFilesystem {
    state: Arc<Mutex<FsState>>,
}

impl MemoryFilesystem {
    pub fn seed_file(&self, path: impl AsRef<Path>, body: &str) {
        self.state
            .lock()
            .unwrap()
            .files
            .insert(path.as_ref().to_path_buf(), body.to_string());
    }

    pub fn set_read_failure(&self, path: impl AsRef<Path>) {
        self.state
            .lock()
            .unwrap()
            .read_failures
            .insert(path.as_ref().to_path_buf());
    }

    pub fn set_write_failure(&self, path: impl AsRef<Path>) {
        self.state
            .lock()
            .unwrap()
            .write_failures
            .insert(path.as_ref().to_path_buf());
    }

    pub fn set_create_dir_failure(&self, path: impl AsRef<Path>) {
        self.state
            .lock()
            .unwrap()
            .create_dir_failures
            .insert(path.as_ref().to_path_buf());
    }

    pub fn set_canonical_path(&self, from: impl AsRef<Path>, to: impl AsRef<Path>) {
        self.state
            .lock()
            .unwrap()
            .canonical_paths
            .insert(from.as_ref().to_path_buf(), to.as_ref().to_path_buf());
    }

    pub fn file_text(&self, path: impl AsRef<Path>) -> Option<String> {
        self.state.lock().unwrap().files.get(path.as_ref()).cloned()
    }

    pub fn removed_files(&self) -> Vec<PathBuf> {
        self.state.lock().unwrap().removed_files.clone()
    }

    pub fn created_dirs(&self) -> Vec<PathBuf> {
        self.state.lock().unwrap().created_dirs.clone()
    }
}

impl Filesystem for MemoryFilesystem {
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        let state = self.state.lock().unwrap();
        if state.read_failures.contains(path) {
            return Err(io::Error::other("read failure"));
        }

        state
            .files
            .get(path)
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "missing file"))
    }

    fn write(&self, path: &Path, contents: &str) -> io::Result<()> {
        let mut state = self.state.lock().unwrap();
        if state.write_failures.contains(path) {
            return Err(io::Error::other("write failure"));
        }

        state.files.insert(path.to_path_buf(), contents.to_string());
        Ok(())
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        let mut state = self.state.lock().unwrap();
        state.files.remove(path);
        state.removed_files.push(path.to_path_buf());
        Ok(())
    }

    fn exists(&self, path: &Path) -> bool {
        let state = self.state.lock().unwrap();
        state.files.contains_key(path) || state.directories.contains(path)
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        let mut state = self.state.lock().unwrap();
        if state.create_dir_failures.contains(path) {
            return Err(io::Error::other("create_dir failure"));
        }

        state.directories.insert(path.to_path_buf());
        state.created_dirs.push(path.to_path_buf());
        Ok(())
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        let state = self.state.lock().unwrap();
        if let Some(canonical) = state.canonical_paths.get(path) {
            return Ok(canonical.clone());
        }
        if state.files.contains_key(path) || state.directories.contains(path) {
            Ok(path.to_path_buf())
        } else {
            Err(io::Error::new(io::ErrorKind::NotFound, "missing path"))
        }
    }
}

// ── ScriptedDialogs ────────────────────────────────────────────────────────

#[derive(Default)]
struct DialogState {
    open_responses: VecDeque<Option<PathBuf>>,
    save_responses: VecDeque<Option<PathBuf>>,
    open_requests: usize,
    save_requests: usize,
    save_suggestions: Vec<String>,
}

#[derive(Clone, Default)]
pub struct ScriptedDialogs {
    state: Arc<Mutex<DialogState>>,
}

impl ScriptedDialogs {
    pub fn push_open(&self, path: impl AsRef<Path>) {
        self.state
            .lock()
            .unwrap()
            .open_responses
            .push_back(Some(path.as_ref().to_path_buf()));
    }

    pub fn cancel_open(&self) {
        self.state.lock().unwrap().open_responses.push_back(None);
    }

    pub fn push_save(&self, path: impl AsRef<Path>) {
        self.state
            .lock()
            .unwrap()
            .save_responses
            .push_back(Some(path.as_ref().to_path_buf()));
    }

    pub fn cancel_save(&self) {
        self.state.lock().unwrap().save_responses.push_back(None);
    }

    pub fn open_requests(&self) -> usize {
        self.state.lock().unwrap().open_requests
    }

    pub fn save_requests(&self) -> usize {
        self.state.lock().unwrap().save_requests
    }

    pub fn save_suggestions(&self) -> Vec<String> {
        self.state.lock().unwrap().save_suggestions.clone()
    }

    fn next_open_response(&self) -> Option<PathBuf> {
        let mut state = self.state.lock().unwrap();
        state.open_requests += 1;
        state.open_responses.pop_front().flatten()
    }

    fn next_save_response(&self, suggested_name: &str) -> Option<PathBuf> {
        let mut state = self.state.lock().unwrap();
        state.save_requests += 1;
        state.save_suggestions.push(suggested_name.to_string());
        state.save_responses.pop_front().flatten()
    }
}

impl Dialogs for ScriptedDialogs {
    fn pick_open_file(&self) -> DialogFuture {
        let response = self.next_open_response();
        Box::pin(async move { response })
    }

    fn pick_save_file(&self, suggested_name: &str) -> DialogFuture {
        let response = self.next_save_response(suggested_name);
        Box::pin(async move { response })
    }

    fn pick_open_file_blocking(&self) -> Option<PathBuf> {
        self.next_open_response()
    }

    fn pick_save_file_blocking(&self, suggested_name: &str) -> Option<PathBuf> {
        self.next_save_response(suggested_name)
    }
}

// ── AppHarness ─────────────────────────────────────────────────────────────

pub struct AppHarness {
    pub app: App,
    pub clipboard: MemoryClipboard,
    pub fs: MemoryFilesystem,
    pub dialogs: ScriptedDialogs,
    window_id: window::Id,
}

impl AppHarness {
    pub fn new(text: &str) -> Self {
        Self::new_with_timestamp(text, "1970-01-01_00-00-00")
    }

    pub fn new_with_timestamp(text: &str, timestamp: &str) -> Self {
        let (clipboard, fs, dialogs, services) = runtime_services(timestamp);
        let app = App::test_with_services(text, services);

        Self {
            app,
            clipboard,
            fs,
            dialogs,
            window_id: window::Id::unique(),
        }
    }

    pub fn boot_with<F>(args: &[&str], timestamp: &str, configure: F) -> Self
    where
        F: FnOnce(&MemoryFilesystem, &ScriptedDialogs),
    {
        let (clipboard, fs, dialogs, services) = runtime_services(timestamp);
        configure(&fs, &dialogs);
        let args = AppArgs::parse_from(args.iter().copied()).unwrap();
        let (app, _) = App::boot_with(args, services).unwrap();

        Self {
            app,
            clipboard,
            fs,
            dialogs,
            window_id: window::Id::unique(),
        }
    }

    pub fn boot(args: &[&str]) -> Self {
        Self::boot_with(args, "1970-01-01_00-00-00", |_, _| {})
    }

    pub fn snapshot(&self) -> lst::app::ViewSnapshot {
        self.app.snapshot()
    }

    pub fn send(&mut self, message: Message) {
        let mut pending = vec![message];

        while let Some(message) = pending.pop() {
            let result = self.app.update_inner(message);
            let mut followups = self.resolve_effects(result.effects);
            followups.reverse();
            pending.extend(followups);
        }
    }

    pub fn dispatch_event(&mut self, event: iced::Event, status: event::Status) {
        if let Some(message) = route_event(event, status, self.window_id) {
            self.send(message);
        }
    }

    pub fn shortcut_char(&mut self, c: char, modifiers: keyboard::Modifiers) {
        self.dispatch_event(
            character_key_event(c.to_string(), modifiers),
            event::Status::Ignored,
        );
    }

    pub fn shortcut_named(&mut self, named: keyboard::key::Named, modifiers: keyboard::Modifiers) {
        self.dispatch_event(named_key_event(named, modifiers), event::Status::Ignored);
    }

    pub fn set_modifiers(&mut self, modifiers: keyboard::Modifiers) {
        self.dispatch_event(
            iced::Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)),
            event::Status::Ignored,
        );
    }

    pub fn close_requested(&mut self) {
        self.dispatch_event(
            iced::Event::Window(iced::window::Event::CloseRequested),
            event::Status::Ignored,
        );
    }

    pub fn gutter_click_line(&mut self, line: usize) {
        let point = Point::new(4.0, EDITOR_PAD + line as f32 * LINE_HEIGHT_PX);
        self.send(Message::GutterMove(point));
        self.send(Message::GutterClick);
    }

    fn resolve_effects(&self, effects: Vec<RuntimeEffect>) -> Vec<Message> {
        effects
            .into_iter()
            .map(|effect| self.resolve_effect(effect))
            .collect()
    }

    fn resolve_effect(&self, effect: RuntimeEffect) -> Message {
        match effect {
            RuntimeEffect::OpenFile => {
                let Some(path) = self.dialogs.next_open_response() else {
                    return Message::Opened(Err(Error::DialogClosed));
                };

                match self.fs.read_to_string(&path) {
                    Ok(body) => Message::Opened(Ok((path, body))),
                    Err(_) => Message::Opened(Err(Error::Io)),
                }
            }
            RuntimeEffect::SaveFile { path, body } => match self.fs.write(&path, &body) {
                Ok(()) => Message::Saved(Ok(path)),
                Err(_) => Message::Saved(Err(Error::Io)),
            },
            RuntimeEffect::AutosaveFile { path, body } => match self.fs.write(&path, &body) {
                Ok(()) => Message::AutosaveComplete(Ok(path)),
                Err(_) => Message::AutosaveComplete(Err(Error::Io)),
            },
            RuntimeEffect::SaveFileAs {
                suggested_name,
                body,
            } => {
                let Some(path) = self.dialogs.next_save_response(&suggested_name) else {
                    return Message::Saved(Err(Error::DialogClosed));
                };

                match self.fs.write(&path, &body) {
                    Ok(()) => Message::Saved(Ok(path)),
                    Err(_) => Message::Saved(Err(Error::Io)),
                }
            }
        }
    }
}

pub fn runtime_services_with_options(
    timestamp: &str,
    runtime_mode: RuntimeMode,
    home_dir: Option<impl AsRef<Path>>,
) -> (
    MemoryClipboard,
    MemoryFilesystem,
    ScriptedDialogs,
    AppServices,
) {
    let clipboard = MemoryClipboard::new();
    let fs = MemoryFilesystem::default();
    let dialogs = ScriptedDialogs::default();
    let services = AppServices {
        clipboard: Arc::new(clipboard.clone()),
        fs: Arc::new(fs.clone()),
        dialogs: Arc::new(dialogs.clone()),
        clock: Arc::new(FixedClock::new(timestamp)),
        home_dir: home_dir.map(|path| path.as_ref().to_path_buf()),
        runtime_mode,
    };

    (clipboard, fs, dialogs, services)
}

fn runtime_services(
    timestamp: &str,
) -> (
    MemoryClipboard,
    MemoryFilesystem,
    ScriptedDialogs,
    AppServices,
) {
    runtime_services_with_options(timestamp, RuntimeMode::Async, Some("/tmp/lst-harness-home"))
}

pub fn character_key_event(text: String, modifiers: keyboard::Modifiers) -> iced::Event {
    let key = keyboard::Key::Character(text.clone().into());

    iced::Event::Keyboard(keyboard::Event::KeyPressed {
        key: key.clone(),
        modified_key: key,
        physical_key: keyboard::key::Physical::Code(keyboard::key::Code::KeyA),
        location: keyboard::Location::Standard,
        modifiers,
        text: if modifiers.command() {
            None
        } else {
            Some(text.into())
        },
        repeat: false,
    })
}

pub fn named_key_event(named: keyboard::key::Named, modifiers: keyboard::Modifiers) -> iced::Event {
    iced::Event::Keyboard(keyboard::Event::KeyPressed {
        key: keyboard::Key::Named(named),
        modified_key: keyboard::Key::Named(named),
        physical_key: keyboard::key::Physical::Code(keyboard::key::Code::KeyA),
        location: keyboard::Location::Standard,
        modifiers,
        text: None,
        repeat: false,
    })
}

// ── App builders with MemoryClipboard ───────────────────────────────────────

pub fn app_with_clipboard(text: &str) -> (App, MemoryClipboard) {
    let clip = MemoryClipboard::new();
    let mut app = App::test(text);
    app.clipboard = Arc::new(clip.clone());
    (app, clip)
}

pub fn app_with_primary(text: &str, primary: &str) -> (App, MemoryClipboard) {
    let clip = MemoryClipboard::with_primary(primary);
    let mut app = App::test(text);
    app.clipboard = Arc::new(clip.clone());
    (app, clip)
}
