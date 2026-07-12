use crate::build_info::BUILD_IDENTITY;
use lst_editor::InputMode;
use std::{fmt, path::PathBuf, process};

#[derive(Clone, Debug, Default)]
pub(crate) struct LaunchArgs {
    pub(crate) files: Vec<PathBuf>,
    pub(crate) window_title: Option<String>,
    pub(crate) scratchpad_dir: Option<PathBuf>,
    pub(crate) input_mode: Option<InputMode>,
}

#[derive(Clone, Debug)]
pub(crate) enum LaunchArgError {
    Help,
    Version,
    Message(String),
}

impl fmt::Display for LaunchArgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Help => f.write_str(usage()),
            Self::Version => f.write_str(BUILD_IDENTITY),
            Self::Message(message) => f.write_str(message),
        }
    }
}

fn usage() -> &'static str {
    "Usage:
  lst [OPTIONS] [FILES...]

Options:
  --title TITLE             Set the window title
  --scratchpad-dir PATH     Store newly created scratchpads in PATH
  --vim                     Start in Vim input mode
  --no-vim                  Start in standard input mode
  -h, --help                Print help
  -V, --version             Print build identity"
}

pub(crate) fn parse_launch_args() -> LaunchArgs {
    match parse_launch_args_from(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(LaunchArgError::Help) => {
            println!("{}", usage());
            process::exit(0);
        }
        Err(LaunchArgError::Version) => {
            println!("{BUILD_IDENTITY}");
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
            "--version" | "-V" => {
                return Err(LaunchArgError::Version);
            }
            "--vim" => args.input_mode = Some(InputMode::Vim),
            "--no-vim" => args.input_mode = Some(InputMode::Standard),
            _ if arg.starts_with("--title=") => {
                args.window_title = Some(arg["--title=".len()..].to_string());
            }
            "--title" => {
                let Some(title) = iter.next() else {
                    return Err(LaunchArgError::Message("missing value for --title".to_string()));
                };
                args.window_title = Some(title);
            }
            _ if arg.starts_with("--scratchpad-dir=") => {
                args.scratchpad_dir = Some(PathBuf::from(arg["--scratchpad-dir=".len()..].to_string()));
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
    fn version_flags_request_the_reproducible_build_identity() {
        for flag in ["--version", "-V"] {
            let error = parse_launch_args_from([flag]).expect_err("version should stop normal launch");
            assert!(matches!(error, LaunchArgError::Version));
            assert_eq!(error.to_string(), BUILD_IDENTITY);
        }
    }

    #[test]
    fn version_takes_precedence_over_graphical_launch_arguments() {
        let error = parse_launch_args_from(["README.md", "--version", "--title", "ignored"])
            .expect_err("version should stop normal launch");
        assert!(matches!(error, LaunchArgError::Version));
    }

    #[test]
    fn existing_window_and_input_arguments_remain_supported() {
        let args = parse_launch_args_from([
            "--title",
            "lst-scratchpad",
            "--scratchpad-dir=/tmp/lst-notes",
            "--no-vim",
            "README.md",
        ])
        .expect("existing arguments should parse");

        assert_eq!(args.window_title.as_deref(), Some("lst-scratchpad"));
        assert_eq!(
            args.scratchpad_dir.as_deref(),
            Some(std::path::Path::new("/tmp/lst-notes"))
        );
        assert_eq!(args.input_mode, Some(InputMode::Standard));
        assert_eq!(args.files, vec![PathBuf::from("README.md")]);
    }
}
