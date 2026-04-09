# lst

**Lucy's Simple Text editor**, a fast, minimal, good-looking text editor for Linux.

`lst` is a native Linux text editor built with [`iced`](https://iced.rs). It focuses on fast startup, low-latency typing, and a clean UI with just the editing features you actually use.

## Highlights

- Catppuccin Mocha theme with GPU-rendered UI
- Multiple tabs, tab cycling, and tab reordering
- Open files from the CLI or with the file picker
- Scratchpad tabs: starting `lst` without files, or pressing `Ctrl+N`, creates a timestamped Markdown note
- Autosave every 500 ms for modified tabs
- Find, replace, and go to line
- Rust syntax highlighting via a lightweight tree-sitter path, many other languages via `syntect`, plus custom Markdown highlighting
- Word wrap, grouped undo and redo, auto-indent, and line numbers
- Line selection and editing helpers, including gutter click, word delete, duplicate line, move line, and delete line
- Vim-style modal editing with Insert, Normal, Visual, and Visual Line modes
- Status bar with file info, cursor position, wrap state, and Vim mode when active

## Common Shortcuts

- `Ctrl+N` new scratchpad, `Ctrl+O` open, `Ctrl+S` save, `Ctrl+Shift+S` save as, `Ctrl+W` close tab, `Ctrl+Q` quit
- `Ctrl+F` find, `Ctrl+H` find and replace, `Ctrl+G` go to line
- `Ctrl+Tab` / `Ctrl+Shift+Tab` switch tabs, `Ctrl+Shift+PageUp` / `Ctrl+Shift+PageDown` reorder tabs
- `Ctrl+Z` undo, `Ctrl+Shift+Z` redo, `Alt+Z` toggle word wrap
- `Ctrl+Backspace` / `Ctrl+Delete` delete by word
- `Ctrl+Shift+K` delete line, `Alt+Up` / `Alt+Down` move line, `Ctrl+Shift+D` duplicate line
- `Ctrl+L` select line, `Shift+Click` extend selection, click the gutter to select a full line
- `Tab` / `Shift+Tab` indent or unindent, `Enter` keeps the current indentation
- `Esc` enters Vim Normal mode, `/` starts find in Vim mode, `n` / `N` move between matches

## Install

`install.sh` installs `lst` to `~/.local/bin/lst` by default.

Requirements:

- `cargo`
- JetBrains Mono installed at `/usr/share/fonts/jetbrains-mono/JetBrainsMono[wght].ttf` if you use `install.sh`

```bash
./install.sh
~/.local/bin/lst
```

Set `LST_PREFIX=/some/prefix` to change the install root.

## Build and Run

A recent stable Rust toolchain is enough to build from source.

```bash
cargo build --release
./target/release/lst
./target/release/lst README.md
./target/release/lst README.md src/main.rs
./target/release/lst --scratchpad-dir ~/notes
./target/release/lst --title lst-scratchpad
```

At runtime, `lst` prefers `TX-02`, then `JetBrains Mono`, then the system monospace font.

## Testing

Use the default suite as the blind refactor gate:

```bash
cargo test
```

That suite is intended to stay focused on user-visible behavior and refactor-stable contracts.
In practice, it compiles the integration-style suites under `tests/` and does not compile the source-file unit tests in `src/`.

## Benchmarking

The current performance optimization workflow is documented in [docs/performance-optimization.md](/home/jmartin/lst/docs/performance-optimization.md).

Build both binaries, then run the recommended next X11 real-display paste benchmark:

```bash
cargo build --release --bin lst --bin bench_paste_x11
./target/release/bench_paste_x11
```

The benchmark prints diagnostics plus a final `score=...` line. The current paste benchmark score is median process CPU time for a fixed real-display pure-append paste trace against `benchmarks/paste-corpus.rs`, and lower is better. The current default Rust highlighting path is the tree-sitter backend; the paste benchmark setup uses real `Ctrl+A`, `Ctrl+C`, `Ctrl+End`, and the separate scroll benchmark remains available via `bench_scroll_x11`.

Recent benchmark attribution notes, including the Rust highlighter comparison and the `syntect` fallback command, are in [docs/highlight-attribution.md](/home/jmartin/lst/docs/highlight-attribution.md).

## Notes

- Launching `lst` without file arguments creates a scratchpad in `~/.local/share/lst/`
- `Ctrl+N` creates another scratchpad tab
- Empty scratchpad tabs are deleted when you close them
- Clipboard integration uses `wl-copy` / `wl-paste` on Wayland and `xclip` on X11 when available
- The window title follows the active file unless `--title` is set
- Linux only

## License

MIT
