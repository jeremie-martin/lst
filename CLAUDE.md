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
* Modules: `main.rs` (~1300 lines), `tab.rs`, `find.rs`, `highlight.rs`, `style.rs`. Split further if main.rs grows significantly.
* All file I/O through async helpers + `Task::perform`.

## Architecture

* **Toolkit**: iced 0.14 (retained-mode, GPU-accelerated via wgpu)
* **Font**: TX-02 preferred, JetBrains Mono fallback, then system monospace (loaded via fontdb)
* **State**: `App { tabs, active, scratchpad_dir, needs_autosave, shift_held, goto_line, ... }` — each Tab holds a `text_editor::Content` and `is_scratchpad` flag
* **Messages**: editing (`Edit`, `Undo`, `Redo`, `AutoIndent`), tabs (`TabSelect`, `TabClose`, `New`, `Open`, `Save`, `NextTab`, `PrevTab`), line ops (`DeleteLine`, `MoveLineUp/Down`, `DuplicateLine`), find (`FindOpen`, `FindNext`, `ReplaceOne`, `ReplaceAll`, etc.), go-to-line (`GotoLineOpen/Close/Changed/Submit`), gutter (`GutterMove`, `GutterClick`), and `ModifiersChanged` for Shift+Click tracking
* **Keyboard shortcuts**: two layers — `key_binding` closure on the text_editor widget (editor-focused shortcuts) and `iced::event::listen_with` subscription (global shortcuts, modifier tracking, Escape, middle-click). Escape is handled in the subscription before the status check so it always closes overlays in a single press.
* **Scratchpad mode**: new tabs create timestamped `.md` files in `~/.local/share/lst/` (override with `--scratchpad-dir`). Ctrl+Shift+S to Save As (changes path).
* **Find & Replace** (`find.rs`): Ctrl+F opens find bar, Ctrl+H opens with replace. Matches recomputed on every edit when visible. Navigation via FindNext/FindPrev with nearest-match seeking. Replace one or all.
* **Undo/Redo** (`tab.rs`): snapshot-based (full text + cursor position). Consecutive same-kind edits grouped; whitespace breaks insert groups. Max 100 snapshots per tab. Line ops and ReplaceAll push a single `EditKind::Other` snapshot.
* **Auto-indent**: Enter key copies leading whitespace from current line (handled via `AutoIndent` message in key_binding).
* **Word wrap**: Alt+Z toggles between `Wrapping::Word` and `Wrapping::None`.
* **Gutter**: click selects entire line (`GutterClick`); mouse position tracked via `GutterMove`.
* **Middle-click paste**: middle mouse button pastes from the primary selection (X11/Wayland) at the current cursor position. Handled in the subscription via `read_primary_selection()`, which calls `wl-paste --primary` or `xclip -selection primary -o`.
* **Clipboard helpers**: `is_wayland()` detects display server; `copy_to_clipboard()` writes to both selections; `read_primary_selection()` reads the primary selection. All use sync subprocess calls.
* **Tab close**: closing last tab exits the app via `exit_with_clipboard()`, which copies active tab content to both X11/Wayland clipboards.
* **Autosave**: saves all modified tabs on the next 500ms tick after any edit.
* **Line operations**: `DeleteLine`, `MoveLineUp/Down`, `DuplicateLine` use a text-rebuild pattern via `rebuild_content()` (split by `'\n'`, manipulate, rejoin, replace Content). Same pattern used by `ReplaceAll`.
* **Shift+Click**: iced's `Action::Click` doesn't carry modifier state, so `shift_held` is tracked via `ModifiersChanged` events in the subscription. Shift+Click is converted to `Action::Drag` in the `Message::Edit` handler to extend selection.
* **Go to line**: Ctrl+G opens a small overlay bar (same style as find bar). Input parsed as 1-based line number on submit.
* **Syntax highlighting**: unified `LstHighlighter` via syntect (~170 languages). Markdown files use hand-rolled block/inline highlighting with syntect for fenced code block interiors. Non-MD files get full-file syntect highlighting. Catppuccin Mocha `.tmTheme` embedded at compile time.
