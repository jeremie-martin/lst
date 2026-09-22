//! The prompt-add filter owns editorial behavior, provider access, and persistent history.
use std::io::Write;
use std::process::{Command, Stdio};

pub(crate) struct Rewrite {
    pub text: String,
    pub warning: String,
}

pub(crate) fn rewrite(source: &str) -> Result<Rewrite, String> {
    let mut child = Command::new("prompt-add")
        .args(["-", "--label", "lst", "--no-clipboard"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Could not start prompt-add: {error}. Install it and make it available on PATH."))?;
    let mut stdin = child.stdin.take().expect("piped child stdin");
    // Drain output while writing input: a filter may report an error before reading stdin.
    let (output, written) = std::thread::scope(|scope| {
        let writer = scope.spawn(move || stdin.write_all(source.as_bytes()));
        (child.wait_with_output(), writer.join())
    });
    let output = output.map_err(|error| format!("Could not wait for prompt-add: {error}"))?;
    let diagnostic = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !output.status.success() {
        return Err(if diagnostic.is_empty() {
            format!("prompt-add exited with {}", output.status)
        } else {
            diagnostic
        });
    }
    written
        .map_err(|_| "Could not send text to prompt-add: writer panicked".to_string())?
        .map_err(|error| format!("Could not send text to prompt-add: {error}"))?;
    let text = String::from_utf8(output.stdout).map_err(|_| "prompt-add returned invalid UTF-8".to_string())?;
    if text.trim().is_empty() {
        return Err("prompt-add returned an empty message".to_string());
    }
    let warning = diagnostic
        .lines()
        .filter(|line| !matches!(*line, "Rewritten" | "Unchanged"))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(Rewrite { text, warning })
}
