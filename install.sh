#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
prefix="${LST_PREFIX:-$HOME/.local}"

cd "$repo_root"

if ! command -v cargo >/dev/null 2>&1; then
    echo "cargo is required to install lst" >&2
    exit 1
fi

if ! command -v git >/dev/null 2>&1; then
    echo "git is required to identify the lst source revision" >&2
    exit 1
fi

if ! command -v fc-match >/dev/null 2>&1; then
    echo "fontconfig is required to verify the TX-02 font" >&2
    exit 1
fi

if [[ "$(fc-match -f '%{family[0]}' ':family=TX-02')" != "TX-02" ]]; then
    echo "TX-02 is required. Install it and refresh fontconfig before installing lst." >&2
    exit 1
fi

git_sha="$(git rev-parse HEAD)"
if [[ -n "$(git status --porcelain=v1)" ]]; then
    git_dirty=1
else
    git_dirty=0
fi

source_identity="$(
    env LST_BUILD_GIT_SHA="$git_sha" LST_BUILD_GIT_DIRTY="$git_dirty" \
        cargo run --quiet --release --locked -p lst-gpui --bin lst -- --version
)"

env LST_BUILD_GIT_SHA="$git_sha" LST_BUILD_GIT_DIRTY="$git_dirty" \
    cargo install --path apps/lst-gpui --locked --profile release --root "$prefix" --force --bin lst

installed_binary="$prefix/bin/lst"
installed_identity="$("$installed_binary" --version)"

if [[ "$installed_identity" != "$source_identity" ]]; then
    echo "installed lst does not match the source build" >&2
    echo "  source:    $source_identity" >&2
    echo "  installed: $installed_identity" >&2
    exit 1
fi

cat <<EOF
Installed the active GPUI editor to:
  $installed_binary

Verified build:
  $installed_identity

Make sure $prefix/bin is on your PATH.
EOF
