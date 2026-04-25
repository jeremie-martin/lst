use std::backtrace::Backtrace;
use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::{self, Write as _};
use std::panic;
use std::path::{Path, PathBuf};

use time::OffsetDateTime;

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
    let _ = writeln!(
        out,
        "\n--- PANIC at {} (thread {thread}) ---",
        format_timestamp(when),
    );
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

#[cfg(test)]
mod tests {
    use super::*;
    use time::{Date, Month, Time};

    fn fixed_time() -> OffsetDateTime {
        let date = Date::from_calendar_date(2026, Month::April, 25).unwrap();
        let time = Time::from_hms(14, 30, 0).unwrap();
        OffsetDateTime::new_utc(date, time)
    }

    #[test]
    fn session_header_includes_version_pid_and_timestamp() {
        let header = format_session_header(fixed_time(), 12345, "0.1.0");
        assert!(
            header.contains("lst v0.1.0"),
            "header missing version: {header}"
        );
        assert!(
            header.contains("2026-04-25 14:30:00"),
            "header missing timestamp: {header}"
        );
        assert!(header.contains("pid 12345"), "header missing pid: {header}");
        assert!(
            header.starts_with('\n'),
            "header should start with newline so entries don't run together"
        );
    }

    #[test]
    fn panic_entry_includes_thread_info_and_backtrace() {
        let entry = format_panic_entry(
            fixed_time(),
            "main",
            "panicked at 'oops', src/foo.rs:1:1",
            "0: lst::main\n1: core::ops::function::FnOnce::call_once",
        );
        assert!(entry.contains("--- PANIC at 2026-04-25 14:30:00 (thread main) ---"));
        assert!(entry.contains("panicked at 'oops', src/foo.rs:1:1"));
        assert!(entry.contains("backtrace:\n0: lst::main"));
    }

    #[test]
    fn append_creates_file_and_concatenates_writes() {
        let dir = std::env::temp_dir().join(format!("lst-crash-log-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("crash.log");
        let _ = fs::remove_file(&path);

        append(&path, "first\n").unwrap();
        append(&path, "second\n").unwrap();
        let contents = fs::read_to_string(&path).unwrap();
        assert_eq!(contents, "first\nsecond\n");

        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir(&dir);
    }
}
