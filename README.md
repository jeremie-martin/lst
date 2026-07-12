# lst

`lst` is a small GPUI desktop text editor.

Editor behavior lives in the framework-neutral `lst-editor` crate. The GPUI app
owns rendering, widgets, desktop integration, and runtime effects.

## Features

- Standard desktop editing by default, with multi-cursor and column selection,
  complete find/replace, soft wrap, auto-pairing, and optional Vim mode.
- Searchable command palette, application menu, visible settings, versioned
  TOML configuration with live reload, and configurable keybindings.
- Scratchpad-only autosave by default and explicit save/discard/cancel handling
  for modified ordinary files.
- Tree-sitter syntax highlighting for Rust, Python, JavaScript/JSX,
  TypeScript/TSX, JSON, TOML, YAML, Markdown, HTML, and CSS, with incremental
  reparsing, language injection (e.g. fenced code blocks in Markdown), and
  theme-driven colors.
- Line bookmarks: toggle with `Ctrl-Alt-K`, jump to the next/previous bookmark
  with `Ctrl-Alt-L` / `Ctrl-Alt-J` (wraps around).
- AI text cleanup for scratchpad transcripts is available from the command
  palette. A selection is cleaned directly; cleaning an entire document first
  requires an explicit in-app confirmation. A single undo restores the
  original. Set `DEEPSEEK_API_KEY` (and optionally `DEEPSEEK_MODEL`) to enable
  it.

## Active Layout

- `apps/lst-gpui`: active GPUI desktop editor.
- `crates/lst-editor`: framework-neutral editor model, document primitives, effects, and Vim state machine.

## Build And Run

```bash
cargo build --release -p lst-gpui
./target/release/lst
./target/release/lst README.md
./target/release/lst --title lst-scratchpad
./target/release/lst --scratchpad-dir /path/to/notes
./target/release/lst --vim README.md
./target/release/lst --version
```

Running without files creates a timestamped scratchpad note in
`~/.local/share/lst/` by default. Use `--scratchpad-dir` to choose another
scratchpad directory.

Open the command palette with `Ctrl-Shift-P` and settings with `Ctrl-,`.
Configuration is stored in `$XDG_CONFIG_HOME/lst/config.toml` or
`~/.config/lst/config.toml`. See `docs/daily-driver.md` for the default behavior,
setting schema, and standard key policy.

## Install

`install.sh` builds and installs the active GPUI editor in release mode to
`~/.local/bin/lst` by default. It then compares the installed package version,
Git revision, and dirty marker with the source build and fails on any mismatch.

```bash
./install.sh
~/.local/bin/lst --version
```

Set `LST_PREFIX=/some/prefix` to change the install root.
The installer verifies that the `TX-02` font is available because it is the
default editor font. Application chrome uses the platform UI font.

For scratchpad window-manager rules, spawn `~/.local/bin/lst --title lst-scratchpad`.
The GPUI window sets that title on X11/Wayland and uses `lst` as its app id /
X11 `WM_CLASS`.

## Testing

Use the workspace suite as the active refactor gate:

```bash
cargo test
```

Run accepted desktop behavior through a real, off-screen X11 server:

```bash
./scripts/run_x11_nested.py
```

This requires a host X11 session plus `Xephyr`, `lwm`, `wmctrl`, `xclip`, and
Python Xlib. It sends real XTEST keyboard and mouse input to the production app
without taking over the visible desktop. See `docs/x11-harness.md` for focused,
stress, physical-display, and real-vs-nested qualification commands.

For deeper Vim state-machine coverage in the editor crate:

```bash
cargo test -p lst-editor --features internal-invariants
```

## Performance

The active GPUI editor has a real-display X11 interaction benchmark. Build the
release app and runner together:

```bash
cargo build --release -p lst-gpui --bin lst --example bench_editor_x11
```

Run the full smoke suite from a real X11 session:

```bash
DISPLAY=:1 ./target/release/examples/bench_editor_x11 --scenario all --repetitions 1 --priming 0
```

The physical-display form remains available for visual baselines and explicit
diagnostics:

```bash
DISPLAY=:1 cargo nextest run --profile x11 -p lst-gpui --tests --run-ignored only
```

For stable baseline work, use the runner default of one priming run and seven
measured repetitions. The benchmark contract is documented in
`docs/performance-optimization.md`.

## License

GPL-3.0-or-later
