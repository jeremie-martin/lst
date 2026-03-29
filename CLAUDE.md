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
* **State**: `App { tabs, active, scratchpad_dir, needs_autosave, shift_held, goto_line, ... }` — each Tab holds a `text_editor::Content` and `is_scratchpad` flag
* **Messages**: `Edit`, `TabSelect`, `TabClose`, `New`, `Open`, `Save`, `SaveAs`, `AutosaveTick`, `Quit`, plus line ops (`DeleteLine`, `MoveLineUp/Down`, `DuplicateLine`), tab cycling (`NextTab`, `PrevTab`), go-to-line (`GotoLineOpen/Close/Changed/Submit`), and `ModifiersChanged`
* **Keyboard shortcuts**: two layers — `key_binding` closure on the text_editor widget (editor-focused shortcuts) and `iced::event::listen_with` subscription (global shortcuts, modifier tracking, Escape). Escape is handled in the subscription before the status check so it always closes overlays in a single press.
* **Scratchpad mode**: new tabs create timestamped `.md` files in `~/.local/share/lst/` (override with `--scratchpad-dir`). Ctrl+Shift+S to Save As (changes path).
* **Autosave**: saves all modified tabs on the next 500ms tick after any edit.
* **Line operations**: `DeleteLine`, `MoveLineUp/Down`, `DuplicateLine` use a text-rebuild pattern via `rebuild_content()` (split by `'\n'`, manipulate, rejoin, replace Content). Same pattern used by `ReplaceAll`.
* **Shift+Click**: iced's `Action::Click` doesn't carry modifier state, so `shift_held` is tracked via `ModifiersChanged` events in the subscription. Shift+Click is converted to `Action::Drag` in the `Message::Edit` handler to extend selection.
* **Go to line**: Ctrl+G opens a small overlay bar (same style as find bar). Input parsed as 1-based line number on submit.
* **Syntax highlighting**: unified `LstHighlighter` via syntect (~170 languages). Markdown files use hand-rolled block/inline highlighting with syntect for fenced code block interiors. Non-MD files get full-file syntect highlighting. Catppuccin Mocha `.tmTheme` embedded at compile time.
