pub(crate) const BUILD_IDENTITY: &str = concat!(
    "lst ",
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("LST_BUILD_GIT_SHA"),
    env!("LST_BUILD_GIT_SUFFIX"),
    ")"
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_identity_contains_package_version_and_git_revision() {
        assert!(BUILD_IDENTITY.starts_with(concat!("lst ", env!("CARGO_PKG_VERSION"), " (")));
        assert!(BUILD_IDENTITY.ends_with(')'));

        let revision = BUILD_IDENTITY
            .strip_prefix(concat!("lst ", env!("CARGO_PKG_VERSION"), " ("))
            .and_then(|identity| identity.strip_suffix(')'))
            .expect("identity should have the documented shape")
            .trim_end_matches("-dirty");
        assert!(revision == "unknown" || revision.chars().all(|character| character.is_ascii_hexdigit()));
    }
}
