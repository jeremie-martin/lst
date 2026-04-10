# lst GPUI

This crate is the GPUI rewrite track for `lst`.

It is intentionally parallel to the shipping `iced` app for now. The goal is to move the editor core onto a custom `Ropey` + GPUI stack without breaking the current release build while feature parity is still incomplete.

## Run

```sh
cd apps/lst-gpui
cargo run
DISPLAY=:1 cargo run -- path/to/file.rs
DISPLAY=:1 cargo run -- --bench-replace-corpus
DISPLAY=:1 cargo run -- --bench-append-corpus
```

## Status

- `cargo check` passes for this crate.
- `cargo build --release` passes on this host after installing `libxkbcommon-x11-dev`.
- Running under `Xvfb` still does not work here because GPUI surface creation wants a real presentation backend with DRI3 support.
- The editor uses a scroll spacer plus a viewport-sized custom-painted canvas with a small shaped-line cache.

Current real-display measurements on `DISPLAY=:1` with `target/release/lst-gpui`:

- `--bench-replace-corpus`: `apply_ms=0.734`, `action_to_next_frame_ms=44.454`
- `--bench-append-corpus`: `apply_ms=0.658`, `action_to_next_frame_ms=55.505`

## Current Features

- editable `Ropey` buffer with cursor and selection
- custom-painted viewport with gutter
- mouse positioning and drag selection
- clipboard copy/cut/paste
- multiple tabs
- open files from CLI or file dialog
- save and save-as
- retained large-paste auto-bench mode

## Missing Parity

- syntax highlighting
- search / replace
- word wrap
- Vim mode
- autosave and the rest of the current `iced` app behavior
