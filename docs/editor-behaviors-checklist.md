# Editor Behaviors Checklist

A comprehensive reference of behaviors a clean, idiomatic text editor should
implement. Use this as an audit checklist for the GPUI editor.

Status legend: `[ ]` not implemented · `[~]` partial · `[x]` done

Status last refreshed: 2026-04-26.

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
- [x] **Last edit location** (`gi` / `g;`) — `EditorTab::last_edit_position` tracks the most-recent edit; `VimCommand::JumpToLastEdit { enter_insert }` jumps (`g;`) or jumps + enters Insert (`gi`) (`crates/lst-editor/src/tab.rs::EditorTab::last_edit_position`, `::push_undo_snapshot`; `crates/lst-editor/src/vim.rs::VimCommand::JumpToLastEdit`; bindings in `apps/lst-gpui/src/keymap.rs` `g;` / `gi`)

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
- [ ] **Multi-cursor**
- [x] **Select line / select paragraph** — triple-click selects the line and quad-click the paragraph; Vim `V` and text objects cover both; non-Vim keyboard actions `SelectLine` (`ctrl/cmd-l`) and `SelectParagraph` (`ctrl/cmd-shift-p`) call `EditorModel::select_current_line` / `::select_current_paragraph` (`apps/lst-gpui/src/interactions.rs::on_mouse_down`; `crates/lst-editor/src/lib.rs::select_current_line`, `::select_current_paragraph`; `apps/lst-gpui/src/keymap.rs` `SelectLine` / `SelectParagraph` bindings)

## Editing Primitives

- [x] **Undo/redo with coalesced typing** — `EditKind::Insert/Delete` + `UndoBoundary::Merge` skip snapshot (`crates/lst-editor/src/tab.rs::push_undo_snapshot`)
- [x] **Redo branch preservation** — abandoned redo paths are pushed onto `EditorTab::redo_branches` (capped at `MAX_REDO_BRANCHES`) instead of being dropped; `swap_redo_branch` pulls the latest sibling branch back into reach (`crates/lst-editor/src/tab.rs::push_undo_snapshot`, `::swap_redo_branch`, `::redo_branches`; binding `ctrl-alt-y` / `cmd-alt-shift-z` `SwapRedoBranch` in `apps/lst-gpui/src/keymap.rs`)
- [x] **Backspace at start of line joins with previous** — `cursor-1..cursor` crosses newline when at col 0 (`crates/lst-editor/src/lib.rs::backspace` → `::delete_selected_or_previous`)
- [x] **Delete at end of line joins with next** — `cursor..cursor+1` crosses newline at EOL (`crates/lst-editor/src/lib.rs::delete_forward` → `::delete_selected_or_next`)
- [x] **Smart indent on Enter** — `line_indent_prefix` preserved (`crates/lst-editor/src/lib.rs::insert_newline`)
- [x] **Auto-dedent on close bracket** — typing a closer from the active language's `auto_dedent_closers` on a whitespace-only line removes one indent level (`indent.width()` leading ASCII spaces) before inserting the closer; Python / YAML / Lisps and other indent-sensitive languages disable it; tab-indented languages (Go, Makefile) also fall through to ordinary input (`crates/lst-editor/src/lib.rs::apply_text_input`, `::auto_dedent_close_brace_range`; `crates/lst-editor/src/language.rs::LanguageConfig::auto_dedent_closers`)
- [x] **Indent/outdent selection** — Tab indents every touched line on a multi-line selection and Shift+Tab outdents (saturating at 0 leading spaces); single-line/no-selection Tab inserts the active language's indent unit (4 spaces in Rust/Python/C-family, 2 in JS/TS/YAML/Markdown/HTML/CSS, a literal `\t` in Go/Makefile). Vim `>>` / `<<` (with optional count, e.g. `2>>`) and visual-line `>` / `<` route through the same `indent_selected_lines` / `outdent_selected_lines` path (`crates/lst-editor/src/lib.rs::insert_tab_at_cursor`, `::outdent_at_cursor`, `::indent_selected_lines`, `::outdent_selected_lines`; `crates/lst-editor/src/vim.rs::VimCommand::IndentLines`, `::OutdentLines`; `crates/lst-editor/src/editor_ops.rs::indent_lines`, `::outdent_lines`; `crates/lst-editor/src/language.rs::IndentStyle`; `apps/lst-gpui/src/keymap.rs` `tab` / `shift-tab` bindings)
- [x] **Move line up/down** (`crates/lst-editor/src/lib.rs::move_line_up`, `::move_line_down`; `crates/lst-editor/src/editor_ops.rs::move_line_up`, `::move_line_down`)
- [x] **Duplicate line/selection** — duplicates the current selection inline, or the current line when there is no selection (`crates/lst-editor/src/lib.rs::duplicate_line`; `crates/lst-editor/src/editor_ops.rs::duplicate_line`)
- [x] **Delete line** (`crates/lst-editor/src/lib.rs::delete_line`; `crates/lst-editor/src/editor_ops.rs::delete_line`)
- [x] **Join lines with single-space collapse** — `vim_join_lines` trims and joins (`crates/lst-editor/src/lib.rs::vim_join_lines`)
- [ ] **Transpose**
- [x] **Toggle comment line/block** — line comments toggle via the active language's `line_comment`; block comments wrap/unwrap selections via the language's `block_comment` pair, falling back to a status message when neither is configured (`crates/lst-editor/src/lib.rs::toggle_comment`, `::toggle_block_comment`; `crates/lst-editor/src/editor_ops.rs::toggle_comment`, `::toggle_block_comment`; `crates/lst-editor/src/language.rs::LanguageConfig::line_comment`, `::block_comment`; bindings `ctrl/cmd-/` and `ctrl/cmd-shift-/` in `apps/lst-gpui/src/keymap.rs`)
- [x] **Surround with brackets/quotes** — typing an opener with a non-empty selection wraps it via auto-pair (`crates/lst-editor/src/lib.rs::auto_pair_surround_edit`); Vim `ys{motion}{char}`, `ds{char}`, and `cs{from}{to}` route through a `SurroundPhase` state machine and emit `VimCommand::SurroundRange` / `::DeleteSurround` / `::ChangeSurround` (`crates/lst-editor/src/vim.rs::SurroundPhase`, `::handle_normal` `'s'` arm, `::resolve_surround`; apply paths in `crates/lst-editor/src/lib.rs` for the three commands)
- [x] **Auto-pair brackets/quotes** — the active language's `auto_pairs` drives the pair set; default includes `()`, `[]`, `{}`, `""`, `''`, `` `` ``; HTML / XML / JSX / TSX add `<>`; Rust suppresses `''` (lifetimes) via `auto_pair_suppress_quotes`; quotes still skip auto-pair when adjacent to an identifier char, after `\`, or when extending repeated quote/backtick runs; typing a closer when the next char already matches steps over it; IME and programmatic paths bypass auto-pair (`crates/lst-editor/src/lib.rs::apply_text_input`, `::auto_pair_pair_for`; `crates/lst-editor/src/language.rs::LanguageConfig::auto_pairs`, `::auto_pair_suppress_quotes`). Known low-priority gaps: no "inside unclosed string" detection (typing `"` inside `"foo |` still auto-pairs instead of closing); a dangling `marked_range` from an IME composition can be consumed by the non-IME path and fed into auto-pair (untested).

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
- [x] **Regex** toggle with capture groups — `FindState::use_regex` uses the raw query as a regex; replace paths interpret `$1`/`$&` capture refs; invalid patterns surface as `FindState::error` (`crates/lst-editor/src/find.rs::FindState::use_regex`, `::build_regex`; `crates/lst-editor/src/lib.rs::toggle_find_regex`, `::replace_one`, `::replace_all_matches`; `ToggleFindRegex`)
- [x] **Find in selection** scope — `FindScope::{Document, Selection}` clamps matches to the active selection range; toggling re-derives the scope from the current selection (`crates/lst-editor/src/find.rs::FindScope`, `::FindState::scope`; `crates/lst-editor/src/lib.rs::toggle_find_in_selection`; `ToggleFindInSelection`)
- [x] **Replace / replace all** (`crates/lst-editor/src/lib.rs::replace_one`, `::replace_all_matches`)
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

- **Done:** 74
- **Partial:** 2
- **Missing:** 19

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
1. Cursor blink respecting OS setting
2. Trim-trailing-whitespace / ensure-final-newline on save
3. Recently-closed-tab reopen
4. Jump list / navigation history
5. Multi-cursor and column/block selection
6. User-configurable keybindings (config file)
7. User-facing language picker / manual override UI (model API exists)
8. Paste-preserves-indentation, transpose, clipboard history
