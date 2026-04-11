use std::{fmt, path::PathBuf, process};

#[derive(Clone, Debug, Default)]
pub(crate) struct LaunchArgs {
    pub(crate) files: Vec<PathBuf>,
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
  cargo run -- --title \"lst GPUI\""
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
            _ if arg.starts_with("--") => {
                return Err(LaunchArgError::Message(format!("unknown argument: {arg}")));
            }
            _ => args.files.push(PathBuf::from(arg)),
        }
    }

    Ok(args)
}
