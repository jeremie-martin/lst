# Configuration

Open Settings with `Ctrl+,`. Changes are applied immediately and written to:

- `$XDG_CONFIG_HOME/lst/config.toml` when `XDG_CONFIG_HOME` is set
- `~/.config/lst/config.toml` otherwise

The file is versioned and reloads while `lst` is running. A valid partial file
uses defaults for omitted fields. When the application writes settings, it
preserves unrelated keys and comments. It reports malformed files instead of
overwriting them; a malformed live update leaves the previous settings active.

## Schema

This is the complete version 1 schema with defaults:

```toml
version = 1

[editor]
input_mode = "standard"                   # standard | vim
word_wrap = true
line_numbers = "absolute"                 # absolute | relative | hybrid
cursor_blink = true
font_family = "TX-02"
font_size = 13                             # clamped to 8..40
match_brackets = "always"                 # never | near | always
bracket_pair_colorization = true
bracket_pair_guides = "off"               # off | active | all
bracket_pair_horizontal_guides = "off"    # off | active | all
indent_guides = false
highlight_active_indent_guide = false
render_whitespace = "selection"           # none | boundary | selection | trailing | all
render_control_characters = true
rulers = []                                # up to 16 entries in 1..1000; sorted and deduplicated
smart_select_subwords = true
smart_select_include_whitespace = true
multi_cursor_limit = 10000                 # clamped to 1..10000

[appearance]
theme = "light"                            # system | dark | light
zoom_level = 0                             # clamped to -4..8

[files]
autosave = "scratchpads"                   # scratchpads | all
trim_trailing_whitespace = false
ensure_final_newline = false
# scratchpad_directory = "/home/me/notes"

[keybindings]
```

`autosave = "scratchpads"` never autosaves ordinary files. `autosave = "all"`
also autosaves ordinary files. Scratchpads always use their timestamped backing
file.

The `--vim` and `--no-vim` launch options choose the initial input mode instead
of `[editor].input_mode`. `--scratchpad-dir` chooses the scratchpad directory
for that launch. Run `lst --help` for the full launch interface.

## Keybindings

Each key in `[keybindings]` is a command ID. Its array replaces every default
binding for that command; an empty array leaves the command unbound.

```toml
[keybindings]
"workbench.command_palette" = ["ctrl-shift-p"]
"file.save" = ["ctrl-s"]
"selection.column_down" = ["ctrl-alt-shift-down"]
"selection.add_next_occurrence" = ["ctrl-d", "ctrl-k ctrl-d"]
```

Keystrokes use GPUI names such as `ctrl-s`, `shift-alt-down`, and `f3`.
Whitespace separates a multi-step chord. Invalid keystrokes and unknown command
IDs are preserved in the file but ignored by the application. Settings marks a
keystroke as a conflict when more than one configured command owns it.

The Settings search includes command titles, IDs, categories, and active
shortcuts. The authoritative command-ID and default-binding tables are
`command_id` and `BINDINGS` in
[`apps/lst-gpui/src/workspace_action.rs`](../apps/lst-gpui/src/workspace_action.rs).
The Settings UI displays bindings but does not capture or edit them directly;
edit the TOML file to change them.

## Other runtime configuration

Prompt polishing runs `prompt-add` from `PATH`, inheriting the editor's
`DEEPSEEK_API_KEY`. Install and configure that tool separately; it owns model
selection and local rewrite history. `DEEPSEEK_MODEL` is no longer used by lst.
The command is named **Polish Agent Prompt** and retains `tools.cleanup_text`
for existing keybindings. It has no default keyboard shortcut.
