use std::backtrace::Backtrace;
use std::env;
use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::panic;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use time::OffsetDateTime;

pub(crate) fn record_label(label: &str, value: &str) {
    record_line(label, format_args!("{value}"));
}

pub(crate) fn record_ms(label: &str, value: f64) {
    record_line(label, format_args!("{value:.3}"));
}

pub(crate) fn record_usize(label: &str, value: usize) {
    record_line(label, format_args!("{value}"));
}

pub(crate) fn record_operation(label: &str, bytes: usize, lines: usize, clipboard_read_ms: Option<f64>, apply_ms: f64) {
    let Some(path) = trace_path() else {
        return;
    };

    if let Err(err) = append_operation(path, label, bytes, lines, clipboard_read_ms, apply_ms) {
        eprintln!("lst_gpui failed to write benchmark trace: {err}");
    }
}

fn record_line(label: &str, value: std::fmt::Arguments<'_>) {
    let Some(path) = trace_path() else {
        return;
    };

    if let Err(err) = append_line(path, format_args!("{label}={value}\n")) {
        eprintln!("lst_gpui failed to write benchmark trace: {err}");
    }
}

fn trace_path() -> Option<&'static Path> {
    static CACHED: OnceLock<Option<PathBuf>> = OnceLock::new();
    CACHED
        .get_or_init(|| {
            env::var_os("LST_BENCH_TRACE_FILE")
                .filter(|p| !p.is_empty())
                .map(PathBuf::from)
        })
        .as_deref()
}

fn append_line(path: &Path, line: std::fmt::Arguments<'_>) -> io::Result<()> {
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_fmt(line)
}

fn append_operation(
    path: &Path,
    label: &str,
    bytes: usize,
    lines: usize,
    clipboard_read_ms: Option<f64>,
    apply_ms: f64,
) -> io::Result<()> {
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{label}_apply_ms={apply_ms:.3}")?;
    if let Some(read_ms) = clipboard_read_ms {
        writeln!(file, "{label}_clipboard_read_ms={read_ms:.3}")?;
    }
    writeln!(file, "{label}_bytes={bytes}")?;
    writeln!(file, "{label}_lines={lines}")?;
    Ok(())
}

const CRASH_LOG_RELATIVE: &str = ".local/share/lst/crash.log";

pub(crate) fn install() {
    let Some(path) = log_path() else { return };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = append(
        &path,
        &format_session_header(now(), std::process::id(), env!("CARGO_PKG_VERSION")),
    );

    let log_path = path;
    panic::set_hook(Box::new(move |info| {
        let backtrace = Backtrace::force_capture();
        let entry = format_panic_entry(
            now(),
            std::thread::current().name().unwrap_or("<unnamed>"),
            &info.to_string(),
            &backtrace.to_string(),
        );
        let _ = append(&log_path, &entry);
        eprintln!("lst panicked; details written to {}", log_path.display());
        eprintln!("{info}");
    }));
}

fn log_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(CRASH_LOG_RELATIVE))
}

fn now() -> OffsetDateTime {
    OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc())
}

fn append(path: &Path, content: &str) -> io::Result<()> {
    let mut file = OpenOptions::new().append(true).create(true).open(path)?;
    file.write_all(content.as_bytes())?;
    file.flush()
}

fn format_session_header(when: OffsetDateTime, pid: u32, version: &str) -> String {
    format!(
        "\n=== lst v{version} session started at {} (pid {pid}) ===\n",
        format_timestamp(when),
    )
}

fn format_panic_entry(when: OffsetDateTime, thread: &str, info: &str, backtrace: &str) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "\n--- PANIC at {} (thread {thread}) ---", format_timestamp(when),);
    let _ = writeln!(out, "{info}");
    let _ = writeln!(out, "backtrace:\n{backtrace}");
    out
}

fn format_timestamp(when: OffsetDateTime) -> String {
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        when.year(),
        u8::from(when.month()),
        when.day(),
        when.hour(),
        when.minute(),
        when.second(),
    )
}
