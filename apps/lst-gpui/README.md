# lst GPUI

This crate is the active desktop editor for `lst`.

Reusable document, search, line-edit, and editor behavior belongs in
`crates/lst-editor`. This crate should stay focused on rendering, input
adaptation, desktop integration, and runtime effects.

## Run

```sh
cd apps/lst-gpui
cargo run
DISPLAY=:1 cargo run -- path/to/file.rs
DISPLAY=:1 cargo run -- --title lst-scratchpad
DISPLAY=:1 cargo run -- --scratchpad-dir /path/to/notes
```

The installed command is `lst`. Feature status is tracked in
`../../docs/editor-behaviors-checklist.md`.

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

## Verification

```sh
cargo test --all-features
cargo clippy --all-targets --all-features
```
