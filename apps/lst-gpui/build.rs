use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
};

fn main() {
    println!("cargo:rerun-if-env-changed=LST_BUILD_GIT_SHA");
    println!("cargo:rerun-if-env-changed=LST_BUILD_GIT_DIRTY");
    println!("cargo:rerun-if-changed=build.rs");

    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("Cargo sets CARGO_MANIFEST_DIR"));
    emit_repository_inputs(&manifest_dir);

    let git_sha = env::var("LST_BUILD_GIT_SHA")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| git_output(&manifest_dir, &["rev-parse", "HEAD"]))
        .unwrap_or_else(|| "unknown".to_string());
    let dirty = env::var("LST_BUILD_GIT_DIRTY")
        .ok()
        .and_then(|value| parse_dirty(&value))
        .unwrap_or_else(|| repository_is_dirty(&manifest_dir));

    println!("cargo:rustc-env=LST_BUILD_GIT_SHA={git_sha}");
    println!(
        "cargo:rustc-env=LST_BUILD_GIT_SUFFIX={}",
        if dirty { "-dirty" } else { "" }
    );
}

fn parse_dirty(value: &str) -> Option<bool> {
    match value {
        "1" | "true" => Some(true),
        "0" | "false" => Some(false),
        _ => None,
    }
}

fn repository_is_dirty(manifest_dir: &Path) -> bool {
    git_output(manifest_dir, &["status", "--porcelain=v1"]).is_some_and(|status| !status.is_empty())
}

fn emit_repository_inputs(manifest_dir: &Path) {
    let Some(repository_root) = git_output(manifest_dir, &["rev-parse", "--show-toplevel"]).map(PathBuf::from) else {
        return;
    };

    if let Some(paths) = git_output(&repository_root, &["ls-files"]) {
        for path in paths.lines().filter(|path| !path.is_empty()) {
            let path = repository_root.join(path);
            // Git's index covers tracked additions/removals. A parent-directory
            // watch here could include Cargo's own target output and self-trigger.
            if path.exists() {
                emit_watch(&path);
            }
        }
    }

    for name in [
        Some("HEAD"),
        Some("index"),
        git_output(manifest_dir, &["symbolic-ref", "-q", "HEAD"]).as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        if let Some(path) = git_output(
            &repository_root,
            &["rev-parse", "--path-format=absolute", "--git-path", name],
        ) {
            emit_watch(Path::new(&path));
        }
    }
    // Packed refs live in the common directory, including for linked worktrees.
    // A loose ref's deletion already triggers a rebuild when it is first packed.
    if let Some(path) = git_output(
        &repository_root,
        &["rev-parse", "--path-format=absolute", "--git-path", "packed-refs"],
    ) {
        if Path::new(&path).exists() {
            emit_watch(Path::new(&path));
        }
    }
}

fn emit_watch(mut path: &Path) {
    // Cargo considers a missing watched file perpetually changed. Watch its
    // nearest existing directory instead, so later creation is still observed.
    while !path.exists() {
        let Some(parent) = path.parent() else { return };
        path = parent;
    }
    println!("cargo:rerun-if-changed={}", path.display());
}

fn git_output(manifest_dir: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("--no-optional-locks")
        .arg("-C")
        .arg(manifest_dir)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}
