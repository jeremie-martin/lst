use std::{fs, path::PathBuf, process};

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
}

fn usage() -> &'static str {
    "Usage:
  cargo run
  cargo run -- file1.rs file2.md
  cargo run -- --bench-replace-corpus
  cargo run -- --bench-append-corpus
  cargo run -- --bench-replace-file /path/to/file.rs
  cargo run -- --bench-append-file /path/to/file.rs"
}

pub(crate) fn parse_launch_args() -> LaunchArgs {
    let mut args = LaunchArgs::default();
    let mut iter = std::env::args().skip(1);

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                println!("{}", usage());
                process::exit(0);
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
                    eprintln!("missing file path for --bench-replace-file\n\n{}", usage());
                    process::exit(2);
                };
                let text = match fs::read_to_string(&path) {
                    Ok(text) => text,
                    Err(err) => {
                        eprintln!("failed to read benchmark file {path}: {err}");
                        process::exit(2);
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
                    eprintln!("missing file path for --bench-append-file\n\n{}", usage());
                    process::exit(2);
                };
                let text = match fs::read_to_string(&path) {
                    Ok(text) => text,
                    Err(err) => {
                        eprintln!("failed to read benchmark file {path}: {err}");
                        process::exit(2);
                    }
                };
                args.auto_bench = Some(AutoBench {
                    action: BenchAction::Append,
                    source: path,
                    text,
                });
            }
            _ if arg.starts_with("--") => {
                eprintln!("unknown argument: {arg}\n\n{}", usage());
                process::exit(2);
            }
            _ => args.files.push(PathBuf::from(arg)),
        }
    }

    args
}
