# lst

`lst` is a small GPUI desktop text editor.

Editor behavior lives in the framework-neutral `lst-editor` crate. The GPUI app
owns rendering, widgets, desktop integration, and runtime effects.

## Features

- Vim-style modal editing alongside standard editor keybindings, multi-cursor
  and column selection, find/replace with regex, soft wrap, and auto-pairing.
- Tree-sitter syntax highlighting for Rust, Python, JavaScript/JSX,
  TypeScript/TSX, JSON, TOML, YAML, Markdown, HTML, and CSS, with incremental
  reparsing, language injection (e.g. fenced code blocks in Markdown), and
  theme-driven colors.
- Line bookmarks: toggle with `Ctrl-Alt-K`, jump to the next/previous bookmark
  with `Ctrl-Alt-L` / `Ctrl-Alt-J` (wraps around).
- AI text cleanup for scratchpad transcripts: `Ctrl-Shift-R` (or the status-bar
  sparkle button) rewrites the buffer or current selection through DeepSeek to
  remove filler words and false starts while preserving meaning and structure;
  a single undo restores the original. Set `DEEPSEEK_API_KEY` (and optionally
  `DEEPSEEK_MODEL`) to enable it.

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
```

Running without files creates a timestamped scratchpad note in
`~/.local/share/lst/` by default. Use `--scratchpad-dir` to choose another
scratchpad directory.

## Install

`install.sh` installs the active GPUI editor to `~/.local/bin/lst` by default.

```bash
./install.sh
~/.local/bin/lst
```

Set `LST_PREFIX=/some/prefix` to change the install root.
The installer verifies that the `TX-02` font is available because the editor
uses it as the primary UI and code font.

For scratchpad window-manager rules, spawn `~/.local/bin/lst --title lst-scratchpad`.
The GPUI window sets that title on X11/Wayland and uses `lst` as its app id /
X11 `WM_CLASS`.

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

The active GPUI editor has a real-display X11 interaction benchmark. Build the
release app and runner together:

```bash
cargo build --release -p lst-gpui --bin lst --example bench_editor_x11
```

Run the full smoke suite from a real X11 session:

```bash
DISPLAY=:1 ./target/release/examples/bench_editor_x11 --scenario all --repetitions 1 --priming 0
```

There is also an opt-in real-display behavior suite for scratchpad cleanup,
clipboard, vim, multi-cursor, modifier, and whole-editor workflow coverage:

```bash
DISPLAY=:1 cargo nextest run --profile x11 -p lst-gpui --tests --run-ignored only
```

For stable baseline work, use the runner default of one priming run and seven
measured repetitions. The benchmark contract is documented in
`docs/performance-optimization.md`.

## License

GPL-3.0-or-later
