use std::{fmt, path::PathBuf, process};

#[derive(Clone, Debug, Default)]
pub(crate) struct LaunchArgs {
    pub(crate) files: Vec<PathBuf>,
    pub(crate) window_title: Option<String>,
    pub(crate) scratchpad_dir: Option<PathBuf>,
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
  cargo run -- --scratchpad-dir /path/to/notes"
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
            _ if arg.starts_with("--title=") => {
                args.window_title = Some(arg["--title=".len()..].to_string());
            }
            "--title" => {
                let Some(title) = iter.next() else {
                    return Err(LaunchArgError::Message(
                        "missing value for --title".to_string(),
                    ));
                };
                args.window_title = Some(title);
            }
            _ if arg.starts_with("--scratchpad-dir=") => {
                args.scratchpad_dir =
                    Some(PathBuf::from(arg["--scratchpad-dir=".len()..].to_string()));
            }
            "--scratchpad-dir" => {
                let Some(dir) = iter.next() else {
                    return Err(LaunchArgError::Message(
                        "missing value for --scratchpad-dir".to_string(),
                    ));
                };
                args.scratchpad_dir = Some(PathBuf::from(dir));
            }
            _ if arg.starts_with("--") => {
                return Err(LaunchArgError::Message(format!("unknown argument: {arg}")));
            }
            _ => args.files.push(PathBuf::from(arg)),
        }
    }

    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_args_accept_window_title() {
        let args = parse_launch_args_from(["--title", "lst-window", "/tmp/example.rs"])
            .expect("args should parse");

        assert_eq!(args.window_title.as_deref(), Some("lst-window"));
        assert_eq!(args.files, [PathBuf::from("/tmp/example.rs")]);
    }

    #[test]
    fn launch_args_accept_scratchpad_dir() {
        let args = parse_launch_args_from([
            "--scratchpad-dir",
            "/tmp/lst-notes",
            "--scratchpad-dir=/tmp/lst-other-notes",
        ])
        .expect("args should parse");

        assert_eq!(
            args.scratchpad_dir,
            Some(PathBuf::from("/tmp/lst-other-notes"))
        );
        assert!(args.files.is_empty());
    }

    #[test]
    fn launch_args_require_title_value() {
        let error = parse_launch_args_from(["--title"]).expect_err("missing title");

        assert!(matches!(
            error,
            LaunchArgError::Message(message) if message == "missing value for --title"
        ));
    }

    #[test]
    fn launch_args_require_scratchpad_dir_value() {
        let error =
            parse_launch_args_from(["--scratchpad-dir"]).expect_err("missing scratchpad dir");

        assert!(matches!(
            error,
            LaunchArgError::Message(message)
                if message == "missing value for --scratchpad-dir"
        ));
    }
}
