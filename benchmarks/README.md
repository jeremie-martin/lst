# Benchmarks

The active end-to-end benchmark is the GPUI/X11 interaction runner:

```sh
cargo build --release -p lst-gpui --bin lst --example bench_editor_x11
DISPLAY=:1 ./target/release/examples/bench_editor_x11 --scenario all
```

The runner generates deterministic Rust and plain-text corpora at runtime, so
this directory does not carry stale snapshots of older editor implementations.

The framework-neutral editor crate also has non-X11 Criterion benchmarks:

```sh
cargo bench -p lst-editor --bench editor_model
cargo bench -p lst-editor --bench vim_model
```

Use `editor_model` to isolate regular editor model costs from GPUI, X11,
desktop clipboard, file saving, and redraw latency. Use `vim_model` for the Vim
state machine and Vim command execution surface.
