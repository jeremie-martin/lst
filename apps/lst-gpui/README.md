# lst GPUI

This crate is the GPUI rewrite track for `lst`.

It is intentionally parallel to the shipping `iced` app for now. The current implementation extracts the reusable document/search/line-edit logic into `crates/lst-core` and keeps the GPUI app focused on rendering, input plumbing, clipboard integration, and benchmarking.

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

- `--bench-replace-corpus`: `apply_ms=1.262`, `action_to_next_frame_ms=55.391`
- `--bench-append-corpus`: `apply_ms=1.342`, `action_to_next_frame_ms=66.637`

## Current Features

- editable `Ropey` buffer with cursor and selection
- custom-painted dark viewport with gutter and soft wrap
- mouse positioning, drag selection, double-click word selection, and triple-click line selection
- clipboard copy/cut/paste
- multiple tabs
- open files from CLI or file dialog
- save and save-as
- background autosave for file-backed dirty tabs
- undo / redo
- find / replace overlay
- goto-line overlay
- line operations: delete, duplicate, move up/down, toggle comment
- Vim normal / insert / visual / visual-line modes
- visual up/down movement across wrapped rows
- retained large-paste auto-bench mode

## Missing Parity

- syntax highlighting
- full parity with the current `iced` app behavior surface
