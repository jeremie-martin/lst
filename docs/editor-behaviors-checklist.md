# Editor Behaviors Checklist

A comprehensive reference of behaviors a clean, idiomatic text editor should
implement. Use this as an audit checklist for the GPUI editor.

Status legend: `[ ]` not implemented · `[~]` partial · `[x]` done

Status last refreshed: 2026-05-05 (after multi-cursor easy-wins clusters).

References use `path::symbol` rather than `path:line` so they survive
reorganization. Grep for the symbol to navigate.

---

## Cursor Movement & Navigation

- [x] **Snap-to-end on last line** — standard `Down` / `PageDown` snap to the active last-line EOL while preserving `preferred_column` for the next upward move (`crates/lst-editor/src/lib.rs::move_vertical`, `::move_display_rows`, `::page_down`; tests `crates/lst-editor/tests/behavior.rs::logical_row_motion_snaps_to_document_edges`, `::logical_row_edge_snap_extends_selection`, `::logical_row_edge_snap_preserves_preferred_column`, `::display_row_motion_snaps_to_document_edges`; `crates/lst-editor/tests/viewport.rs::page_down_at_eof_snaps_to_line_end_and_emits_reveal`, `::half_page_down_at_eof_snaps_to_eol`). Vim vertical motion still clamps (`crates/lst-editor/src/vim.rs::compute_motion` `Motion::Down` arm; `crates/lst-editor/tests/viewport.rs::vim_arrow_down_at_eof_keeps_vim_clamp_behavior`)
- [x] **Snap-to-start on first line** — standard `Up` / `PageUp` snap to column 0 on the first line while preserving `preferred_column` for the next downward move (same model paths as above; `crates/lst-editor/tests/viewport.rs::page_up_at_bof_snaps_to_line_start_and_emits_reveal`). Vim vertical motion still clamps (`crates/lst-editor/src/vim.rs::compute_motion` `Motion::Up` arm; `crates/lst-editor/tests/viewport.rs::vim_half_page_commands_keep_clamped_columns_at_document_edges`)
- [x] **Virtual / sticky column** — `preferred_column` preserved across Up/Down (`crates/lst-editor/src/vim.rs::cursor_motion_target`; `crates/lst-editor/src/lib.rs::move_vertical`)
- [x] **Word-wise motion** (`Ctrl+←/→`) — Word/Symbol/Whitespace classes (`crates/lst-editor/src/selection.rs::token_class`, `::previous_word_boundary_in_text`, `::next_word_boundary_in_text`)
- [x] **Subword motion** — `Alt+←/→` and `Alt+Shift+←/→` use shared subword helpers in both the editor and inline inputs, splitting camelCase / snake_case / digit transitions while keeping `Ctrl+←/→`, double-click word selection, and delete-word behavior whole-word (`crates/lst-editor/src/selection.rs::subword_class`, `::previous_subword_boundary`, `::next_subword_boundary`; `crates/lst-editor/src/lib.rs::move_subword`; `apps/lst-gpui/src/keymap.rs` `alt-left`/`alt-right` bindings; `apps/lst-gpui/src/ui/input_field.rs::previous_subword_boundary`)
- [x] **Smart Home** — editor `Home` / `Shift+Home` toggle between first non-blank and column 0 from the current selection head; `cmd-left` remains hard line-start and Vim `Home` still maps to `0` (`apps/lst-gpui/src/keymap.rs` `MoveSmartHome` / `SelectSmartHome` bindings; `crates/lst-editor/src/lib.rs::smart_home`, `::move_smart_home_inner`; tests `crates/lst-editor/tests/behavior.rs::smart_home_toggles_between_first_non_blank_and_line_start`, `::smart_home_selection_tracks_the_selection_head`, `::smart_home_clears_preferred_column_and_skips_noop_reveal`; binding asserts in `apps/lst-gpui/src/tests.rs`)
- [x] **End-of-line** — `$` stops at last char (`crates/lst-editor/src/vim.rs::compute_motion` `Motion::LineEnd` arm)
- [x] **Document start/end** — `Ctrl+Home`/`Ctrl+End` wired (`apps/lst-gpui/src/keymap.rs` `ctrl-home`/`ctrl-end` bindings; `crates/lst-editor/src/lib.rs::move_document_boundary`)
- [x] **Page up/down** — viewport-relative (`crates/lst-editor/src/lib.rs::page_down`, `::page_up`)
- [x] **Half-page scroll** (Vim `Ctrl+D`/`Ctrl+U`) (`crates/lst-editor/src/lib.rs::half_page_down`, `::half_page_up`; `crates/lst-editor/src/viewport.rs::Viewport::half_page`)
- [x] **Scroll without moving cursor** — `scroll_to_*` reveal APIs (`crates/lst-editor/src/lib.rs::scroll_to_center`, `::scroll_to_top`, `::scroll_to_bottom`) and the viewport's vertical scroll container (`apps/lst-gpui/src/shell.rs` `overflow_y_scroll`)
- [x] **Matching bracket jump** (`%` in Vim) (`crates/lst-editor/src/vim.rs::compute_motion` `Motion::Percent` arm → `match_bracket`)
- [x] **Go to line** — panel with integer parser (`crates/lst-editor/src/lib.rs::submit_goto_line`)
- [x] **Go to column** — `submit_goto_line` accepts `line:column` and clamps both values (`crates/lst-editor/src/lib.rs::submit_goto_line`)
- [ ] **Jump list / navigation history**
- [x] **Last edit location** (`gi` / `g;`) — `EditorTab::last_edit_position` tracks the final selection head of the most-recent text-changing transaction; `VimCommand::JumpToLastEdit { enter_insert }` jumps (`g;`) or jumps + enters Insert (`gi`) (`crates/lst-editor/src/tab.rs::EditorTab::last_edit_position`, `::apply_edit_request`; `crates/lst-editor/src/vim.rs::VimCommand::JumpToLastEdit`; bindings in `apps/lst-gpui/src/keymap.rs` `g;` / `gi`)

## Selection

- [x] **Shift+motion extends selection** — `select: bool` threaded through all motions (`crates/lst-editor/src/lib.rs::move_to_char`, `::move_word`, `::move_subword`, `::move_line_boundary`)
- [x] **Selection anchor preservation** — `select_to` branches on `selection_reversed` (`crates/lst-editor/src/tab.rs::select_to`)
- [x] **Click-drag selection with auto-scroll** — `drag_autoscroll_target`, per-frame scheduling (`apps/lst-gpui/src/interactions.rs::schedule_drag_autoscroll`, `::run_drag_autoscroll`, `::drag_autoscroll_target`)
- [x] **Double-click word**, **triple-click line** (`apps/lst-gpui/src/interactions.rs::on_mouse_down` click_count branches)
- [x] **Quad-click paragraph** — `event.click_count >= 4` selects the enclosing paragraph via `paragraph_range_at_char` and seeds a `DragSelectionMode::Paragraph` (`apps/lst-gpui/src/interactions.rs::on_mouse_down`)
- [x] **Shift+click extends** (`apps/lst-gpui/src/interactions.rs::on_mouse_down` shift-modifier path through `move_to_char`)
- [ ] **Column / block selection** (Alt+drag)
- [x] **Select all** (`crates/lst-editor/src/tab.rs::EditorTab::select_all`)
- [ ] **Expand selection to enclosing scope** (smart select)
- [~] **Multi-cursor / multi-selection** — see the dedicated [Multi-Cursor & Multi-Selection](#multi-cursor--multi-selection) section for the full breakdown. Model foundation, multi-cursor text input (insert / delete / auto-pair / surround / overtype / smart-Enter-per-cursor / paste-distribute / cut-copy round-trip / column-mode), atomic batch edits, undo restoration, IME isolation, find-flag-aware Ctrl-D / Ctrl-Shift-L, Alt-click toggle on/off, Ctrl-Alt-Up/Down, Esc collapse, multi-cursor-aware status bar and gutter, and a head caret on every selection are all wired. Plain motion, line / Vim / find-scope / indent / comment / line-edit ops still collapse to the primary; remaining creation-gesture polish (Ctrl-U history, Ctrl-K Ctrl-D skip, Shift-Alt-I, dedicated F2-style word-occurrence binding) and per-cursor goal column / anchor-direction survival are open.
- [x] **Select line / select paragraph** — triple-click selects the line and quad-click the paragraph; Vim `V` and text objects cover both; non-Vim keyboard actions `SelectLine` (`ctrl/cmd-l`) and `SelectParagraph` (`ctrl/cmd-shift-p`) call `EditorModel::select_current_line` / `::select_current_paragraph` (`apps/lst-gpui/src/interactions.rs::on_mouse_down`; `crates/lst-editor/src/lib.rs::select_current_line`, `::select_current_paragraph`; `apps/lst-gpui/src/keymap.rs` `SelectLine` / `SelectParagraph` bindings)

## Multi-Cursor & Multi-Selection

State-of-the-art multi-cursor (VSCode-grade) is more than painting N carets.
The model-level foundation already enforces a normalized `SelectionSet`
(non-empty, ordered, non-overlapping) and batch text operations route through
`TextChangeSet` so multi-selection edits commit atomically
(`crates/lst-editor/src/selection.rs::SelectionSet`,
`::from_selections_coalescing_cursors`;
`crates/lst-editor/src/transaction.rs::TextChangeSet`;
`crates/lst-editor/src/multi_selection.rs::replacement_request`,
`::delete_request`;
`crates/lst-editor/src/lib.rs::set_selection_set`;
`apps/lst-gpui/src/viewport.rs::ViewportPaintInput`).

Today, the multi-cursor *text-input* path is broad — literal insert,
backspace, delete-forward, word-delete, auto-pair / surround / overtype /
auto-dedent, IME-aware short-circuit, smart-Enter-per-cursor, paste (with
line-count distribution), and cut / copy round-tripping all iterate the
selection set through a single `TextChangeSet`. Cursor-add gestures
(Alt-click with toggle, Ctrl-Alt-Up/Down, Ctrl-D and Ctrl-Shift-L with
find-flag awareness) and Esc-collapse round out the *interaction* path.
The collapse-to-primary boundaries lie elsewhere: every plain motion
(`tab.move_to` / `select_to` route through `SelectionSet::set_single`),
every line- / Vim- / find-scope- / indent- / comment- / line-edit op, and
column-mode is still bound to plain Alt-drag rather than the canonical
Shift-Alt-drag. The items below break down the full surface area this
section is meant to drive to completion.

### Cursor Set Invariants

- [x] **Normalized after every change** — sorted by position, no overlaps, never empty; duplicate collapsed cursors coalesce (`crates/lst-editor/src/selection.rs::SelectionSet`, `::from_selections_coalescing_cursors`)
- [x] **Atomic batch edits** — every multi-cursor text op compiles to a single `TextChangeSet` so positions stay consistent across all cursors (`crates/lst-editor/src/transaction.rs::TextChangeSet`, `::SelectionAfter::Exact`)
- [x] **Stable primary identity** — the primary cursor survives normalization and batch edits
- [ ] **Per-cursor goal column** — each cursor remembers its own preferred column across vertical motion (today only the primary cursor has `preferred_column`)
- [~] **Per-cursor anchor / head direction** — `Selection` carries distinct `anchor` / `head` and `is_reversed`, but every multi-cursor batch edit lands each cursor as `Selection::collapsed(...)` (`crates/lst-editor/src/multi_selection.rs::replacement_request`, `::delete_request`), so reverse-direction selections do not survive an edit
- [x] **Single undo step per multi-cursor op** — `HistorySnapshot { text, selection: SelectionSet }`; `apply_edit_request` records exactly one snapshot per `EditRequest`, so multi-cursor edits collapse to one undo entry and undo restores the full `SelectionSet` (`crates/lst-editor/src/history.rs::HistorySnapshot`; `crates/lst-editor/src/tab.rs::apply_edit_request`)

### Cursor Creation Gestures

- [x] **Alt-click adds / removes** a cursor at the click point (toggle on existing) — single Alt-click on a point already covered by a multi-cursor selection drops that cursor via `SelectionSet::with_removed_at`; otherwise the existing add path runs. Refuses to empty the set (single-selection sets are a no-op) (`apps/lst-gpui/src/interactions.rs::on_mouse_down`; `crates/lst-editor/src/lib.rs::remove_cursor_at_char`; `crates/lst-editor/src/selection.rs::SelectionSet::with_removed_at`)
- [x] **Ctrl-Alt-Up / Ctrl-Alt-Down** add a cursor on the adjacent line at the active visual column (`apps/lst-gpui/src/keymap.rs` `ctrl-alt-up`/`down` → `AddCursorAbove`/`Below`; `crates/lst-editor/src/lib.rs::add_cursor_above`, `::add_cursor_below`, `::add_cursor_on_adjacent_line`)
- [x] **Ctrl-D adds next occurrence** of the current word/selection — `multi_selection::occurrence_ranges` builds a regex via `find::build_query_regex` so the find panel's case / whole-word / smart-case flags apply. The selection itself is always treated literally (find's regex flag is intentionally ignored — there is no independent pattern to interpret) (`apps/lst-gpui/src/keymap.rs` `ctrl-d` → `SelectNextOccurrence`; `crates/lst-editor/src/lib.rs::select_next_occurrence`; `crates/lst-editor/src/multi_selection.rs::next_occurrence_addition`, `::occurrence_ranges`; `crates/lst-editor/src/find.rs::build_query_regex`)
- [ ] **Ctrl-K Ctrl-D skips the current match** and adds the next
- [x] **Ctrl-Shift-L selects all occurrences** of the current selection — same `find::build_query_regex` wiring as Ctrl-D, so case / whole-word / smart-case flags apply to file-wide cursor adds (`crates/lst-editor/src/lib.rs::select_all_occurrences`; `crates/lst-editor/src/multi_selection.rs::all_occurrences_set`)
- [~] **Select all occurrences of word under cursor** (file-wide, e.g. Ctrl-F2) — Ctrl-Shift-L with no selection falls back to `word_range_at_char` and now honours find flags via `build_query_regex`; no dedicated F2-style binding yet (`crates/lst-editor/src/multi_selection.rs::occurrence_query`)
- [ ] **Add cursor at end of every line in selection** (Shift-Alt-I)
- [ ] **Ctrl-U pops the last-added cursor** (cursor-history stack — important for undoing overshoot of Ctrl-D)
- [x] **Esc collapses** — clear non-empty selections first, then drop secondary cursors. Routes through `shell.rs::on_key_down` (after panel-dismiss handling, before vim escape) so two presses always reach a single collapsed cursor; nothing-to-collapse falls through to vim escape (`apps/lst-gpui/src/shell.rs::on_key_down`; `crates/lst-editor/src/lib.rs::collapse_to_primary`)
- [x] **Click without modifier collapses** to a single cursor at the click point — `on_mouse_down` without alt routes through `move_to_char` / `set_selection`, both of which call `SelectionSet::set_single` (`apps/lst-gpui/src/interactions.rs::on_mouse_down`; `crates/lst-editor/src/selection.rs::SelectionSet::set_single`)

### Per-Cursor Movement

- [ ] **All horizontal motions per cursor** — char, word, subword, line-boundary, smart Home
- [ ] **All vertical motions per cursor** — line, page, half-page, document edges
- [ ] **Shift-extend per cursor** — every cursor's head moves independently
- [ ] **Smart-expand / smart-shrink per cursor** — once smart-select lands, it must apply per cursor

### Per-Cursor Editing

- [x] **Literal text insert** applies at every cursor (`crates/lst-editor/src/multi_selection.rs::replacement_request`)
- [x] **Backspace / delete-forward** apply at every cursor (`crates/lst-editor/src/multi_selection.rs::delete_request`)
- [x] **Word delete** applies at every cursor
- [x] **Auto-pair brackets / quotes** apply at every cursor — `multi_edit_action` builds a `multi_request` with `auto_pair_insert_edit` per selection; all-or-nothing (defers if any selection rejects) (`crates/lst-editor/src/text_input.rs::multi_edit_action`, `::auto_pair_insert_edit`)
- [x] **Auto-pair surround** wraps every non-empty selection — same `multi_request` machinery using `auto_pair_surround_edit` per selection (`crates/lst-editor/src/text_input.rs::multi_edit_action`, `::auto_pair_surround_edit`)
- [x] **Auto-dedent on close bracket** applies at every cursor — per-selection `auto_dedent_close_brace_range` participates in the same `multi_request` (`crates/lst-editor/src/text_input.rs::multi_edit_action`, `::auto_dedent_close_brace_range`)
- [x] **Overtype mode** applies at every cursor — `auto_pair_overtype_cursor` per selection with `MultiSelectionAfter::AbsoluteCursor` for caret placement (`crates/lst-editor/src/text_input.rs::multi_edit_action`, `::auto_pair_overtype_cursor`)
- [x] **Smart Enter / auto-indent** applies at every cursor — `insert_newline` builds per-cursor newline + indent strings via `multi_selection::replacement_request_by_index`, computing `line_indent_prefix` from each cursor's own line so differently-indented cursors all get the right prefix (`crates/lst-editor/src/lib.rs::insert_newline`; `crates/lst-editor/src/multi_selection.rs::replacement_request_by_index`)
- [ ] **Indent / outdent** coalesces by line (two cursors on one line do not double-indent)
- [ ] **Toggle line / block comment** coalesces by line
- [ ] **Move line up / down** coalesces contiguous cursor clusters
- [ ] **Duplicate line / selection** applies per cursor
- [ ] **Delete line** coalesces by line
- [ ] **Join lines** applies per cursor cluster
- [ ] **Transpose / case conversion / sort** applies per cursor
- [ ] **Snippet tabstops** produce one cursor per `$N`; Tab walks tabstops in lockstep

### Clipboard Semantics

- [x] **Paste broadcasts** the clipboard to every cursor when there is a single fragment
- [x] **Paste distributes by line count** — if the clipboard has exactly N lines and there are N cursors, paste one line per cursor; otherwise broadcast (the canonical VSCode round-trip) (`crates/lst-editor/src/multi_selection.rs::paste_request`, `::clipboard_lines_for_distribution`)
- [x] **Cut / copy collects per cursor** — one fragment per cursor in document order, joined by `\n`; cut also routes through `apply_multi_selection_delete` to remove every selection (`crates/lst-editor/src/multi_selection.rs::selected_text_joined`; `crates/lst-editor/src/lib.rs::copy_selection`, `::cut_selection`)
- [x] **Copy → paste round-trip identity** — `selected_text_joined` emits an empty fragment for cursor-only selections instead of filtering them, so the clipboard always has exactly one line per selection. Pasting back into the same set rides `paste_request`'s line-count equality branch and reproduces the original layout (`crates/lst-editor/src/multi_selection.rs::selected_text_joined`, `::paste_request`)

### Search & Replace Integration

- [ ] **Find scope honours multi-selection** — `FindScope::Selection` covers every active selection, not just the primary
- [ ] **Replace-all in selection** respects the multi-selection set
- [x] **Cursor-add gestures read find flags** — Ctrl-D / Ctrl-Shift-L call into `find::build_query_regex` with the active `FindState`'s case / whole-word / smart-case flags (`use_regex` is intentionally not honoured — the selection is always treated literally)

### Mouse Gestures

- [x] **Alt-click toggle** — add a cursor, or remove if one already exists at that point (see [Cursor Creation Gestures](#cursor-creation-gestures) for the full reference)
- [~] **Alt-drag** adds an additional selection without disturbing existing ones — Alt-drag currently starts `DragSelectionMode::Column`, producing a rectangular selection rather than a free-form additive range (`apps/lst-gpui/src/interactions.rs::on_mouse_down`, `::apply_drag_selection_at_point`)
- [ ] **Shift-Alt-drag** column / box selection (one cursor per line of the rectangle)
- [ ] **Middle-click drag** as alternate column-select (platform-dependent)
- [ ] **Drag extends only the cursor under the mouse**, leaving the rest intact

### Visual Feedback

- [x] **All selections painted** (`apps/lst-gpui/src/viewport.rs::ViewportPaintInput`)
- [x] **All cursors painted** — `paint_viewport` iterates `paint_cursors`, which emits one `PaintCursor` per selection. Collapsed cursors take the wide block-cursor in Vim Normal; selections-with-extent get a thin caret on their head over the range fill (`apps/lst-gpui/src/viewport.rs::paint_cursors`, `::paint_viewport`)
- [ ] **All cursors blink in phase** (depends on cursor-blink work under [Rendering & Viewport](#rendering--viewport))
- [ ] **Primary-cursor distinction** — subtle visual differentiation (used as the reveal-on-scroll target)
- [~] **Reveal targets the primary** (or last-moved) cursor on movement — reveal uses `tab.cursor_char()` (primary head); adequate while motions collapse to primary, but no notion of "last-moved" once per-cursor motion lands (`apps/lst-gpui/src/main.rs::active_cursor_visual_row`, `::try_reveal_active_cursor`)
- [ ] **Off-screen-cursor indicator** — gutter/edge marker when cursors exist outside the viewport
- [x] **Gutter marker for cursor-bearing lines** — `ViewportPreparation::cursor_lines` lists every line whose selection head lives on it (sorted, deduped). Hybrid mode shows the absolute number on every cursor row instead of just the primary; Relative still anchors on the primary cursor's line (`apps/lst-gpui/src/viewport.rs::ViewportPreparation`; `crates/lst-editor/src/lib.rs::GutterMode::format`; `apps/lst-gpui/src/shell.rs` cursor_lines computation)
- [x] **Status-bar metrics** — `selection_summary` reports `N cursors · Sel M · K lines` whenever the selection set has multiple selections; falls back to the single-selection `Sel M` summary otherwise (`apps/lst-gpui/src/main.rs::selection_summary`, `::status_details`)

### Column / Box Selection

This block subsumes the standalone "Column / block selection" line under
[Selection](#selection).

- [~] **Shift-Alt-drag** creates a rectangular selection — the rectangle exists, but it is currently bound to plain Alt-drag, not Shift-Alt-drag; the spec gesture as such is not wired (`apps/lst-gpui/src/interactions.rs::on_mouse_down`; `crates/lst-editor/src/lib.rs::set_rectangular_column_selection`; `crates/lst-editor/src/multi_selection.rs::rectangular_selection_set`)
- [ ] **Ctrl-Shift-Alt-arrow** extends column selection by row / column
- [x] **Defined short-line policy** — `rectangular_selection_set` clamps via `char_at_line_column`, so lines shorter than `start_column` collapse to a cursor at line end and duplicates coalesce through `from_selections_coalescing_cursors` (`crates/lst-editor/src/multi_selection.rs::rectangular_selection_set`; `crates/lst-editor/src/selection.rs::char_at_line_column`)
- [x] **Insert in column mode** inserts at every column-aligned position — column selections are ordinary `SelectionSet`s, so multi-selection replacement applies directly (`crates/lst-editor/src/lib.rs::replace_text`; `crates/lst-editor/src/multi_selection.rs::replacement_request`)
- [x] **Backspace in column mode** deletes the column — `delete_selected_or_previous` detects `selection_set().has_multiple()` and routes to `apply_multi_selection_delete` regardless of whether the set was built by Alt-click or column drag (`crates/lst-editor/src/lib.rs::delete_selected_or_previous`; `crates/lst-editor/src/multi_selection.rs::delete_request`)

### IME, Composition, Macros

- [x] **IME composition routes to primary only** — `replace_and_mark_text` builds an `EditRequest::single` via `marked_text_request`, and `multi_selection::replacement_request` short-circuits when `tab.marked_range().is_some()` so secondary cursors stay inert during composition (`apps/lst-gpui/src/input_adapter.rs::replace_and_mark_text_in_range`; `crates/lst-editor/src/text_input.rs::marked_text_request`; `crates/lst-editor/src/multi_selection.rs::replacement_request`)
- [ ] **Macro recording policy defined** — single-cursor only, or whole multi-cursor op as one step (pick one)

### Performance

- [x] **No per-cursor LSP / lint / tokenize calls** — vacuously true today (no LSP/lint pipeline exists); tree-sitter highlighting is per-line and revision-keyed, not per-cursor (`apps/lst-gpui/src/viewport.rs::line_syntax_spans`). Revisit when LSP lands.
- [~] **Batched render** — one draw pass for all carets, one for all selection rects — `paint_viewport` runs per-row inner loops over `selection_set.as_slice()` (selection backgrounds) and `cursors` (caret quads); fine at typical caret counts but not document-batched (`apps/lst-gpui/src/viewport.rs::paint_viewport`)
- [ ] **Stays responsive at 1k cursors**; warn or cap at very large counts

## Editing Primitives

- [x] **Undo/redo with coalesced typing** — `EditHistory` owns snapshot boundaries; `EditKind::Insert/Delete` + `UndoBoundary::Merge` coalesce into one undo group (`crates/lst-editor/src/history.rs::EditHistory`, `::HistorySnapshot`; `crates/lst-editor/src/tab.rs::apply_edit_request`)
- [x] **Redo branch preservation** — abandoned redo paths are retained inside `EditHistory` (capped at `MAX_REDO_BRANCHES`) instead of being dropped; `swap_redo_branch` pulls the latest sibling branch back into reach (`crates/lst-editor/src/history.rs::EditHistory`, `::swap_redo_branch`; `crates/lst-editor/src/tab.rs::swap_redo_branch`; binding `ctrl-alt-y` / `cmd-alt-shift-z` `SwapRedoBranch` in `apps/lst-gpui/src/keymap.rs`)
- [x] **Backspace at start of line joins with previous** — `cursor-1..cursor` crosses newline when at col 0 (`crates/lst-editor/src/lib.rs::backspace` → `::delete_selected_or_previous`)
- [x] **Delete at end of line joins with next** — `cursor..cursor+1` crosses newline at EOL (`crates/lst-editor/src/lib.rs::delete_forward` → `::delete_selected_or_next`)
- [x] **Smart indent on Enter** — `line_indent_prefix` preserved (`crates/lst-editor/src/lib.rs::insert_newline`)
- [x] **Auto-dedent on close bracket** — typing a closer from the active language's `auto_dedent_closers` on a whitespace-only line removes one indent level (`indent.width()` leading ASCII spaces) before inserting the closer; Python / YAML / Lisps and other indent-sensitive languages disable it; tab-indented languages (Go, Makefile) also fall through to ordinary input (`crates/lst-editor/src/lib.rs::apply_text_input`; `crates/lst-editor/src/text_input.rs::auto_dedent_close_brace_range`; `crates/lst-editor/src/language.rs::LanguageConfig::auto_dedent_closers`)
- [x] **Indent/outdent selection** — Tab indents every touched line on a multi-line selection and Shift+Tab outdents (saturating at 0 leading spaces); single-line/no-selection Tab inserts the active language's indent unit (4 spaces in Rust/Python/C-family, 2 in JS/TS/YAML/Markdown/HTML/CSS, a literal `\t` in Go/Makefile). Vim `>>` / `<<` (with optional count, e.g. `2>>`) and visual-line `>` / `<` route through the same model API, with `line_edit` building the `TextChangeSet` instead of rebuilding the whole buffer (`crates/lst-editor/src/lib.rs::insert_tab_at_cursor`, `::outdent_at_cursor`; `crates/lst-editor/src/line_edit.rs::indent_request`, `::outdent_request`; `crates/lst-editor/src/transaction.rs::TextChangeSet`; `crates/lst-editor/src/vim.rs::VimCommand::IndentLines`, `::OutdentLines`; `crates/lst-editor/src/language.rs::IndentStyle`; `apps/lst-gpui/src/keymap.rs` `tab` / `shift-tab` bindings)
- [x] **Move line up/down** — swaps the active line with its neighbor through a localized transaction request outside `EditorModel` (`crates/lst-editor/src/lib.rs::move_line_up`, `::move_line_down`; `crates/lst-editor/src/line_edit.rs::line_swap_request`)
- [x] **Duplicate line/selection** — duplicates the current selection inline, or inserts a localized line transaction when there is no selection (`crates/lst-editor/src/lib.rs::duplicate_line`; `crates/lst-editor/src/line_edit.rs::duplicate_selection_request`, `::duplicate_line_request`)
- [x] **Delete line** — deletes the active line through a localized line-span transaction (`crates/lst-editor/src/lib.rs::delete_line`; `crates/lst-editor/src/line_edit.rs::delete_line_request`)
- [x] **Join lines with single-space collapse** — `vim_join_lines` delegates trim/join request construction to `vim_edit` (`crates/lst-editor/src/lib.rs::vim_join_lines`; `crates/lst-editor/src/vim_edit.rs::join_lines`)
- [ ] **Transpose**
- [x] **Toggle comment line/block** — line comments toggle via the active language's `line_comment`; block comments wrap/unwrap selections via the language's `block_comment` pair, falling back to a status message when neither is configured. Both paths build delimiter transactions in `line_edit` (`crates/lst-editor/src/lib.rs::toggle_comment`, `::toggle_block_comment`; `crates/lst-editor/src/line_edit.rs::toggle_comment_action`, `::toggle_block_comment_request`; `crates/lst-editor/src/transaction.rs::TextChangeSet`; `crates/lst-editor/src/language.rs::LanguageConfig::line_comment`, `::block_comment`; bindings `ctrl/cmd-/` and `ctrl/cmd-shift-/` in `apps/lst-gpui/src/keymap.rs`)
- [x] **Surround with brackets/quotes** — typing an opener with a non-empty selection wraps it via auto-pair (`crates/lst-editor/src/text_input.rs::auto_pair_surround_edit`); Vim `ys{motion}{char}`, `ds{char}`, and `cs{from}{to}` route through a `SurroundPhase` state machine and emit `VimCommand::SurroundRange` / `::DeleteSurround` / `::ChangeSurround`, with `vim_edit` building the edit request (`crates/lst-editor/src/vim.rs::SurroundPhase`, `::handle_normal` `'s'` arm, `::resolve_surround`; `crates/lst-editor/src/vim_edit.rs::surround_range`, `::delete_surround`, `::change_surround`)
- [x] **Auto-pair brackets/quotes** — the active language's `auto_pairs` drives the pair set; default includes `()`, `[]`, `{}`, `""`, `''`, `` `` ``; HTML / XML / JSX / TSX add `<>`; Rust suppresses `''` (lifetimes) via `auto_pair_suppress_quotes`; quotes still skip auto-pair when adjacent to an identifier char, after `\`, or when extending repeated quote/backtick runs; typing a closer when the next char already matches steps over it; IME and programmatic paths bypass auto-pair (`crates/lst-editor/src/lib.rs::apply_text_input`; `crates/lst-editor/src/text_input.rs::auto_pair_pair_for`; `crates/lst-editor/src/language.rs::LanguageConfig::auto_pairs`, `::auto_pair_suppress_quotes`). Known low-priority gap: no "inside unclosed string" detection (typing `"` inside `"foo |` still auto-pairs instead of closing).

## Clipboard

- [x] **Cut/copy/paste with platform clipboard** — `WriteClipboard`/`ReadClipboard` effects (`crates/lst-editor/src/lib.rs::copy_selection_inner`, `::cut_selection_inner`; `apps/lst-gpui/src/runtime.rs::handle_model_effects`)
- [x] **Write primary selection** (X11 middle-click) — `WritePrimary` effect (same model handlers; `apps/lst-gpui/src/runtime.rs::handle_model_effects`; middle-click paste at `apps/lst-gpui/src/interactions.rs::on_middle_mouse_down`)
- [x] **Cut/copy whole line when no selection** — copy/cut fall back to the current line via `selection_or_current_line` (`crates/lst-editor/src/lib.rs::selection_or_current_line`, `::copy_selection_inner`, `::cut_selection_inner`)
- [ ] **Paste preserves/normalizes indentation**
- [ ] **Clipboard history / kill ring**
- [ ] **Bracketed paste** (N/A for GUI, primarily a terminal concern)

## Search & Replace

- [x] **Incremental find** — `set_find_query_and_activate` reindexes live (`crates/lst-editor/src/lib.rs::set_find_query_and_activate`; `crates/lst-editor/src/find.rs::FindState::compute_matches_in_text`)
- [x] **Find next/previous** — modulo-wrap (`crates/lst-editor/src/find.rs::FindState::next`, `::prev`)
- [x] **Case sensitivity + smart case** — `FindState::case_sensitive` toggles strict case; otherwise `build_regex` ignores case unless the query has an uppercase char (`crates/lst-editor/src/find.rs::FindState::case_sensitive`, `::build_regex`; `crates/lst-editor/src/lib.rs::toggle_find_case_sensitive`; binding via `ToggleFindCase` in `apps/lst-gpui/src/actions.rs`)
- [x] **Whole word** toggle — `FindState::whole_word` wraps the pattern with `\b…\b` in `build_regex` (`crates/lst-editor/src/find.rs::FindState::whole_word`; `crates/lst-editor/src/lib.rs::toggle_find_whole_word`; `ToggleFindWholeWord`)
- [x] **Regex** toggle with capture groups — `FindState::use_regex` uses the raw query as a regex; replace requests interpret `$1`/`$&` capture refs; invalid patterns surface as `FindState::error` (`crates/lst-editor/src/find.rs::FindState::use_regex`, `::build_regex`, `::replace_one_request`, `::replace_all_request`; `ToggleFindRegex`)
- [x] **Find in selection** scope — `FindScope::{Document, Selection}` clamps matches to the active selection range; toggling re-derives the scope from the current selection (`crates/lst-editor/src/find.rs::FindScope`, `::FindState::scope`; `crates/lst-editor/src/lib.rs::toggle_find_in_selection`; `ToggleFindInSelection`)
- [x] **Replace / replace all** (`crates/lst-editor/src/find.rs::replace_one_request`, `::replace_all_request`; `crates/lst-editor/src/lib.rs::replace_one`, `::replace_all_matches`)
- [x] **Wrap-around at end** — modulo wrap (`crates/lst-editor/src/find.rs::FindState::next`)
- [x] **Highlight all matches** — `matches` vec stored separately from selection (`crates/lst-editor/src/find.rs::FindState`)
- [x] **Star search** (Vim `*`) — `SearchWordUnderCursor` (`crates/lst-editor/src/vim.rs::VimCommand::SearchWordUnderCursor`)

## Text Input

- [x] **IME composition** — full `EntityInputHandler` (marked range, bounds, unmark) (`apps/lst-gpui/src/input_adapter.rs::text_for_range`, `::marked_text_range`, `::unmark_text`, `::replace_and_mark_text_in_range`; model state at `crates/lst-editor/src/tab.rs::EditorTab::marked_range`; test `crates/lst-editor/tests/behavior.rs::ime_marked_text_replacement_remains_model_behavior`)
- [x] **Unicode grapheme clusters** — every cursor-position-producing helper walks `GraphemeCell`s built from `unicode-segmentation`'s extended grapheme clusters, classifying each cluster by its first scalar (matches Helix and Zed). Single-step motion uses `crates/lst-editor/src/selection.rs::next_grapheme_boundary`, `::previous_grapheme_boundary`, `::next_grapheme_column`, `::previous_grapheme_column`, `::last_grapheme_column`. Word, subword, and double-click selection use cell-based `::previous_word_boundary`, `::next_word_boundary`, `::previous_subword_boundary`, `::next_subword_boundary`, `::word_range_at_char` (and their `_in_text` byte-offset siblings used by `apps/lst-gpui/src/ui/input_field.rs`). Vim `w`/`b`/`e`, text objects (`iw`/`aw`/`iW`/`aW`), and `*` star-search route through `crates/lst-editor/src/vim.rs::word_forward`, `::word_backward`, `::word_end`, `::word_object_at`, `::word_under_cursor`, all walking cells via the shared `crates/lst-editor/src/selection.rs::cells_of_str`/`::cells_of_rope`/`::cells_of_rope_line` builders and `::cell_partition_by_char`/`::cell_containing_char` lookups. Find-match positions in `crates/lst-editor/src/find.rs::compute_matches_in_text` align to cluster boundaries via `cell_partition_by_byte` and skip mid-cluster regex hits (test `find.rs::grapheme_boundary_filters_mid_cluster_match`); wrap segments in `crates/lst-editor/src/wrap.rs` walk whole `GraphemeCell`s so they never split a cluster.
- [x] **Tab → spaces with soft-tab backspace** — Tab inserts the active language's indent unit via `IndentStyle::indent_unit` (4 spaces for Rust, 2 for JS/TS, a literal `\t` for Go); backspace inside the leading indent of an all-blank prefix deletes a full indent unit, not just one grapheme (`crates/lst-editor/src/lib.rs::insert_tab_at_cursor`, `::backspace`, `::delete_selected_or_previous`, `::soft_tab_backspace_range`; `crates/lst-editor/src/language.rs::IndentStyle::indent_unit`)
- [ ] **Trim trailing whitespace on save**
- [ ] **Ensure final newline on save**
- [x] **Detect/preserve line endings** — `preferred_newline_for_active_tab` scans for `\r\n` vs `\n` (`crates/lst-editor/src/lib.rs::preferred_newline_for_active_tab`)
- [ ] **Detect/preserve encoding** — no encoding detection

## Rendering & Viewport

- [x] **Soft wrap** with cursor movement across visual lines — `WrapLayout`, `move_display_rows` (`apps/lst-gpui/src/viewport.rs`; `crates/lst-editor/src/wrap.rs::build_wrap_layout`; `crates/lst-editor/src/lib.rs::move_display_rows`)
- [x] **Visual vs logical line motion** — `move_display_rows` (visual) vs `move_logical_rows` / `move_line_boundary` (`crates/lst-editor/src/lib.rs::move_display_rows`, `::move_logical_rows`, `::move_line_boundary`)
- [x] **Line numbers** — absolute / relative / hybrid via `GutterMode`, cycled through `cycle_gutter_mode` and toggled with `alt-l` `ToggleLineNumberMode` (`crates/lst-editor/src/lib.rs::GutterMode`, `::cycle_gutter_mode`, `::gutter_mode`; `apps/lst-gpui/src/viewport.rs` row-paint `gutter_lines` block; `apps/lst-gpui/src/keymap.rs` `ToggleLineNumberMode` binding)
- [ ] **Ruler / column guides**
- [x] **Current line highlight** — `CURRENT_LINE_BG` painted for row containing cursor (`apps/lst-gpui/src/viewport.rs` row background fill; theme constant in `apps/lst-gpui/src/ui/theme.rs`)
- [ ] **Cursor blink** respecting OS setting
- [x] **Scroll margin** — `DEFAULT_SCROLLOFF=4`, `DEFAULT_SIDESCROLLOFF=8` (`crates/lst-editor/src/viewport.rs::DEFAULT_SCROLLOFF`, `::DEFAULT_SIDESCROLLOFF`)
- [x] **Visible scrollbar when content overflows** — editor renders a slim vertical scrollbar overlay for overflowing content with thumb drag and track paging, backed by existing GPUI scroll handles (`apps/lst-gpui/src/shell.rs::render_editor_scrollbar`; `apps/lst-gpui/src/ui/scrollbar.rs`; tests `apps/lst-gpui/src/tests.rs::editor_scrollbar_drag_scrolls_without_text_selection`, `::editor_scrollbar_track_click_pages_without_text_selection`, `::editor_scrollbar_is_absent_without_overflow`). This is editor-only for now; tab-strip/general scrollbar reuse may be worth extracting later if more scroll surfaces need the same behavior.
- [x] **Horizontal scroll on long lines** — when soft-wrap is off the buffer scroll surface enables both axes (`apps/lst-gpui/src/shell.rs::LstGpuiApp::render` uses `overflow_x_scroll().overflow_y_scroll()` only when `!show_wrap`), the inner content is sized to the longest line × cached `char_width` (max-line-chars cached on `apps/lst-gpui/src/viewport.rs::ViewportCache::max_line_chars`), the painter offsets each row by the current horizontal scroll (`apps/lst-gpui/src/viewport.rs::paint_viewport`), a slim horizontal scrollbar at the bottom mirrors the vertical one (`apps/lst-gpui/src/shell.rs::render_editor_horizontal_scrollbar` and the `*_horizontal_*` helpers in `apps/lst-gpui/src/ui/scrollbar.rs`), and cursor moves trigger horizontal reveal-on-cursor that respects `Viewport::sidescrolloff` (`apps/lst-gpui/src/main.rs::try_reveal_active_cursor_horizontally`). Tests: `apps/lst-gpui/src/tests.rs::editor_horizontal_scrollbar_drag_scrolls_without_text_selection`, `::editor_horizontal_scrollbar_track_click_pages_without_text_selection`, `::editor_horizontal_scrollbar_is_absent_when_wrap_is_on`, `::editor_horizontal_scrollbar_is_absent_without_overflow`, `::arrow_right_at_long_line_scrolls_horizontally_to_keep_cursor_in_sidescrolloff`.
- [ ] **Minimap**
- [ ] **Indent guides**

## File & Buffer

- [x] **Dirty indicator** — tab UI renders a dirty bullet from `tab.modified()` (`apps/lst-gpui/src/shell.rs::render_tab` `dirty_marker`; `crates/lst-editor/src/tab.rs::EditorTab::modified`)
- [x] **Reload on external change prompt** — background polling checks file stamps and either reloads clean tabs or prompts on conflicts (`apps/lst-gpui/src/runtime.rs::start_background_tasks`, `::check_external_file_changes`)
- [x] **Auto-save** — `autosave_tick` / `AutosaveFile` effect (`crates/lst-editor/src/lib.rs::autosave_tick`; `apps/lst-gpui/src/runtime.rs::handle_model_effects` `AutosaveFile` arm → `::start_autosave_job`)
- [ ] **Recover from crash via swap/journal**
- [x] **Multiple tabs/buffers** — tabs with new/close/activate plus keyboard reorder via `MoveTabLeft`/`MoveTabRight` (`ctrl/cmd-shift-pageup`/`pagedown`); the active tab moves with its content under `TabSet::reorder` so positions stay stable (`crates/lst-editor/src/lib.rs::new_tab`, `::activate_tab`, `::move_active_tab`; `crates/lst-editor/src/tab_set.rs::TabSet::reorder`; `apps/lst-gpui/src/keymap.rs` `MoveTabLeft`/`MoveTabRight` bindings). Drag-to-reorder is still gesture-only future work.
- [ ] **Recently closed** reopen
- [~] **Filetype / language detection** — one registry in `lst-editor` detects language by filename → extension → shebang first line (Rust, Python, JS/TS/JSX/TSX, JSON/JSONC, TOML, YAML, Markdown, HTML/XML, CSS/SCSS, C/C++, Java, Go, Makefile, Dockerfile, shells, Lua, Lisps, etc.) and carries per-language indent / comments / auto-pair / auto-dedent in `LanguageConfig` (`crates/lst-editor/src/language.rs::Language`, `::LanguageConfig`, `::detect`, `::detect_from_filename`, `::detect_from_extension`, `::detect_from_shebang`; stored at `crates/lst-editor/src/tab.rs::EditorTab::language`; override via `crates/lst-editor/src/lib.rs::EditorModel::set_tab_language`; GPUI maps to tree-sitter grammars at `apps/lst-gpui/src/syntax.rs::SyntaxLanguage::from_language`, `::syntax_mode_for_language`). No user-facing language picker yet, no per-user config file

## Accessibility & Input

- [ ] **Screen reader** support — no a11y/aria code in the repo
- [x] **Keyboard-only operation** — all actions reachable via keys + Vim state machine
- [~] **Configurable keybindings** — keymap is hardcoded in `apps/lst-gpui/src/keymap.rs`; no user config file
- [ ] **Respect OS text settings** (double-click word-separators, repeat rate)

---

## Commonly-Missed Fundamentals

Items most often overlooked in custom editors:

- [x] Sticky virtual column across up/down motion
- [x] Smart Home (two-stage)
- [x] Grapheme-aware motion — single-step, word, subword, double-click word selection, Vim `w`/`b`/`e`, text objects, and `*` all walk grapheme clusters
- [x] Undo coalescing by word/time
- [x] Scroll margin
- [x] Auto-scroll during drag-selection
- [x] IME composition (EntityInputHandler)
- [x] Current-line highlight

---

## Summary

- **Done:** 107
- **Partial:** 9
- **Missing:** 45

**Strong foundation:** Vim state machine (operators, text objects, surround,
indent, jump-to-last-edit), viewport with scroll margin, soft wrap with
cluster-aligned segments, undo coalescing with redo-branch preservation,
autosave, find/replace with case/whole-word/regex/scope toggles and
cluster-aligned matches, drag-select with auto-scroll, IME composition,
gutter modes (absolute/relative/hybrid), current-line highlight, line-ending
detection, grapheme-cluster awareness across motion, selection, search, and
wrap, scrollbar overlays for both axes, horizontal-scroll reveal,
keyboard-driven tab reorder.

**Biggest gaps to close for "idiomatic" feel:**
1. Multi-cursor per-cursor motion — every motion currently funnels through `tab.move_to` / `select_to` (`SelectionSet::set_single`), dropping all non-primary cursors (see [Multi-Cursor & Multi-Selection](#per-cursor-movement))
2. Multi-cursor line- / Vim- / find-scope- / indent- / comment- / duplicate- / move-line ops still primary-only (see [Per-Cursor Editing](#per-cursor-editing) and [Search & Replace Integration](#search--replace-integration))
3. Multi-cursor creation polish — Ctrl-U history, Ctrl-K Ctrl-D skip, Shift-Alt-I, dedicated F2-style word-occurrence binding
4. Column / block selection on the canonical Shift-Alt-drag gesture (the rectangle exists; the spec gesture doesn't)
5. Cursor blink respecting OS setting
6. Trim-trailing-whitespace / ensure-final-newline on save
7. Recently-closed-tab reopen
8. Jump list / navigation history
9. User-configurable keybindings (config file)
10. User-facing language picker / manual override UI (model API exists)
11. Paste-preserves-indentation, transpose, clipboard history
