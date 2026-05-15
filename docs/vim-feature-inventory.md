# Vim Feature Inventory

This document records the Vim behavior that existed before commit `5e3abbe` rewrote `lst-editor`. It is the restoration checklist for keeping the compact editor core while bringing daily Vim workflows back under accepted behavior coverage.

## Implementation Strategy

- Treat every item in this file as product behavior once restored.
- Prefer X11 tests for user-visible behavior; keep source-only tests only for pure parser/motion invariants that cannot naturally be driven through the GPUI app.
- Vim has an additional fast model-level black-box parity lane in `crates/lst-editor/tests/vim_behavior.rs`. It drives the public `EditorModel` input surface and asserts observable text, cursor, mode, find, register, and reveal outcomes. This complements, rather than replaces, X11 because exhaustive Vim command coverage would be too slow and fragile through a real display.
- `crates/lst-editor/tests/vim_oracle.rs` runs a checked-in generated corpus from `crates/lst-editor/tests/fixtures/vim_oracle.json`. Regenerate it with `python3 scripts/generate_vim_oracle_fixtures.py`.
- Local Neovim 0.9.5 loaded with the user config is the oracle for generated stock Vim behavior. If this inventory disagrees with Neovim for a supported generated case, Neovim takes precedence and this document should be corrected.
- Surround and enhanced text-object behavior may be checked against local Neovim plugins when present, but only for lst's existing supported surface. The generator detects `ys`/`ds`/`cs`; when no plugin oracle is available, use the restored lst/vim-surround-style behavior recorded here.
- `modalkit 0.0.25` provides a Vim keybinding machine that emits generic modal editing actions. It is useful as the long-term parser/binding source, but it is not a direct replacement for `EditorModel` execution because lst already owns selections, transactions, find state, wrapping, viewport reveal, registers, and GPUI effects. A safe adapter must translate modalkit actions into lst commands without bypassing those contracts.
- Surround commands are outside modalkit's default Vim surface and remain lst-specific.

## Restoration Status

- The parity baseline has been restored by bringing the previous lst Vim state machine, edit helpers, and `EditorModel` command executor back into the GPUI editor path.
- `modalkit` is restored as a workspace dependency and remains the planned parser/keybinding source once an adapter can translate its `editor-types` actions onto lst's document and transaction contracts with full X11 parity.
- New accepted X11 coverage now exercises representative restored surfaces: text-object change (`ciw`), open-line/join/replace, linewise yank/paste, surround change/delete, visual text-object case transforms, and `*`/`n` word search.
- The tables below record the feature status immediately after compact rewrite commit `5e3abbe`, before the parity restoration.

## Model-Level Coverage Map

The fast black-box suite in `crates/lst-editor/tests/vim_behavior.rs` is the exhaustive parity lane for the restored Vim surface:

| Inventory area | Model-level coverage |
| --- | --- |
| Modes and state | `modes_state_pending_and_escape_follow_vim_contracts`, `vertical_motions_preserve_preferred_column`, `undo_redo_and_last_edit_jump_track_vim_edits` |
| Motions | `normal_motions_cover_words_lines_char_search_and_brackets`, `named_and_page_motions_cover_keyboard_boundary_paths`, `word_and_big_word_motions_cover_counts_punctuation_empty_lines_and_unicode`, `viewport_commands_emit_reveal_effects_and_move_to_visible_rows` |
| Operators | `operators_cover_motion_ranges_text_objects_counts_and_lines`, `operators_cover_linewise_inclusive_exclusive_and_register_edges` |
| Normal edits | `normal_edits_cover_insert_positions_substitute_join_replace_paste_and_indent`, `normal_edits_cover_counts_boundaries_empty_lines_and_noops` |
| Visual mode | `visual_mode_covers_charwise_linewise_text_objects_case_and_indentation`, `visual_mode_covers_counts_reverse_selection_search_repeat_and_viewport`, `viewport_page_motions_preserve_visual_state` |
| Search | `search_commands_cover_word_search_find_panel_and_visual_stepping`, `search_commands_cover_wrap_empty_words_and_find_query_editing` |
| Text objects | `text_objects_cover_words_paragraphs_pairs_quotes_counts_and_escapes` |
| Surround | `surround_commands_cover_motion_text_object_and_delimiter_variants`, `surround_commands_cover_all_delimiters_aliases_motion_counts_and_noops` |
| Registers | `registers_preserve_charwise_and_linewise_paste_placement`, `paste_placement_covers_charwise_linewise_before_after_and_empty_registers` |
| X11 bridge | `x11_vim_smoke_specs_run_through_the_editor_model` mirrors the current real-display Vim acceptance cases at model level. |

The generated oracle corpus currently adds 696 Neovim-derived cases across motions, operators, text objects, normal edits, registers, visual operators, and search. It records the nvim version and options used to produce the fixture. Text-object generation covers stock aliases such as `cib`, `ci]`, `ciB`, and `ci>` across multiple cursor positions, count composition, empty-pair changes, no-op boundaries, failed motions, search query state, final visual selections, and exact unnamed-register kind/text where the command should touch the register. Indent commands are generated with lst's Markdown editor indent policy so the oracle checks Vim command semantics rather than the scratch nvim buffer's default `shiftwidth`.

## Modes And State

| Feature | Old behavior | Status after compact rewrite |
| --- | --- | --- |
| Insert mode | Text is handled by the app; Escape enters Normal. | Present |
| Normal mode | Vim keys drive motions, operators, search, paste, line edits, and mode changes. | Partial |
| Visual charwise mode | `v` starts/toggles charwise selection; operators apply to selection. | Partial |
| Visual line mode | `V` starts/toggles linewise selection; operators apply to full touched lines. | Partial |
| Escape behavior | Insert Escape moves one grapheme left before Normal; Visual Escape collapses to cursor. | Present |
| Tab switch behavior | Visual state is cancelled when switching tabs. | Present |
| Pending display | Counts, pending operators, partial commands, and surround state are displayed. | Partial |
| Preferred column | Vertical Vim motions preserve preferred column until a non-vertical motion clears it. | Missing |
| Last edit position | `g;` jumps to last edit; `gi` jumps to last edit and enters Insert. | Missing |

## Motions

| Keys | Old behavior | Status after compact rewrite |
| --- | --- | --- |
| `h`, `l`, arrows | Grapheme-aware horizontal movement. | Partial |
| `j`, `k`, arrows | Vertical movement with preferred column. | Partial |
| `0`, Home | Start of line. | Present |
| `$`, End | End of line; count moves to end of later line. | Partial |
| `^` | First non-blank character. | Missing |
| `w`, `b`, `e` | Word forward/backward/end. | Missing |
| `W`, `B`, `E` | Big-word forward/backward/end. | Missing |
| `gg` | Document start; with count, target line. | Partial |
| `G` | Document end; with count, target line. | Missing |
| `%` | Matching bracket; with count, percentage through file. | Missing |
| `f{char}`, `t{char}` | Find/till next character on line. | Missing |
| `F{char}`, `T{char}` | Find/till previous character on line. | Missing |
| `;`, `,` | Repeat last char search forward/reverse. | Missing |
| `H`, `M`, `L` | Move to top/middle/bottom visible screen row. | Missing |
| Ctrl-d/u/f/b | Half-page down/up and page down/up. | Present |
| PageUp/PageDown | Page motion. | Present |

## Operators

| Keys | Old behavior | Status after compact rewrite |
| --- | --- | --- |
| `d{motion}` | Delete motion range into char/line register. | Missing |
| `c{motion}` | Change motion range into register and enter Insert. | Missing |
| `y{motion}` | Yank motion range into char/line register. | Missing |
| `dd`, `cc`, `yy` | Line delete/change/yank, with counts. | Partial |
| Counts | Operator count and motion count multiply, e.g. `2d3w`. | Missing |
| Linewise motion operators | `dj`, `dk`, `dG`, counted `$`, counted `%`, etc. operate linewise. | Missing |
| Inclusive/exclusive rules | Vim-style range adjustment for word, end, char-search, backward motions. | Missing |
| `cw`, `cW` | Behave like `ce`, `cE` when cursor is on non-whitespace. | Missing |

## Normal Mode Edits

| Keys | Old behavior | Status after compact rewrite |
| --- | --- | --- |
| `i`, `a`, `I`, `A` | Enter Insert at cursor/after cursor/first nonblank/EOL. | Present |
| `o`, `O` | Open line below/above, inherit indentation, enter Insert. | Missing |
| `x`, `X` | Delete character under/before cursor, with count. | Missing |
| `s` | Substitute character(s), enter Insert. | Missing |
| `D`, `C` | Delete/change to end of line. | Missing |
| `S` | Change full line(s), preserving indentation. | Missing |
| `J` | Join lines; count controls number of joined lines. | Missing |
| `r{char}` | Replace character(s) under cursor. | Missing |
| `p`, `P` | Paste charwise or linewise register after/before cursor. | Missing |
| `u` | Undo. | Present |
| platform `r` | Redo. | Present |
| `>>`, `<<` | Indent/outdent current line(s), with count. | Partial |

## Visual Mode

| Keys | Old behavior | Status after compact rewrite |
| --- | --- | --- |
| Motions | Visual selections extend with normal motions, counts, char search, and search repeat. | Partial |
| `d`, `x` | Delete selection; linewise in VisualLine. | Partial |
| `c`, `s` | Change selection and enter Insert. | Missing |
| `y` | Yank selection and return to Normal. | Partial |
| `>`, `<` | Indent/outdent selected lines and return to Normal. | Present |
| `v`, `V` | Toggle Visual/VisualLine modes. | Partial |
| `u`, `U` | Lowercase/uppercase selected range or selected lines. | Missing |
| `i{object}`, `a{object}` | Select text object in Visual mode. | Missing |
| `/`, `n`, `N` | Open find and step find while preserving/adjusting visual state. | Missing |
| `H`, `M`, `L` | Move visual head to visible screen top/middle/bottom. | Missing |
| `zz`, `zt`, `zb` | Reveal cursor center/top/bottom without editing. | Missing |

## Search

| Keys | Old behavior | Status after compact rewrite |
| --- | --- | --- |
| `/` | Open find panel. | Missing |
| `n`, `N` | Step through current find matches. | Missing |
| `*`, `#` | Search for word under cursor forward/backward. | Missing |
| Visual search stepping | `n`/`N` move visual head to match. | Missing |

## Text Objects

| Keys | Old behavior | Status after compact rewrite |
| --- | --- | --- |
| `iw`, `aw` | Inner/a word. | Partial only for `ysiw)` |
| `iW`, `aW` | Inner/a big-word. | Missing |
| `ip`, `ap` | Inner/a paragraph. | Missing |
| `i(`/`a(`, `i)`/`a)`, `ib`/`ab` | Parentheses object. | Missing |
| `i{`/`a{`, `i}`/`a}`, `iB`/`aB` | Braces object. | Missing |
| `i[`/`a[`, `i]`/`a]` | Brackets object. | Missing |
| `i<`/`a<`, `i>`/`a>` | Angle-bracket object. | Missing |
| `i"`/`a"`, `i'`/`a'`, ``i` ``/``a` `` | Quote objects with escape handling. | Missing |
| Counts | Word object count extends through following objects. | Missing |

## Surround

| Keys | Old behavior | Status after compact rewrite |
| --- | --- | --- |
| `ys{motion}{delim}` | Add surrounding delimiter around motion range. | Partial |
| `ysi{object}{delim}`, `ysa{object}{delim}` | Add surrounding delimiter around text object. | Partial only for `ysiw)` |
| `ds{delim}` | Delete nearest surrounding delimiter pair. | Missing |
| `cs{from}{to}` | Change nearest surrounding delimiter pair. | Missing |
| Delimiters | `()`, `b`, `{}`, `B`, `[]`, `<>`, `"`, `'`, and backtick. | Partial |

## Registers

| Feature | Old behavior | Status after compact rewrite |
| --- | --- | --- |
| Char register | Charwise deletes/yanks captured as char register. | Missing |
| Line register | Linewise deletes/yanks captured as line register. | Partial |
| Empty delete/yank | Empty register recorded when operation has no deleted text. | Partial |
| Paste placement | Charwise and linewise `p`/`P` place text using Vim-style cursor targets. | Missing |

## Viewport Commands

| Keys | Old behavior | Status after compact rewrite |
| --- | --- | --- |
| `zz`, `zt`, `zb` | Reveal cursor centered/top/bottom. | Missing |
| `H`, `M`, `L` | Move cursor/visual head to visible top/middle/bottom screen row. | Missing |
| Page motions | Vim page motions preserve visual state when in Visual mode. | Present |

## Compatibility Notes

- The old implementation was custom and lived primarily in `crates/lst-editor/src/vim.rs`, `vim_edit.rs`, and the Vim executor section of `lib.rs`.
- Modalkit should be evaluated as a parser/keybinding source, not as a direct editor engine, unless lst deliberately adopts modalkit's buffer/store model. The direct engine route would duplicate or bypass existing lst transactions, selections, find panel, clipboard effects, and viewport synchronization.
- The first restoration milestone is parity with this file. The second is replacing custom parser pieces with modalkit-generated actions where that reduces code without weakening lst invariants.
