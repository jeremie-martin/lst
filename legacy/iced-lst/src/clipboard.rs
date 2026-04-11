use std::io::Write as _;
use std::process::{Command, Stdio};
use std::sync::Arc;

pub trait Clipboard: Send + Sync {
    fn copy(&self, text: &str);
    fn copy_primary(&self, text: &str);
    fn read_primary(&self) -> Option<String>;
}

pub type SharedClipboard = Arc<dyn Clipboard>;

// ── Real clipboard (production) ──────────────────────────────────────────────

pub struct RealClipboard;

impl Clipboard for RealClipboard {
    fn copy(&self, text: &str) {
        if is_wayland() {
            pipe_to_command("wl-copy", &[], text);
        } else {
            pipe_to_command("xclip", &["-selection", "clipboard"], text);
        }
        self.copy_primary(text);
    }

    fn copy_primary(&self, text: &str) {
        if is_wayland() {
            pipe_to_command("wl-copy", &["--primary"], text);
        } else {
            pipe_to_command("xclip", &["-selection", "primary"], text);
        }
    }

    fn read_primary(&self) -> Option<String> {
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
}

// ── Null clipboard (for tests) ──────────────────────────────────────────────

pub struct NullClipboard;

impl Clipboard for NullClipboard {
    fn copy(&self, _: &str) {}
    fn copy_primary(&self, _: &str) {}
    fn read_primary(&self) -> Option<String> {
        None
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn is_wayland() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
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
