use std::{
    io::{self, Write},
    process::{Command, Stdio},
};

/// Persist the active tab's text into the system clipboard at app shutdown.
/// Forks a clipboard owner process so contents survive `lst` exiting; live
/// copy/paste during a session goes through GPUI's own clipboard, not this.
pub(crate) fn persist_clipboards_after_exit(text: &str) -> Result<(), String> {
    // Don't take ownership of the clipboard/primary selection when there is
    // nothing to persist — quitting with an empty buffer must not wipe out
    // whatever the user already had on the clipboard.
    if text.is_empty() {
        return Ok(());
    }
    // Integration tests exercise the user-visible retry path through the
    // production quit transaction while faking only this process boundary.
    if std::env::var_os("LST_TEST_CLIPBOARD_OWNER_FAILURE").is_some() {
        return Err("clipboard owner failure requested by test environment".to_string());
    }
    let clipboard = persist_selection_after_exit(SystemSelection::Clipboard, text);
    let primary = persist_selection_after_exit(SystemSelection::Primary, text);
    match (clipboard, primary) {
        (Ok(()), Ok(())) => Ok(()),
        (clipboard, primary) => Err(format!(
            "clipboard owner failed (CLIPBOARD: {}; PRIMARY: {})",
            selection_result_label(clipboard),
            selection_result_label(primary)
        )),
    }
}

#[derive(Clone, Copy)]
enum SystemSelection {
    Clipboard,
    Primary,
}

fn persist_selection_after_exit(selection: SystemSelection, text: &str) -> io::Result<()> {
    let mut last_error = None;
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        match spawn_clipboard_owner("wl-copy", wl_copy_args(selection), text) {
            Ok(()) => return Ok(()),
            Err(error) => last_error = Some(error),
        }
    }

    if std::env::var_os("DISPLAY").is_some() {
        match spawn_clipboard_owner("xclip", xclip_args(selection), text) {
            Ok(()) => return Ok(()),
            Err(error) => last_error = Some(error),
        }
        match spawn_clipboard_owner("xsel", xsel_args(selection), text) {
            Ok(()) => return Ok(()),
            Err(error) => last_error = Some(error),
        }
    }

    Err(last_error.unwrap_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "no Wayland or X11 clipboard owner is available",
        )
    }))
}

fn selection_result_label(result: io::Result<()>) -> String {
    result.map_or_else(|error| error.to_string(), |()| "ok".to_string())
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
