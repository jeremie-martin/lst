use std::{
    env,
    fs::OpenOptions,
    io::{self, Write},
    path::PathBuf,
};

pub(crate) fn record_ms(label: &str, value: f64) {
    record_line(label, format_args!("{value:.3}"));
}

pub(crate) fn record_usize(label: &str, value: usize) {
    record_line(label, format_args!("{value}"));
}

pub(crate) fn record_label(label: &str, value: &str) {
    record_line(label, format_args!("{value}"));
}

pub(crate) fn record_operation(
    label: &str,
    bytes: usize,
    lines: usize,
    clipboard_read_ms: Option<f64>,
    apply_ms: f64,
) {
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

fn trace_path() -> Option<PathBuf> {
    let path = env::var_os("LST_BENCH_TRACE_FILE")?;
    if path.is_empty() {
        return None;
    }
    Some(path.into())
}

fn append_line(path: PathBuf, line: std::fmt::Arguments<'_>) -> io::Result<()> {
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_fmt(line)
}

fn append_operation(
    path: PathBuf,
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
