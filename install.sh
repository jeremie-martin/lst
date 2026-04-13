#!/usr/bin/env bash
set -euo pipefail

prefix="${LST_PREFIX:-$HOME/.local}"

if ! command -v cargo >/dev/null 2>&1; then
    echo "cargo is required to install lst" >&2
    exit 1
fi

if ! command -v fc-match >/dev/null 2>&1; then
    echo "fontconfig is required to verify the TX-02 font" >&2
    exit 1
fi

if ! fc-match 'TX\-02' | grep -qi 'TX-02'; then
    echo "TX-02 is required. Install it and refresh fontconfig before installing lst." >&2
    exit 1
fi

cargo install --path apps/lst-gpui --locked --root "$prefix" --force --bin lst
ln -sf lst "$prefix/bin/lst-gpui"

cat <<EOF
Installed the active GPUI editor to:
  $prefix/bin/lst
  $prefix/bin/lst-gpui -> lst

Make sure $prefix/bin is on your PATH.
EOF
