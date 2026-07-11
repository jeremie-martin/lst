# Daily-Driver Contract

`lst` is optimized for quick file edits and persistent scratchpads. Standard
desktop editing is the primary interaction model. Vim remains available as an
explicit mode, but new general editing behavior must work without it.

## Defaults

- Launching without a path creates a timestamped scratchpad.
- Standard input mode is active unless `--vim` or the Vim setting is selected.
- Scratchpads autosave. Ordinary files require an explicit save.
- Closing a modified ordinary file asks whether to save, discard, or cancel.
- Word wrap, absolute line numbers, cursor blink, and system theme are enabled
  by default.

## Primary Surfaces

- `Ctrl+Shift+P` opens the searchable command palette.
- `Ctrl+,` opens settings.
- The application menu exposes the common file, navigation, and preference
  commands without requiring shortcut knowledge.
- `Ctrl+F` and `Ctrl+H` open find and replace. The visible controls cover
  previous/next, replace-one/all, case, whole-word, regex, and selection scope.
- The status bar exposes the active language and its explicit override menu.

## Settings

Settings live at `$XDG_CONFIG_HOME/lst/config.toml`, or
`~/.config/lst/config.toml` when `XDG_CONFIG_HOME` is unset. Known fields are
updated without discarding unrelated keys or comments. Valid external changes
are reloaded while the app is running; malformed files are reported and never
overwritten automatically.

```toml
version = 1

[editor]
input_mode = "standard"
word_wrap = true
line_numbers = "absolute"
cursor_blink = true
font_family = "TX-02"
font_size = 13

[appearance]
theme = "system"
zoom_level = 0

[files]
autosave = "scratchpads"
trim_trailing_whitespace = false
ensure_final_newline = false
# scratchpad_directory = "/home/me/notes"

[keybindings]
"workbench.command_palette" = ["ctrl-shift-p"]
"file.save" = ["ctrl-s"]
```

An entry in `[keybindings]` replaces that command's defaults. Unknown command
IDs are preserved but ignored. Duplicate configured chords are marked as
conflicts in the settings table.

## Standard Key Policy

The Linux/Windows-style bindings are the compatibility baseline:

- `Alt+Up/Down` moves lines.
- `Shift+Alt+Up/Down` duplicates lines.
- `Ctrl+Alt+Up/Down` adds cursors.
- `Ctrl+Up/Down` scrolls without moving the caret.
- `Escape` first collapses selections, then removes secondary cursors.

Platform-command equivalents remain registered where GPUI exposes them.

## Acceptance

User-visible convenience changes belong in the real-display lane. The focused
contract is `apps/lst-gpui/tests/real_x11_daily_driver.rs`; the full blocking
X11 profile remains the submission gate. Vim-only tests launch with `--vim` so
they cannot accidentally redefine the default editing experience.
