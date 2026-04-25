use std::{
    io::{self, Write},
    process::{Command, Stdio},
};

/// Persist the active tab's text into the system clipboard at app shutdown.
/// The production impl forks a clipboard owner process so contents survive `lst` exiting;
/// live copy/paste during a session goes through GPUI's own clipboard, not this trait.
pub(crate) trait ExitClipboard: Send + Sync + 'static {
    fn persist(&self, text: &str);
}

pub(crate) struct SubprocessExitClipboard;

impl ExitClipboard for SubprocessExitClipboard {
    fn persist(&self, text: &str) {
        persist_selection_after_exit(SystemSelection::Clipboard, text);
        persist_selection_after_exit(SystemSelection::Primary, text);
    }
}

#[cfg(test)]
#[derive(Default, Clone)]
pub(crate) struct CapturingExitClipboard {
    pub(crate) persisted: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

#[cfg(test)]
impl ExitClipboard for CapturingExitClipboard {
    fn persist(&self, text: &str) {
        self.persisted.lock().unwrap().push(text.to_string());
    }
}

#[derive(Clone, Copy)]
enum SystemSelection {
    Clipboard,
    Primary,
}

fn persist_selection_after_exit(selection: SystemSelection, text: &str) {
    if std::env::var_os("WAYLAND_DISPLAY").is_some()
        && spawn_clipboard_owner("wl-copy", wl_copy_args(selection), text).is_ok()
    {
        return;
    }

    if std::env::var_os("DISPLAY").is_some()
        && spawn_clipboard_owner("xclip", xclip_args(selection), text).is_ok()
    {
        return;
    }

    if std::env::var_os("DISPLAY").is_some() {
        let _ = spawn_clipboard_owner("xsel", xsel_args(selection), text);
    }
}

fn wl_copy_args(selection: SystemSelection) -> &'static [&'static str] {
    match selection {
        SystemSelection::Clipboard => &[],
        SystemSelection::Primary => &["--primary"],
    }
}

fn xclip_args(selection: SystemSelection) -> &'static [&'static str] {
    match selection {
        SystemSelection::Clipboard => &["-selection", "clipboard", "-in"],
        SystemSelection::Primary => &["-selection", "primary", "-in"],
    }
}

fn xsel_args(selection: SystemSelection) -> &'static [&'static str] {
    match selection {
        SystemSelection::Clipboard => &["--clipboard", "--input"],
        SystemSelection::Primary => &["--primary", "--input"],
    }
}

fn spawn_clipboard_owner(program: &str, args: &[&str], text: &str) -> io::Result<()> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(text.as_bytes())?;
    }
    Ok(())
}
