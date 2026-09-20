use std::backtrace::Backtrace;
use std::env;
use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::panic;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, Once, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

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
    let Some(file) = trace_file() else {
        return;
    };

    if let Err(err) = append_operation(file, label, bytes, lines, clipboard_read_ms, apply_ms) {
        eprintln!("lst_gpui failed to write benchmark trace: {err}");
    }
}

static PROCESS_START: OnceLock<Instant> = OnceLock::new();

/// Records when `main` started so the first-frame trace can measure from it.
pub(crate) fn mark_process_start() {
    let _ = PROCESS_START.set(Instant::now());
}

/// Records how long after `main` started a startup phase completed.
pub(crate) fn record_startup_mark(label: &str) {
    if let Some(started) = PROCESS_START.get() {
        record_ms(&format!("startup_{label}_ms"), started.elapsed().as_secs_f64() * 1000.0);
    }
}

/// Wall and thread-CPU clocks captured at the start of a frame so its cost
/// can be recorded once painting completes.
#[derive(Clone, Copy)]
pub(crate) struct FrameClock {
    wall: Instant,
    cpu_ns: u64,
}

fn thread_cpu_ns() -> u64 {
    let mut spec = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: `spec` is a valid, writable timespec for the duration of the call.
    if unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut spec) } != 0 {
        return 0;
    }
    spec.tv_sec as u64 * 1_000_000_000 + spec.tv_nsec as u64
}

/// Starts frame accounting; returns `None` when tracing is disabled so the
/// render path pays nothing.
pub(crate) fn frame_clock() -> Option<FrameClock> {
    trace_enabled().then(|| FrameClock {
        wall: Instant::now(),
        cpu_ns: thread_cpu_ns(),
    })
}

/// Records the wall and CPU cost of the frame that `clock` started.
pub(crate) fn record_frame(clock: FrameClock) {
    record_ms("frame_wall_ms", clock.wall.elapsed().as_secs_f64() * 1000.0);
    record_ms(
        "frame_cpu_ms",
        thread_cpu_ns().saturating_sub(clock.cpu_ns) as f64 / 1_000_000.0,
    );
}

/// Records when a frame's render started, in milliseconds since `main`.
pub(crate) fn record_frame_start() {
    if let Some(started) = PROCESS_START.get() {
        record_ms("frame_start_ms", started.elapsed().as_secs_f64() * 1000.0);
    }
}

/// Records the wall-clock time of an event as microseconds since the Unix
/// epoch, so an external runner can align it with its own clock.
pub(crate) fn record_epoch(label: &str) {
    if !trace_enabled() {
        return;
    }
    if let Ok(since_epoch) = SystemTime::now().duration_since(UNIX_EPOCH) {
        record_line(label, format_args!("{}", since_epoch.as_micros()));
    }
}

/// Records the reason a redraw was requested, for frame-count diagnostics.
pub(crate) fn record_notify(reason: &str) {
    record_label("notify", reason);
}

/// Records the first completed editor frame, once per process. The wall-clock
/// stamp lets an external runner measure from process spawn.
pub(crate) fn record_first_frame() {
    static RECORDED: Once = Once::new();
    RECORDED.call_once(|| {
        if !trace_enabled() {
            return;
        }
        if let Some(started) = PROCESS_START.get() {
            record_ms("startup_first_frame_ms", started.elapsed().as_secs_f64() * 1000.0);
        }
        if let Ok(since_epoch) = SystemTime::now().duration_since(UNIX_EPOCH) {
            record_line(
                "startup_first_frame_epoch_us",
                format_args!("{}", since_epoch.as_micros()),
            );
        }
    });
}

pub(crate) fn trace_enabled() -> bool {
    trace_file().is_some()
}

fn record_line(label: &str, value: std::fmt::Arguments<'_>) {
    let Some(file) = trace_file() else {
        return;
    };

    if let Err(err) = append_line(file, format_args!("{label}={value}\n")) {
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

fn trace_file() -> Option<&'static Mutex<fs::File>> {
    static CACHED: OnceLock<Option<Mutex<fs::File>>> = OnceLock::new();
    CACHED
        .get_or_init(|| {
            let path = trace_path()?;
            match OpenOptions::new().create(true).append(true).open(path) {
                Ok(file) => Some(Mutex::new(file)),
                Err(err) => {
                    eprintln!("lst_gpui failed to open benchmark trace {}: {err}", path.display());
                    None
                }
            }
        })
        .as_ref()
}

fn append_line(file: &Mutex<fs::File>, line: std::fmt::Arguments<'_>) -> io::Result<()> {
    let mut file = file.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    file.write_fmt(line)
}

fn append_operation(
    file: &Mutex<fs::File>,
    label: &str,
    bytes: usize,
    lines: usize,
    clipboard_read_ms: Option<f64>,
    apply_ms: f64,
) -> io::Result<()> {
    let mut file = file.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
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
