# Daily-Driver Contract

`lst` is optimized for quick file edits and persistent scratchpads. Standard
desktop editing is the primary interaction model. Vim remains available as an
explicit mode, but new general editing behavior must work without it.

## Dogfooding Current Source

Use the repository installer before evaluating daily-driver behavior:

```sh
./install.sh
~/.local/bin/lst --version
```

The version line includes the Cargo package version, full Git revision, and a
`-dirty` suffix when the source working tree has changes. The installer builds in
release mode and refuses to report success unless the installed identity exactly
matches the source build. This keeps window-manager launchers and terminal runs
on the same revision being reviewed.

## Defaults

- Launching without a path creates a timestamped scratchpad.
- Standard input mode is active unless `--vim` or the Vim setting is selected.
- Scratchpads autosave. Ordinary files require an explicit save.
- Closing a modified ordinary file asks whether to save, discard, or cancel.
  Quitting with several modified files opens one review with a decision and
  save result for every document.
- Closing a non-empty scratchpad copies it to CLIPBOARD and PRIMARY and archives
  it in recent history. Closing or quitting an ordinary file never replaces
  either selection. If no desktop clipboard owner can keep a scratchpad copy
  alive after process exit, the editor stays open once with a visible warning;
  a second `Ctrl+Q` explicitly quits anyway while the disk copy remains.
- Word wrap, absolute line numbers, cursor blink, and the light theme are enabled
  by default.

## Primary Surfaces

- `Ctrl+Shift+P` opens the searchable command palette.
- `Ctrl+,` opens settings.
- Settings opens with search focused; its font chooser and destructive reset
  confirmation are keyboard-operable.
- The application menu exposes the common file, navigation, and preference
  commands without requiring shortcut knowledge.
- `Ctrl+F` always opens or refocuses Find. Its disclosure arrow expands a
  second, aligned Replace row; `Ctrl+H` opens that expanded form directly.
  The visible controls cover previous/next, replace-one/all, case, whole-word,
  regex, and selection scope.
- `Ctrl+P` opens compact recent-file quick open; `Ctrl+R` opens the full recent
  view. Both distinguish regular files and scratchpads. The pinned all-tabs
  button provides keyboard access to every tab even when the strip overflows.
- The status bar exposes the active language and its explicit override menu.

External changes reload clean files in place. A dirty file gets a non-modal,
per-tab banner with Reload, Keep Mine, Save As, and version-scoped Dismiss
actions. A deleted backing file remains an explicit save-or-discard state and
is never silently treated as clean.

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
theme = "light"
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
