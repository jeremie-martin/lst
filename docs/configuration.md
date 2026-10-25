# Configuration

Open Settings with `Ctrl+,`. Use the category sidebar to browse, or search across
all settings and keyboard shortcuts. `Tab` selects a result, arrow keys adjust it,
and `Esc` returns to the editor. Changes are applied immediately and written to:

- `$XDG_CONFIG_HOME/lst/config.toml` when `XDG_CONFIG_HOME` is an absolute path
- `$HOME/.config/lst/config.toml` otherwise, if HOME is absolute

Empty or relative configuration roots are ignored. If neither root is usable,
Settings reports that preferences cannot be persisted.

The file is versioned and reloads while `lst` is running. A valid partial file
uses defaults for omitted fields. When the application writes settings, it
patches only changed values, preserving unrelated keys, comments, keybinding
examples, and the spelling of unchanged values. Omitted defaults stay omitted.
It reports malformed files instead of overwriting them; an invalid live update
leaves the previous settings active.

A failed save leaves your changes active and displays the error in Settings. A
failed reset leaves the existing preferences active; defaults take effect only
after their configuration has been saved successfully. A later successful save
or valid external reload clears the error.

Writes replace the file atomically and preserve its permissions. An existing
symlink remains a symlink: its target is updated. Dangling links, non-file targets,
and read-only files produce an error. Failed writes clean up their temporary file.

## Schema

This is the complete version 1 schema with defaults:

```toml
version = 1

[editor]
input_mode = "standard"                   # standard | vim
word_wrap = true
line_numbers = "absolute"                 # absolute | relative | hybrid
cursor_blink = true
smooth_cursor = false                     # smooth slide, no stretching
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

[voice]
language = "en"                           # auto or language code
# directory = "/home/me/audio-notes"        # defaults to ~/audio-notes

[keybindings]
```

Custom `scratchpad_directory` and voice `directory` values must be absolute paths
or start with `~/`, which expands using an absolute HOME directory. Other shell
or environment-variable expansion is unsupported. Invalid paths are reported in
Settings and never resolved relative to the launch working directory.

`autosave = "scratchpads"` never autosaves ordinary files. `autosave = "all"`
also autosaves ordinary files. Scratchpads always use their timestamped backing
file.

Explicit Save and Save As apply the two optional text policies. Trimming removes
trailing spaces and tabs while preserving existing line-ending bytes. Ensuring
a final newline uses the first existing line-ending style, or LF when there is
none; empty documents stay empty. Successful normalization is one undoable edit
and keeps selections attached to the remaining text. Autosave preserves the
document exactly and does not apply these policies.

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

Prompt improvement runs `prompt-add` from `PATH`, inheriting the editor's
`DEEPSEEK_API_KEY`. Install and configure that tool separately; it owns model
selection and local rewrite history. `DEEPSEEK_MODEL` is no longer used by lst.
The command is named **Improve Prompt** and retains `tools.cleanup_text`
for existing keybindings. It has no default keyboard shortcut.

## Voice notes

Voice notes use the default microphone and ElevenLabs Scribe v2 batch
transcription. Set `ELEVENLABS_API_KEY` in the environment that launches LST;
leading and trailing whitespace is ignored. LST does not load another project's
`.env` file or store the key in settings.
`[voice].language` accepts `auto` or a lowercase two/three-letter language code
(default `en`). `[voice].directory` overrides `~/audio-notes`. These two options
are edited in TOML and apply to the next recording session.

Voice commands have no default shortcuts. For example:

```toml
[keybindings]
"voice.new_note" = ["ctrl-shift-r"]
"voice.pause_resume" = ["f7"]
"voice.finish" = ["f8"]
"voice.cancel" = ["shift-f8"]
"voice.retry" = ["f9"]
```

**Cancel Pending Dictation** stops the microphone and discards uninserted audio
and results. Text already inserted stays in the document and remains undoable.
An upload already in progress may finish at the service, but its result cannot
enter this or a later session. Failed uploads keep their temporary WAV files
until retry succeeds or the session is cancelled. Retention lasts for the
running session, not across application crashes. Each recording segment is
limited to 15 minutes; resume to record another. At most 16 segments can wait
for transcription. Requests have a two-minute timeout and are retried only on
request, because another request can incur another service charge.
