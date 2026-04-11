# lst GPUI

This crate is the active desktop editor for `lst`.

The current implementation keeps reusable document/search/line-edit logic and framework-neutral editor behavior in `crates/lst-editor`, while GPUI rendering, shell widgets, and desktop integration stay in this crate.
GPUI should keep moving product behavior into `lst-editor` and remain focused on rendering and framework boundary work.

## Run

```sh
cd apps/lst-gpui
cargo run
DISPLAY=:1 cargo run -- path/to/file.rs
DISPLAY=:1 cargo run -- --title "lst GPUI"
```

## Status

- `cargo check` passes for this crate.
- `cargo build --release` passes on this host after installing `libxkbcommon-x11-dev`.
- The real-display X11 benchmark runner is available as `bench_editor_x11`.
- Running under `Xvfb` still does not work here because GPUI surface creation wants a real presentation backend with DRI3 support.
- The editor uses a scroll spacer plus a viewport-sized custom-painted canvas with a small shaped-line cache.

## Benchmark

```sh
cargo build --release -p lst-gpui --bin lst-gpui --example bench_editor_x11
DISPLAY=:1 ../../target/release/examples/bench_editor_x11 --scenario all --repetitions 1 --priming 0
```

Use the runner defaults for real baseline work.

## Current Features

- editable `Ropey` buffer with cursor and selection
- custom-painted dark viewport with gutter and soft wrap
- minimal GPUI shell with real tab strip, close affordances, inline find/replace, inline goto-line, and status bar
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
- Rust syntax highlighting via tree-sitter in the custom viewport
- Vim normal / insert / visual / visual-line modes
- visual up/down movement across wrapped rows

## Missing Parity

- remaining editor behaviors that have not yet moved behind `lst-editor`
