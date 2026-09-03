# Vim mode

Vim is an optional input mode implemented by `lst-editor`; it is not an
embedded Vim or a compatibility claim for the full Vim language. Enable it in
Settings or start one launch with `lst --vim`. Vim input currently starts in
Insert mode; press `Escape` to enter Normal mode.

## Supported surface

The supported core includes:

- Insert, Normal, characterwise Visual, and Visual Line modes
- counts and pending-command display
- `h j k l`, arrows, `0 ^ $`, `w W e E b B`, `gg`, `G`, `%`, `H M L`,
  `f F t T`, `;`, `,`, page keys, and `Ctrl+D/U/F/B`
- `d`, `c`, and `y` with motions, text objects, and multiplied counts;
  `dd`, `cc`, and `yy`
- `i a I A`, `o O`, `x X`, `s`, `D C S`, `J`, `r`, `p P`, `u`, and
  `>> <<`
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

Vim behavior with multiple active cursors is also undefined. Establish an
explicit user contract before extending it; do not preserve incidental current
results.

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

Regenerate the fixture only for an intentional parity change:

```sh
python3 scripts/generate_vim_oracle_fixtures.py
cargo test -p lst-editor --test vim_oracle
```

The generator runs the locally installed `nvim` with the user's configuration,
then overrides the buffer-local indentation used by the cases. Local mappings
or options can therefore change the fixture. Review both metadata and case
diffs; do not accept regenerated output solely because the replay test passes.

Use the `vim_model` Criterion benchmark for changes to motion, range, parser,
register, or whole-document paths. See [Performance](performance.md).
