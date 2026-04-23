use std::{
    io::{self, Write},
    process::{Command, Stdio},
};

#[derive(Clone, Copy)]
enum SystemSelection {
    Clipboard,
    Primary,
}

pub(super) fn persist_clipboards_after_exit(text: &str) {
    persist_selection_after_exit(SystemSelection::Clipboard, text);
    persist_selection_after_exit(SystemSelection::Primary, text);
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
