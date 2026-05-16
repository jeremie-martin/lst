//! Selection helpers backed by `xclip`. The harness shells out instead of
//! implementing X11 selection requests in-process: getting the clipboard
//! correct (INCR for large pastes, owner negotiation, target conversion) is
//! a meaningful body of code, and `xclip` is already a hard dep of the test
//! infrastructure.

use std::io::Write;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::Result;

#[derive(Clone, Copy, Debug)]
pub enum Selection {
    Clipboard,
    Primary,
}

impl Selection {
    fn xclip_arg(self) -> &'static str {
        match self {
            Selection::Clipboard => "clipboard",
            Selection::Primary => "primary",
        }
    }
}

/// Read selection text via `xclip`. Returns `None` if `xclip` exits non-zero
/// (typically: empty selection, or X server unreachable).
pub fn read_clipboard_text(sel: Selection) -> Option<String> {
    let output = Command::new("xclip")
        .args(["-selection", sel.xclip_arg(), "-o"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

pub fn write_clipboard_text(sel: Selection, text: &str) -> Result<()> {
    let mut child = Command::new("xclip")
        .args(["-selection", sel.xclip_arg(), "-in"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(text.as_bytes())?;
    }
    let status = child.wait()?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("xclip exited with {status}").into())
    }
}

pub fn wait_clipboard_text(sel: Selection, expected: &str, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        if read_clipboard_text(sel).as_deref() == Some(expected) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "timed out waiting for X11 {} selection to match expected text",
                sel.xclip_arg()
            )
            .into());
        }
        thread::sleep(Duration::from_millis(25));
    }
}

/// Wait until the selection's raw byte count from `xclip -o` equals
/// `expected_bytes`. ASCII-only equivalence: byte count == char count holds
/// only for ASCII corpora.
pub fn wait_clipboard_bytes(sel: Selection, expected_bytes: u64, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(actual) = read_clipboard_bytes(sel) {
            if actual == expected_bytes {
                return Ok(());
            }
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "timed out waiting for {} selection to reach {expected_bytes} bytes",
                sel.xclip_arg()
            )
            .into());
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn read_clipboard_bytes(sel: Selection) -> Option<u64> {
    let output = Command::new("xclip")
        .args(["-selection", sel.xclip_arg(), "-o"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output.status.success().then_some(output.stdout.len() as u64)
}

pub(crate) fn require_xclip() -> Result<()> {
    let status = Command::new("sh")
        .arg("-c")
        .arg("command -v xclip >/dev/null 2>&1")
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err("xclip is required on PATH for the X11 harness".into())
    }
}
