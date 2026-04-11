# lst

`lst` is being rebuilt around the GPUI implementation in `apps/lst-gpui`.
The active editor behavior lives in framework-neutral crates under `crates/`,
and GPUI owns the rendering and desktop integration boundary.

The old iced implementation has been archived under `legacy/iced-lst`. It is
not part of the active workspace and should not be used as a source of shared
modules for new editor work.

## Active Layout

- `apps/lst-gpui`: active GPUI desktop editor.
- `crates/lst-core`: document, selection, find, wrap, and low-level editor operations.
- `crates/lst-editor`: framework-neutral editor model, commands, effects, and Vim state machine.
- `crates/lst-ui`: reusable GPUI shell widgets.
- `benchmarks`: shared benchmark corpora for active performance tests.
- `legacy/iced-lst`: archived iced editor, kept for historical reference only.

## Build And Run

```bash
cargo build --release -p lst-gpui
./target/release/lst-gpui
./target/release/lst-gpui README.md
./target/release/lst-gpui --title "lst GPUI"
```

## Install

`install.sh` installs the active GPUI editor to `~/.local/bin/lst-gpui` by default.

```bash
./install.sh
~/.local/bin/lst-gpui
```

Set `LST_PREFIX=/some/prefix` to change the install root.

## Testing

Use the workspace suite as the active refactor gate:

```bash
cargo test
```

For deeper Vim state-machine coverage in the editor crate:

```bash
cargo test -p lst-editor --features internal-invariants
```

## Benchmarks

Build the GPUI benchmark tools from the workspace root:

```bash
cargo build --release -p lst-gpui --bin lst-gpui --example bench_editor_x11 --example bench_syntax_highlight
```

Run the real-display X11 benchmark suite:

```bash
./target/release/examples/bench_editor_x11 --scenario all
```

The current performance workflow is documented in `docs/performance-optimization.md`.

## License

GPL-3.0-or-later
