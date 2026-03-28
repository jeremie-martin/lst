# lst

**Lucy's Simple Text editor** — a fast, minimal, good-looking text editor for Linux.

## Why

Every existing simple text editor is either ugly (gedit, mousepad), slow (VS Code, Atom), or both. lst aims to be the text editor you actually *want* to use for quick edits: beautiful dark theme, instant startup, zero typing latency, and nothing you don't need.

## Features

- Catppuccin Mocha dark theme
- Multiple tabs with Ctrl+N / Ctrl+W
- Open files from CLI (`lst file.txt`) or Ctrl+O dialog
- Save with Ctrl+S, Save As with Ctrl+Shift+S
- Scratchpad mode: new tabs are timestamped `.md` files in `~/.local/share/lst/`
- Autosave: files save automatically ~2s after you stop typing
- Find & Replace (Ctrl+F / Ctrl+H)
- Markdown syntax highlighting
- Word wrap toggle (Alt+Z)
- Undo/redo with edit grouping (Ctrl+Z / Ctrl+Shift+Z)
- Auto-indent on Enter
- Line numbers with gutter click-to-select
- Tab reorder (Shift+PageUp/PageDown)
- JetBrains Mono font
- GPU-accelerated rendering (wgpu)

### Planned

- Light/dark theme switching
- Recent files / session restore

## Install

Requires Rust 1.75+ and JetBrains Mono installed at `/usr/share/fonts/jetbrains-mono/JetBrainsMono[wght].ttf`.

```bash
./install.sh
~/.local/bin/lst
```

`install.sh` uses `cargo install --path . --locked --root ~/.local` by default. Set `LST_PREFIX=/some/prefix` if you want a different install location.

## Build & Run

Requires Rust 1.75+ and JetBrains Mono installed.

```bash
cargo build --release
./target/release/lst                    # new scratchpad file
./target/release/lst README.md          # open a file
./target/release/lst *.rs               # open multiple files
./target/release/lst --scratchpad-dir ~/notes  # custom scratchpad directory
```

## LWM Scratchpad

`lst` supports a fixed window title for scratchpad setups:

```bash
lst --title lst-scratchpad
```

Example `~/.config/lwm/config.toml` entries:

```toml
[[binds]]
key = "super+t"
toggle_scratchpad = "lst"

[[scratchpads]]
name = "lst"
spawn = { argv = ["/home/you/.local/bin/lst", "--title", "lst-scratchpad"] }
match = { title = "^lst-scratchpad$" }
size = { width = 0.8, height = 0.7 }
```

## Design

- **Toolkit**: [iced](https://iced.rs) 0.14 — retained-mode GUI, GPU-rendered via wgpu
- **Font rendering**: cosmic-text (same engine as System76's COSMIC desktop)
- **Philosophy**: do one thing well. No plugins, no config files, no 200-line settings menu. Just a clean place to edit text.

## License

MIT
