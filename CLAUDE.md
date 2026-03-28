# CLAUDE.md

## Quick Commands

* Build: `cargo build --release`
* Run: `cargo run --release` or `cargo run --release -- file.txt`
* Lint: `cargo clippy`
* Format: `cargo fmt`
* Check: `cargo check`

## Code Standards

* Idiomatic Rust — no premature abstractions, no speculative generality.
* Palette-based theming via `theme.extended_palette()` — never hardcode colors.
* Use `Theme::CatppuccinMocha` as the default theme (built into iced).
* Single `main.rs` until it grows past ~600 lines, then split into modules.
* All file I/O through async helpers + `Task::perform`.

## Architecture

* **Toolkit**: iced 0.14 (retained-mode, GPU-accelerated via wgpu)
* **Font**: JetBrains Mono (loaded from system path)
* **State**: `App { tabs: Vec<Tab>, active: usize }` — each Tab holds a `text_editor::Content`
* **Messages**: `Edit`, `TabSelect`, `TabClose`, `New`, `Open`, `Save`, `Quit`
* **Keyboard shortcuts**: handled via `iced::event::listen_with` subscription
