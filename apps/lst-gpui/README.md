# lst GPUI

This crate is the GPUI rewrite track for `lst`.

It is intentionally parallel to the shipping `iced` app for now. The current implementation extracts the reusable document/search/line-edit logic into `crates/lst-core`, keeps the custom-painted editor surface in this crate, and moves the shell layer into `crates/lst-ui` for tabs and inline command inputs.

## Run

```sh
cd apps/lst-gpui
cargo run
DISPLAY=:1 cargo run -- path/to/file.rs
DISPLAY=:1 cargo run -- --title "lst GPUI"
DISPLAY=:1 cargo run -- --bench-replace-corpus
DISPLAY=:1 cargo run -- --bench-append-corpus
```

## Benchmarks

Build the GPUI editor benchmarks from the workspace root:

```sh
cargo build --release -p lst-gpui --bin lst-gpui --example bench_editor_x11 --example bench_syntax_highlight
```

Run the real-display X11 editor benchmark suite:

```sh
./target/release/examples/bench_editor_x11 --scenario all
```

The editor runner supports `large-paste`, `typing-medium`, `typing-large`,
`scroll-highlighted`, `scroll-plain`, `open-large`, and `search-large`.
Each scenario prints one `primary_metric` and waits for completed work before
reporting measured runs.

Run the production tree-sitter highlighting benchmark separately:

```sh
cargo run --release -p lst-gpui --example bench_syntax_highlight -- --backend tree-sitter-highlight --language rust --iterations 7
```

See [docs/performance-optimization.md](/home/jmartin/lst/docs/performance-optimization.md)
for the metric contract and completion conditions.

## Status

- `cargo check` passes for this crate.
- `cargo build --release` passes on this host after installing `libxkbcommon-x11-dev`.
- Running under `Xvfb` still does not work here because GPUI surface creation wants a real presentation backend with DRI3 support.
- The editor uses a scroll spacer plus a viewport-sized custom-painted canvas with a small shaped-line cache.

Current real-display measurements on `DISPLAY=:1` with `target/release/lst-gpui`:

- `--bench-replace-corpus`: `apply_ms=1.685`, `action_to_next_frame_ms=77.713`
- `--bench-append-corpus`: `apply_ms=1.776`, `action_to_next_frame_ms=77.721`

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
- retained large-paste auto-bench mode

## Missing Parity

- non-Rust syntax highlighting
- full parity with the current `iced` app behavior surface
