use crate::build_info::BUILD_IDENTITY;
use lst_editor::InputMode;
use std::{path::PathBuf, process};

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct LaunchArgs {
    pub(crate) files: Vec<PathBuf>,
    pub(crate) dictate: bool,
    pub(crate) window_title: Option<String>,
    pub(crate) scratchpad_dir: Option<PathBuf>,
    pub(crate) input_mode: Option<InputMode>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum LaunchArgError {
    Help,
    Version,
    Message(String),
}

fn usage() -> &'static str {
    "Usage:
  lst [OPTIONS] [FILES...]

Options:
  --dictate                 Open a new voice note and start recording
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
            "--dictate" => args.dictate = true,
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
    fn launch_arguments_parse_into_options_or_stop_launch() {
        let message = |text: &str| Err(LaunchArgError::Message(text.to_string()));
        for (raw, expected) in [
            (
                &["--dictate", "--vim", "notes.md"][..],
                Ok(LaunchArgs {
                    files: vec![PathBuf::from("notes.md")],
                    dictate: true,
                    input_mode: Some(InputMode::Vim),
                    ..LaunchArgs::default()
                }),
            ),
            (
                &[
                    "--title",
                    "lst-scratchpad",
                    "--scratchpad-dir=/tmp/lst-notes",
                    "--no-vim",
                    "README.md",
                ],
                Ok(LaunchArgs {
                    files: vec![PathBuf::from("README.md")],
                    window_title: Some("lst-scratchpad".to_string()),
                    scratchpad_dir: Some(PathBuf::from("/tmp/lst-notes")),
                    input_mode: Some(InputMode::Standard),
                    ..LaunchArgs::default()
                }),
            ),
            (
                &["--title=Notes", "--scratchpad-dir", "/tmp/notes"],
                Ok(LaunchArgs {
                    window_title: Some("Notes".to_string()),
                    scratchpad_dir: Some(PathBuf::from("/tmp/notes")),
                    ..LaunchArgs::default()
                }),
            ),
            (&["--version"], Err(LaunchArgError::Version)),
            (&["-V"], Err(LaunchArgError::Version)),
            // Version and help stop launch before later arguments are parsed.
            (&["README.md", "--version", "--title"], Err(LaunchArgError::Version)),
            (&["-h", "--nope"], Err(LaunchArgError::Help)),
            (&["--help"], Err(LaunchArgError::Help)),
            (&["--nope", "README.md"], message("unknown argument: --nope")),
            (&["--title"], message("missing value for --title")),
            (&["--scratchpad-dir"], message("missing value for --scratchpad-dir")),
        ] {
            assert_eq!(parse_launch_args_from(raw.iter().copied()), expected, "{raw:?}");
        }
    }
}
