# Editor Partial-Item Bundles

Companion to `editor-behaviors-checklist.md`: takes every `[~]` item plus the
two "worth-noting" findings from the audit and groups them into bundles that
share a code surface, a state structure, or an infrastructure investment.

References use `path::symbol` to survive reorganization.

---

## 1. Find subsystem upgrade — ✅ shipped

Single module, single state struct, single panel.

- [x] **Grapheme: find-match cluster alignment** — `crates/lst-editor/src/find.rs::compute_matches_in_text` partitions cells via `cell_partition_by_byte` and skips matches not aligned to cluster boundaries (test `find.rs::grapheme_boundary_filters_mid_cluster_match`).
- [x] **Case sensitivity + smart case** — `FindState::case_sensitive` plus uppercase-aware `build_regex` (`crates/lst-editor/src/find.rs::FindState::build_regex`).
- [x] **Whole-word toggle** — `FindState::whole_word` wraps the pattern with `\b…\b` (`build_regex`).
- [x] **Regex toggle** — `FindState::use_regex`; replace requests expand capture refs via `caps.expand` in `crates/lst-editor/src/find.rs`.
- [x] **Find-in-selection scope** — `FindScope::{Document, Selection}` clamps matches; `EditorModel::toggle_find_in_selection` re-derives scope from the active selection.

Driven by the `Command::ToggleFindCaseSensitive` / `ToggleFindWholeWord` /
`ToggleFindRegex` / `ToggleFindInSelection` model commands, wired through the
find panel in `apps/lst-gpui/src/shell.rs` and bound in the keybinding table in
`apps/lst-gpui/src/workspace_action.rs`.

## 2. User config-file infrastructure

Both items want the same loader; designing the schema once unlocks several
other future settings (theme, autosave cadence, per-language save hooks).

- [~] **Configurable keybindings** — `apps/lst-gpui/src/workspace_action.rs::editor_keybindings` builds a hardcoded `Vec<KeyBinding>` from the `BINDINGS` table; no config-load infra.
- [~] **Language picker / per-file override** — language is auto-detected per tab (`crates/lst-editor/src/tab.rs::detect_language`), but there is no override API, command palette, or config file for language overrides.

Tradeoff: you commit to a config schema (likely `~/.config/lst/config.toml`)
that is painful to break later — design it once, deliberately.

## 3. Editing & selection primitives — ✅ shipped

- [x] **Block-comment toggle** — `LanguageConfig::block_comment` carries the open/close pair; `EditorModel::toggle_block_comment` delegates delimiter transaction construction to `line_edit` (`crates/lst-editor/src/language.rs::LanguageConfig::block_comment`; `crates/lst-editor/src/lib.rs::toggle_block_comment`; `crates/lst-editor/src/line_edit.rs::toggle_block_comment_request`; bindings `ctrl/cmd-shift-/`).
- [x] **Soft-tab backspace** — `crates/lst-editor/src/lib.rs::soft_tab_backspace_range` snaps backspace to a full `IndentStyle::indent_unit` when the cursor sits in leading whitespace; falls back to a single grapheme otherwise.
- [x] **Select line / select paragraph** — `EditorModel::select_current_line` / `::select_current_paragraph` plus `SelectLine` (`ctrl/cmd-l`) and `SelectParagraph` (`ctrl/cmd-shift-p`) actions.
- [x] **Quad-click paragraph** — `apps/lst-gpui/src/input.rs::on_mouse_down` routes `click_count >= 4` to the enclosing paragraph via `paragraph_range_at_char`.

## 4. Tabs, rendering chrome & history — ✅ shipped

Heterogeneous but each item is small and standalone — file together because
they don't fit the other groups, not because they share a surface.

- [x] **Tab reorder** — `TabSet::reorder` plus `EditorModel::move_active_tab`, exposed via `MoveTabLeft`/`MoveTabRight` (`ctrl/cmd-shift-pageup/pagedown`).
- [x] **Line numbers (relative / hybrid)** — `GutterMode` (Absolute/Relative/Hybrid), `EditorModel::cycle_gutter_mode`, `ToggleLineNumberMode` (`alt-l`).
- [x] **Wrap segment grapheme alignment** — `crates/lst-editor/src/wrap.rs` walks `cells_of_str` so wrap segments never split a grapheme cluster.
- [x] **Redo branch preservation** — `EditHistory` keeps abandoned redo paths; `swap_redo_branch` cycles through them (`ctrl-alt-y` / `cmd-alt-shift-z`).
- [x] **Last edit location (`gi`/`g;`)** — `EditorTab::last_edit_position` plus `VimCommand::JumpToLastEdit { enter_insert }`; `g;` jumps, `gi` jumps and enters Insert.

---

## Suggested order

Groups 1, 3, and 4 are shipped. The only remaining bundle is Group 2 (user
config infrastructure) — the schema decision is the hard part; designing it
once unlocks both keybindings and language overrides plus future settings
(theme, autosave cadence, per-language hooks).
