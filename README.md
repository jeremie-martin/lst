# lst

**Lucy's Simple Text editor** — a fast, minimal, good-looking text editor for Linux.

## Why

Every existing simple text editor is either ugly (gedit, mousepad), slow (VS Code, Atom), or both. lst aims to be the text editor you actually *want* to use for quick edits: beautiful dark theme, instant startup, zero typing latency, and nothing you don't need.

## Features

- Catppuccin Mocha dark theme
- Multiple tabs with Ctrl+N / Ctrl+W
- Open files from CLI (`lst file.txt`) or Ctrl+O dialog
- Save with Ctrl+S (Save As for new files)
- Line numbers
- JetBrains Mono font
- GPU-accelerated rendering (wgpu)

### Planned

- Syntax highlighting (Markdown first, then common languages)
- Find & Replace (Ctrl+F)
- Light/dark theme switching
- Recent files

## Build & Run

Requires Rust 1.75+ and JetBrains Mono installed.

```bash
cargo build --release
./target/release/lst                    # empty editor
./target/release/lst README.md          # open a file
./target/release/lst *.rs               # open multiple files
```

## Design

- **Toolkit**: [iced](https://iced.rs) 0.14 — retained-mode GUI, GPU-rendered via wgpu
- **Font rendering**: cosmic-text (same engine as System76's COSMIC desktop)
- **Philosophy**: do one thing well. No plugins, no config files, no 200-line settings menu. Just a clean place to edit text.

## License

MIT
