# lst

`lst` is a small Linux desktop text editor built with GPUI. It provides
standard desktop editing by default and an optional, deliberately bounded Vim
mode.

The editor supports multiple tabs and cursors, find and replace, soft wrap,
line bookmarks, configurable keybindings, and tree-sitter highlighting for
Rust, Python, JavaScript/JSX, TypeScript/TSX, JSON, TOML, YAML, Markdown, HTML,
and CSS. Scratchpads autosave; ordinary files use explicit save and conflict
handling.

## Run from source

A Rust toolchain and a graphical session are required.

```sh
cargo build --release -p lst-gpui
./target/release/lst README.md
```

Run `./target/release/lst --help` for the authoritative command-line options.
Common forms are:

```sh
./target/release/lst                         # new scratchpad
./target/release/lst file.rs notes.md        # open files in tabs
./target/release/lst --vim file.rs           # start in Vim mode
./target/release/lst --scratchpad-dir notes  # override scratchpad storage
./target/release/lst --title lst-scratchpad  # override the window title
```

With no file arguments, `lst` creates a timestamped Markdown scratchpad in
`~/.local/share/lst/`. Scratchpads are saved automatically. Closing a non-empty
scratchpad also copies its contents to the desktop clipboard and primary
selection; if the clipboard cannot remain available after exit, `lst` warns
before quitting. Modified ordinary files always require an explicit save or
discard decision.

The main entry points are:

- `Ctrl+Shift+P`: searchable command palette
- `Ctrl+,`: settings
- `Ctrl+P`: quick open from recent files
- `Ctrl+R`: full recent-files view
- `Ctrl+F` / `Ctrl+H`: find / replace
- `Ctrl+G`: go to line or `line:column`

The application menu and command palette expose the rest of the active command
set and its current shortcuts.

## Install

The installer requires Cargo, Git, fontconfig, and the `TX-02` font. It builds
from the locked dependency set, installs `lst`, and verifies that the installed
build identity matches the checkout.

```sh
./install.sh
~/.local/bin/lst --version
```

The default prefix is `~/.local`. Set `LST_PREFIX` to install elsewhere:

```sh
LST_PREFIX=/opt/lst ./install.sh
```

## Configuration

Settings are available in the application and persist to
`$XDG_CONFIG_HOME/lst/config.toml`, or `~/.config/lst/config.toml` when
`XDG_CONFIG_HOME` is unset. The file reloads while the app is running.

See [Configuration](docs/configuration.md) for the schema, keybinding format,
and precedence rules.

## Agent prompt polishing

**Polish Agent Prompt** runs the installed `prompt-add` executable on the active
selection. Without a selection, it asks before submitting the whole document.
The result replaces the text as one undo step; failed requests or results for a
changed buffer leave the document untouched.

Install `prompt-add` separately and make it available on `PATH`. It inherits
`DEEPSEEK_API_KEY` from the editor's environment and owns the model and editorial
instructions. It sends the submitted text to DeepSeek and keeps its normal local
history, labeled `lst`. The integration does not access the clipboard.

The command retains the `tools.cleanup_text` ID for existing custom shortcuts.

## Development

The root manifest is a Cargo workspace. It is not an application crate.

- `apps/lst-gpui`: desktop application, rendering, input, and runtime effects
- `crates/lst-editor`: framework-neutral editor model
- `crates/lst-x11-harness`: real-X11 behavior-test driver

Contributor rules and the normal verification commands are in
[AGENTS.md](AGENTS.md). The remaining documentation has one subject per file:

- [Architecture](docs/architecture.md): ownership, data flow, and design invariants
- [Testing](docs/testing.md): test placement and the real-X11 harness
- [Performance](docs/performance.md): benchmark selection and comparison workflow
- [Vim mode](docs/vim.md): supported scope and oracle maintenance
- [Changelog](CHANGELOG.md): release history

## License

GPL-3.0-or-later
