use std::{fmt, fs, path::PathBuf, process};

use crate::{CORPUS_PATH, PREMADE_CORPUS};

#[derive(Clone, Copy, Debug)]
pub(crate) enum BenchAction {
    Replace,
    Append,
}

impl BenchAction {
    pub(crate) fn action_name(self) -> &'static str {
        match self {
            Self::Replace => "replace",
            Self::Append => "append",
        }
    }

    pub(crate) fn operation_label(self) -> &'static str {
        match self {
            Self::Replace => "bench_replace",
            Self::Append => "bench_append",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct AutoBench {
    pub(crate) action: BenchAction,
    pub(crate) source: String,
    pub(crate) text: String,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct LaunchArgs {
    pub(crate) files: Vec<PathBuf>,
    pub(crate) auto_bench: Option<AutoBench>,
    pub(crate) window_title: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) enum LaunchArgError {
    Help,
    Message(String),
}

impl fmt::Display for LaunchArgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Help => f.write_str(usage()),
            Self::Message(message) => f.write_str(message),
        }
    }
}

fn usage() -> &'static str {
    "Usage:
  cargo run
  cargo run -- file1.rs file2.md
  cargo run -- --title \"lst GPUI\"
  cargo run -- --bench-replace-corpus
  cargo run -- --bench-append-corpus
  cargo run -- --bench-replace-file /path/to/file.rs
  cargo run -- --bench-append-file /path/to/file.rs"
}

pub(crate) fn parse_launch_args() -> LaunchArgs {
    match parse_launch_args_from(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(LaunchArgError::Help) => {
            println!("{}", usage());
            process::exit(0);
        }
        Err(LaunchArgError::Message(message)) => {
            eprintln!("{message}\n\n{}", usage());
            process::exit(2);
        }
    }
}

pub(crate) fn parse_launch_args_from<I, S>(raw_args: I) -> Result<LaunchArgs, LaunchArgError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut args = LaunchArgs::default();
    let mut iter = raw_args.into_iter().map(Into::into);

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                return Err(LaunchArgError::Help);
            }
            "--title" => {
                let Some(title) = iter.next() else {
                    return Err(LaunchArgError::Message(
                        "missing value for --title".to_string(),
                    ));
                };
                args.window_title = Some(title);
            }
            "--bench-replace-corpus" => {
                args.auto_bench = Some(AutoBench {
                    action: BenchAction::Replace,
                    source: CORPUS_PATH.to_string(),
                    text: PREMADE_CORPUS.to_string(),
                });
            }
            "--bench-append-corpus" => {
                args.auto_bench = Some(AutoBench {
                    action: BenchAction::Append,
                    source: CORPUS_PATH.to_string(),
                    text: PREMADE_CORPUS.to_string(),
                });
            }
            "--bench-replace-file" => {
                let Some(path) = iter.next() else {
                    return Err(LaunchArgError::Message(
                        "missing file path for --bench-replace-file".to_string(),
                    ));
                };
                let text = match fs::read_to_string(&path) {
                    Ok(text) => text,
                    Err(err) => {
                        return Err(LaunchArgError::Message(format!(
                            "failed to read benchmark file {path}: {err}"
                        )));
                    }
                };
                args.auto_bench = Some(AutoBench {
                    action: BenchAction::Replace,
                    source: path,
                    text,
                });
            }
            "--bench-append-file" => {
                let Some(path) = iter.next() else {
                    return Err(LaunchArgError::Message(
                        "missing file path for --bench-append-file".to_string(),
                    ));
                };
                let text = match fs::read_to_string(&path) {
                    Ok(text) => text,
                    Err(err) => {
                        return Err(LaunchArgError::Message(format!(
                            "failed to read benchmark file {path}: {err}"
                        )));
                    }
                };
                args.auto_bench = Some(AutoBench {
                    action: BenchAction::Append,
                    source: path,
                    text,
                });
            }
            _ if arg.starts_with("--") => {
                return Err(LaunchArgError::Message(format!("unknown argument: {arg}")));
            }
            _ => args.files.push(PathBuf::from(arg)),
        }
    }

    Ok(args)
}
