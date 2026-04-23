# Benchmarks

The active benchmark is the GPUI/X11 interaction runner:

```sh
cargo build --release -p lst-gpui --bin lst --example bench_editor_x11
DISPLAY=:1 ./target/release/examples/bench_editor_x11 --scenario all
```

The runner generates deterministic Rust and plain-text corpora at runtime, so
this directory does not carry stale snapshots of older editor implementations.
