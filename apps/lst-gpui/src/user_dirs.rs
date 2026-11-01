//! The user's home and XDG base directories, validated where they enter the
//! app. The XDG Base Directory specification requires an unset, empty, or
//! relative `XDG_*_HOME` to be ignored in favor of its `HOME` default, and a
//! relative `HOME` locates nothing: a path built from either would resolve
//! against whatever the working directory happens to be.

use std::{
    env,
    ffi::OsString,
    path::{Path, PathBuf},
};

/// The absolute home directory, if `HOME` provides one.
pub(crate) fn home_dir() -> Option<PathBuf> {
    absolute(env::var_os("HOME"))
}

/// `$XDG_CONFIG_HOME`, defaulting to `~/.config`.
pub(crate) fn config_home() -> Option<PathBuf> {
    base_dir(env::var_os("XDG_CONFIG_HOME"), env::var_os("HOME"), ".config")
}

/// `$XDG_STATE_HOME`, defaulting to `~/.local/state`.
pub(crate) fn state_home() -> Option<PathBuf> {
    base_dir(env::var_os("XDG_STATE_HOME"), env::var_os("HOME"), ".local/state")
}

fn base_dir(xdg: Option<OsString>, home: Option<OsString>, default: &str) -> Option<PathBuf> {
    absolute(xdg).or_else(|| absolute(home).map(|home| home.join(default)))
}

fn absolute(value: Option<OsString>) -> Option<PathBuf> {
    value.map(PathBuf::from).filter(|path| Path::is_absolute(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unset_empty_or_relative_values_never_resolve_against_the_working_directory() {
        let home = Some(OsString::from("/home/person"));
        for xdg in [None, Some(OsString::new()), Some("relative".into())] {
            assert_eq!(
                base_dir(xdg, home.clone(), ".local/state"),
                Some("/home/person/.local/state".into())
            );
        }
        assert_eq!(
            base_dir(Some("/state".into()), Some("relative".into()), ".local/state"),
            Some("/state".into())
        );
        for home in [None, Some(OsString::new()), Some("relative".into())] {
            assert!(base_dir(None, home.clone(), ".config").is_none());
            assert!(absolute(home).is_none());
        }
    }
}
