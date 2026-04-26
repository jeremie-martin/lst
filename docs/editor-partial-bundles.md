# Editor Partial-Item Bundles

Companion to `editor-behaviors-checklist.md`: takes every `[~]` item plus the
two "worth-noting" findings from the audit and groups them into bundles that
share a code surface, a state structure, or an infrastructure investment.

References use `path::symbol` to survive reorganization.

---

## 1. Find subsystem upgrade

Single module, single state struct, single panel. Highest "idiomatic feel"
payoff — the checklist summary already calls it out as gap #1.

- [~] **Grapheme: find-match cluster alignment** — `crates/lst-editor/src/find.rs::compute_matches_in_text` rounds match positions via `.chars().count()` rather than grapheme cluster boundaries.
- Natural `[ ]` ride-alongs (same edit point):
  - Case sensitivity + smart case (`line.find(&query)` is currently always case-sensitive)
  - Whole-word toggle
  - Regex toggle (capture groups)
  - Find-in-selection scope

Tradeoff: regex pulls in a crate dep and changes the "find next" perf profile;
worth deciding up front whether to gate it behind a toggle.

## 2. User config-file infrastructure

Both items want the same loader; designing the schema once unlocks several
other future settings (theme, autosave cadence, per-language save hooks).

- [~] **Configurable keybindings** — `apps/lst-gpui/src/keymap.rs::editor_keybindings` returns a hardcoded `Vec<KeyBinding>`; no config-load infra.
- [~] **Language picker / per-file override** — model API exists at `crates/lst-editor/src/lib.rs::EditorModel::set_tab_language`, but no UI command palette and no config file for language overrides.

Tradeoff: you commit to a config schema (likely `~/.config/lst/config.toml`)
that is painful to break later — design it once, deliberately.

## 3. Editing & selection primitives

Small, scattered "didn't quite finish" items that share testing patterns and
mostly live in `crates/lst-editor/src/lib.rs` + `editor_ops.rs`. Naturally
shippable as a single "editing-ops sweep."

- [~] **Block-comment toggle** — `crates/lst-editor/src/language.rs::LanguageConfig` only has `line_comment`; no `block_comment` field.
- [~] **Vim surround (`ys`/`ds`/`cs`)** — only `crates/lst-editor/src/lib.rs::auto_pair_surround_edit` (typing an opener over a selection wraps it); no Vim-style surround ops in `crates/lst-editor/src/vim.rs`.
- [~] **Soft-tab backspace** — `crates/lst-editor/src/lib.rs::backspace` deletes one grapheme via `delete_selected_or_previous`; should delete a full `IndentStyle::indent_unit` when the cursor sits in leading indent.
- [~] **Select line / select paragraph** — only triple-click and Vim `V` / text objects exist. No non-Vim keyboard action; nothing in `apps/lst-gpui/src/keymap.rs`.
- **Worth noting:** Quad-click paragraph (`[ ]`) — `apps/lst-gpui/src/interactions.rs::on_mouse_down` uses `click_count >= 3`, so quadruple-click currently re-runs triple-click line-select instead of escalating to paragraph. Cheap to wire up once paragraph-select exists.

## 4. Tabs, rendering chrome & history

Heterogeneous but each item is small and standalone — file together because
they don't fit the other groups, not because they share a surface.

- [x] **Tab reorder** — `TabSet::reorder` plus `EditorModel::move_active_tab`, exposed via `MoveTabLeft`/`MoveTabRight` (`ctrl/cmd-shift-pageup/pagedown`).
- [x] **Line numbers (relative / hybrid)** — `GutterMode` (Absolute/Relative/Hybrid), `EditorModel::cycle_gutter_mode`, `ToggleLineNumberMode` (`alt-l`).
- [x] **Wrap segment grapheme alignment** — `crates/lst-editor/src/wrap.rs` walks `cells_of_str` so wrap segments never split a grapheme cluster.
- [x] **Redo branch preservation** — `EditorTab::redo_branches` keeps abandoned redo paths; `swap_redo_branch` cycles through them (`ctrl-alt-y` / `cmd-alt-shift-z`).
- [x] **Last edit location (`gi`/`g;`)** — `EditorTab::last_edit_position` plus `VimCommand::JumpToLastEdit { enter_insert }`; `g;` jumps, `gi` jumps and enters Insert.

---

## Suggested order

1. **Group 1** — biggest perceived-quality win, contained to `find.rs` + find panel.
2. **Group 2** — unlocks future settings; the schema decision is the hard part.
3. **Group 3** — pick whichever subset fits a sprint; each item is self-contained.
4. **Group 4** — opportunistic, tackle when adjacent code is already open.
