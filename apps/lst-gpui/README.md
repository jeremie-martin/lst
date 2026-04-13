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
DISPLAY=:1 cargo run -- --scratchpad-dir /path/to/notes
```

The installed command is `lst`. The installer also creates a `lst-gpui`
compatibility alias for older scripts.

## Status

- `cargo check` passes for this crate.
- `cargo build --release` passes on this host after installing `libxkbcommon-x11-dev`.
- The real-display X11 benchmark runner is available as `bench_editor_x11`.
- Running under `Xvfb` still does not work here because GPUI surface creation wants a real presentation backend with DRI3 support.
- The editor uses a scroll spacer plus a viewport-sized custom-painted canvas with a small shaped-line cache.

## Benchmark

```sh
cargo build --release -p lst-gpui --bin lst --example bench_editor_x11
DISPLAY=:1 ../../target/release/examples/bench_editor_x11 --scenario all --repetitions 1 --priming 0
```

Use the runner defaults for real baseline work.

Run the opt-in real-display behavior smoke test with:

```sh
DISPLAY=:1 cargo test -p lst-gpui --test real_x11_smoke -- --ignored --nocapture
```

## Current Features

- editable `Ropey` buffer with cursor and selection
- custom-painted dark viewport with gutter and soft wrap
- minimal GPUI shell with real tab strip, close affordances, inline find/replace, inline goto-line, and status bar
- mouse positioning, drag selection, double-click word selection, and triple-click line selection
- clipboard copy/cut/paste
- multiple tabs
- open files from CLI or file dialog
- timestamped scratchpad notes in `~/.local/share/lst/` by default
- save, save-as, and background autosave for path-backed dirty tabs
- undo / redo
- find / replace overlay
- goto-line overlay
- line operations: delete, duplicate, move up/down, toggle comment
- Rust syntax highlighting via tree-sitter in the custom viewport
- Vim normal / insert / visual / visual-line modes
- visual up/down movement across wrapped rows

## Missing Parity

- remaining editor behaviors that have not yet moved behind `lst-editor`
