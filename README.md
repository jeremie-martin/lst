# lst

`lst` is being rebuilt around the GPUI implementation in `apps/lst-gpui`.
The active editor behavior lives in the framework-neutral `lst-editor` crate,
and GPUI owns rendering, widgets, and desktop integration.

The old iced implementation has been removed from this repository. Historical
code should not be used as a source of shared modules for new editor work.

## Active Layout

- `apps/lst-gpui`: active GPUI desktop editor.
- `crates/lst-editor`: framework-neutral editor model, document primitives, effects, and Vim state machine.

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

## Performance

There is no maintained benchmark contract in this repository right now.
Performance work should start from a focused behavioral scenario and add a
narrow measurement only when it is needed.

## License

GPL-3.0-or-later
