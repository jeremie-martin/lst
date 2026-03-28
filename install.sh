#!/usr/bin/env bash
set -euo pipefail

prefix="${LST_PREFIX:-$HOME/.local}"
font_path="/usr/share/fonts/jetbrains-mono/JetBrainsMono[wght].ttf"

if ! command -v cargo >/dev/null 2>&1; then
    echo "cargo is required to install lst" >&2
    exit 1
fi

if [[ ! -f "$font_path" ]]; then
    echo "JetBrains Mono is required at $font_path" >&2
    exit 1
fi

cargo install --path . --locked --root "$prefix"

cat <<EOF
Installed lst to $prefix/bin/lst

Make sure $prefix/bin is on your PATH.
EOF
