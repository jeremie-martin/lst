use crate::build_info::BUILD_IDENTITY;
use lst_editor::InputMode;
use std::{
    ffi::{OsStr, OsString},
    os::unix::ffi::OsStrExt,
    path::PathBuf,
    process,
};

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
  --                        Treat remaining arguments as file paths
  -h, --help                Print help
  -V, --version             Print build identity"
}

pub(crate) fn parse_launch_args() -> LaunchArgs {
    match parse_launch_args_from(std::env::args_os().skip(1)) {
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
    S: Into<OsString>,
{
    let mut args = LaunchArgs::default();
    let mut iter = raw_args.into_iter().map(Into::into);

    while let Some(arg) = iter.next() {
        match arg.as_bytes() {
            b"--" => {
                args.files.extend(iter.map(PathBuf::from));
                break;
            }
            b"--help" | b"-h" => return Err(LaunchArgError::Help),
            b"--version" | b"-V" => return Err(LaunchArgError::Version),
            b"--dictate" => args.dictate = true,
            b"--vim" => args.input_mode = Some(InputMode::Vim),
            b"--no-vim" => args.input_mode = Some(InputMode::Standard),
            b"--title" => {
                let title = iter
                    .next()
                    .ok_or_else(|| LaunchArgError::Message("missing value for --title".into()))?;
                args.window_title = Some(parse_title(&title)?);
            }
            b"--scratchpad-dir" => {
                let dir = iter
                    .next()
                    .ok_or_else(|| LaunchArgError::Message("missing value for --scratchpad-dir".into()))?;
                args.scratchpad_dir = Some(PathBuf::from(dir));
            }
            bytes if bytes.starts_with(b"--title=") => {
                args.window_title = Some(parse_title(OsStr::from_bytes(&bytes[b"--title=".len()..]))?);
            }
            bytes if bytes.starts_with(b"--scratchpad-dir=") => {
                args.scratchpad_dir = Some(PathBuf::from(OsStr::from_bytes(&bytes[b"--scratchpad-dir=".len()..])));
            }
            bytes if bytes.starts_with(b"--") => {
                return Err(LaunchArgError::Message(format!(
                    "unknown argument: {}",
                    arg.to_string_lossy()
                )));
            }
            _ => args.files.push(PathBuf::from(arg)),
        }
    }

    Ok(args)
}

fn parse_title(value: &OsStr) -> Result<String, LaunchArgError> {
    value
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| LaunchArgError::Message("--title must be valid UTF-8".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filesystem_arguments_preserve_bytes_and_text_options_reject_invalid_utf8() {
        let file = OsStr::from_bytes(b"note-\xff.md");
        let dir = OsStr::from_bytes(b"/tmp/notes-\xff");
        for args in [
            vec![OsStr::new("--scratchpad-dir"), dir, file],
            vec![OsStr::from_bytes(b"--scratchpad-dir=/tmp/notes-\xff"), file],
        ] {
            let parsed = parse_launch_args_from(args).unwrap();
            assert_eq!(parsed.files, [PathBuf::from(file)]);
            assert_eq!(parsed.scratchpad_dir, Some(PathBuf::from(dir)));
        }
        for args in [
            vec![OsStr::new("--title"), file],
            vec![OsStr::from_bytes(b"--title=\xff")],
        ] {
            assert_eq!(
                parse_launch_args_from(args),
                Err(LaunchArgError::Message("--title must be valid UTF-8".into()))
            );
        }
        let parsed = parse_launch_args_from([OsStr::new("--"), OsStr::from_bytes(b"--file-\xff")]).unwrap();
        assert_eq!(parsed.files, [PathBuf::from(OsStr::from_bytes(b"--file-\xff"))]);
        assert!(matches!(
            parse_launch_args_from([OsStr::from_bytes(b"--bad-\xff")]),
            Err(LaunchArgError::Message(_))
        ));
    }

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
