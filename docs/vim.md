# Vim mode

Vim is an optional input mode implemented by `lst-editor`; it is not an
embedded Vim or a compatibility claim for the full Vim language. Enable it in
Settings or start one launch with `lst --vim`. Vim input currently starts in
Insert mode; press `Escape` to enter Normal mode. Search, replacement, and other
text fields always use standard field editing while focused; their keys do not
execute document Vim commands. Cancelling an incremental `/` or `?` search with
Escape or the find close button restores its starting position, unless the
cursor has since moved, the text changed, or another tab became active; Enter
accepts the preview, or a match chosen with a find step such as Shift+Enter.

## Supported surface

The supported core includes:

- Insert, Normal, characterwise Visual, and Visual Line modes
- counts and pending-command display
- `h j k l`, arrows, `0 ^ $`, `w W e E b B`, `gg`, `G`, `%`, `H M L`,
  `f F t T`, `;`, `,`, page keys, and `Ctrl+D/U/F/B`
- `d`, `c`, and `y` with motions, text objects, and multiplied counts;
  `dd`, `cc`, and `yy`
- `i a I A`, `o O`, `x X`, `s`, `D C S`, `J`, `r`, `p P`, `u`, and
  `>> <<`; redo is `Super+R`, because `Ctrl+R` opens the recent-files view
- Visual delete, change, yank, case conversion, paste, end swapping, and
  indentation
- inner/around word, big-word, paragraph, parentheses, brackets, braces,
  angle-bracket, and quoted-string text objects
- `/`, `?`, `n`, `N`, `*`, and `#` search through the shared find model
- one unnamed characterwise or linewise register
- `zz`, `zt`, `zb`, `g;`, and `gi`
- grapheme-aware movement and editing on the supported paths

The precise behavior is covered by
`crates/lst-editor/tests/vim_behavior.rs`, the checked-in Neovim oracle fixture,
and representative flows in `apps/lst-gpui/tests/real_x11_vim.rs`. Those tests
are the executable specification when this summary is incomplete.

## Explicitly unsupported

The model treats representative unsupported commands as Normal-mode no-ops and
clears pending input. The current contract excludes:

- dot repeat
- macros and `q` / `@`
- named registers and the `"` prefix
- marks and a Vim jump list
- Ex commands and `:`
- Replace mode
- Visual Block mode
- `g~`, `gu`, and `gU` Normal-mode operators
- surround commands
- full Vim option and regular-expression compatibility

Vim uses one selection. Enabling it retains each tab's primary selection and
removes secondary selections. Cursor-addition commands and gestures are unavailable
with a visible explanation. Undo/redo and externally supplied selection sets also
respect this limit. Standard mode retains its configured multi-cursor limit.

Ctrl+D/U initially move half a viewport. An explicit count becomes their shared
line distance, capped at the viewport height, until its height changes. Resizing
resets the distance to half the new height; changing tabs does not. Ctrl+F/B counts
remain page counts.

The reference profile uses `nostartofline`: linewise navigation, deletion and
indentation preserve the intended column, including tab expansion. First-nonblank
motions recognize spaces and tabs, not Unicode whitespace. Character-find history
survives edits, mode transitions and tab switches. Explicit `/` and `?` searches
start case-sensitive, literal and without a whole-word restriction, regardless of
previous Find-panel or `*`/`#` settings. Find-panel controls remain available.

## Implementation

`crates/lst-editor/src/vim.rs` contains the public key, mode, register, and
visible-state vocabulary. `vim_engine.rs` parses supported pending sequences
and executes them directly against `EditorModel`. There is no external engine,
fallback implementation, or separate command-translation layer.

Keep operator semantics explicit: linewise versus characterwise, inclusive
versus exclusive, failed-motion no-ops, register updates, and end-of-line
adjustment are part of the command being implemented. Reuse editor transactions
and history boundaries rather than adding a Vim-only mutation path.

## Tests and oracle maintenance

Run the fast Vim lanes with:

```sh
cargo test -p lst-editor --test vim_behavior
cargo test -p lst-editor --test vim_oracle
cargo test -p lst-editor --features internal-invariants
```

The oracle test replays
`crates/lst-editor/tests/fixtures/vim_oracle.json`. Its metadata records the
Neovim version, active option profile, and editor indent policy used to create
it.

The oracle owns supported Normal and Visual parity: add those cases to
`build_cases()` in the generator, not to `vim_behavior.rs`. The generator converts
Neovim's UTF-8 byte columns to the model's Unicode scalar columns, including visual
selection endpoints. `vim_behavior.rs` keeps intentional editor policy: grapheme
editing, logical line terminators beyond LF, unsupported-command no-ops, pending
display, search focus, `<cmd-r>` redo, viewport-dependent motions, and `g;` and `gi`.
The editor's shared character-cell layout is not a claim of Neovim wide-character
screen-column parity. A command that fails in Neovim, whether with an error such as
E35 or as a failed motion such as `fz`, discards the rest of that case's keys,
so end a case at such a command.

Regenerate the fixture only for an intentional parity change:

```sh
python3 scripts/generate_vim_oracle_fixtures.py --nvim /path/to/nvim-v0.12.6
cargo test -p lst-editor --test vim_oracle
```

The pinned reference is Neovim **0.12.6**, the stable release checked on
2026-10-25. The generator uses `-u NONE --noplugin -i NONE -n`, non-remapped input,
an explicit option profile and two-column indentation. It resets registers,
search and character-find state between cases. Personal configuration and persisted
state are excluded. Review metadata and case diffs when deliberately updating the
pin; the replay test requires the recorded stable version.

All 912 previous cases regenerate unchanged under this profile; the expanded
fixture contains 1,109 cases. Repeating generation with a hostile user configuration
and reversing case order produce identical expectations. The same corpus was also
run on the newest available nightly, `v0.13.0-dev-1946+g659164699f`. It differs in four
undo cursor positions (append, yank/paste, visual paste and inner-word yank/paste),
with identical text and registers. The checked-in contract keeps stable behavior.

Intentional differences also include the shared Find engine's literal/regex syntax,
editor viewport movement rather than Neovim's entire window-scrolling algorithm,
and the unsupported commands listed above.

Use the `vim_model` Criterion benchmark for changes to motion, range, parser,
register, or whole-document paths. See [Performance](performance.md).
