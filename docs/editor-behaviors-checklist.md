# Editor Behaviors Checklist

A comprehensive reference of behaviors a clean, idiomatic text editor should
implement. Use this as an audit checklist for the GPUI editor.

Status legend: `[ ]` not implemented · `[~]` partial · `[x]` done

Status last audited: 2026-04-15 (code paths verified directly).

---

## Cursor Movement & Navigation

- [x] **Snap-to-end on last line** — standard `Down` / `PageDown` snap to the active last-line EOL while preserving `preferred_column` for the next upward move (`crates/lst-editor/src/lib.rs:701`, `:741`, `:1727`; tests at `crates/lst-editor/tests/behavior.rs:332`, `:395`; `crates/lst-editor/tests/viewport.rs:204`). Vim vertical motion still clamps (`crates/lst-editor/src/vim.rs:1058`; `crates/lst-editor/tests/viewport.rs:239`)
- [x] **Snap-to-start on first line** — standard `Up` / `PageUp` snap to column 0 on the first line while preserving `preferred_column` for the next downward move (`crates/lst-editor/src/lib.rs:701`, `:741`, `:1734`; tests at `crates/lst-editor/tests/behavior.rs:332`, `:359`, `:395`; `crates/lst-editor/tests/viewport.rs:224`). Vim vertical motion still clamps (`crates/lst-editor/src/vim.rs:1065`; `crates/lst-editor/tests/viewport.rs:239`)
- [x] **Virtual / sticky column** — `preferred_column` preserved across Up/Down (`crates/lst-editor/src/vim.rs:896`; `lib.rs:667`)
- [x] **Word-wise motion** (`Ctrl+←/→`) — Word/Symbol/Space classes (`crates/lst-editor/src/selection.rs:11`, `:21`, `:38`)
- [x] **Subword motion** — `Alt+←/→` and `Alt+Shift+←/→` use shared subword helpers in both the editor and inline inputs, splitting camelCase / snake_case / digit transitions while keeping `Ctrl+←/→`, double-click word selection, and delete-word behavior whole-word (`crates/lst-editor/src/selection.rs`; `crates/lst-editor/src/lib.rs`; `apps/lst-gpui/src/keymap.rs`; `apps/lst-gpui/src/ui/input_field.rs`)
- [x] **Smart Home** — editor `Home` / `Shift+Home` toggle between first non-blank and column 0 from the current selection head; `cmd-left` remains hard line-start and Vim `Home` still maps to `0` (`apps/lst-gpui/src/keymap.rs:102`, `:106`; `crates/lst-editor/src/lib.rs:884`, `:1819`, `:2324`; tests at `crates/lst-editor/tests/behavior.rs:440`, `:474`, `:507`; `apps/lst-gpui/src/tests.rs:1461`; Vim at `crates/lst-editor/src/vim.rs:1268`)
- [x] **End-of-line** — `$` stops at last char (`crates/lst-editor/src/vim.rs:1127`)
- [x] **Document start/end** — `Ctrl+Home`/`Ctrl+End` wired (`apps/lst-gpui/src/keymap.rs:82`; `crates/lst-editor/src/lib.rs:805`)
- [x] **Page up/down** — viewport-relative (`crates/lst-editor/src/lib.rs:1627`)
- [x] **Half-page scroll** (Vim `Ctrl+D`/`Ctrl+U`) (`crates/lst-editor/src/lib.rs:1641`; `viewport.rs:43`)
- [x] **Scroll without moving cursor** — `scroll_to_*` APIs (`crates/lst-editor/src/lib.rs:1676`), mouse wheel scroll
- [x] **Matching bracket jump** (`%` in Vim) (`crates/lst-editor/src/vim.rs:1213`)
- [x] **Go to line** — panel with integer parser (`crates/lst-editor/src/lib.rs:437`)
- [ ] **Go to column** — `submit_goto_line` only parses an integer; no `line:column` syntax (`crates/lst-editor/src/lib.rs:441`)
- [ ] **Jump list / navigation history**
- [ ] **Last edit location** (`gi` / `g;`)

## Selection

- [x] **Shift+motion extends selection** — `select: bool` through all motions (`crates/lst-editor/src/lib.rs:206`)
- [x] **Selection anchor preservation** — `select_to` branches on `selection_reversed` (`crates/lst-editor/src/tab.rs:330`)
- [x] **Click-drag selection with auto-scroll** — `drag_autoscroll_target`, per-frame scheduling (`apps/lst-gpui/src/interactions.rs:183`, `:216`)
- [x] **Double-click word**, **triple-click line** (`apps/lst-gpui/src/interactions.rs:48`, `:57`)
- [ ] **Quad-click paragraph**
- [x] **Shift+click extends** (`apps/lst-gpui/src/interactions.rs:69`)
- [ ] **Column / block selection** (Alt+drag)
- [x] **Select all** (`crates/lst-editor/src/tab.rs:283`)
- [ ] **Expand selection to enclosing scope** (smart select)
- [ ] **Multi-cursor**
- [~] **Select line / select paragraph** — Vim `V` mode and triple-click exist; no non-Vim "select line" action (`crates/lst-editor/src/vim.rs:77`)

## Editing Primitives

- [x] **Undo/redo with coalesced typing** — `EditKind::Insert/Delete` + `UndoBoundary::Merge` skip snapshot (`crates/lst-editor/src/tab.rs:308`)
- [x] **Redo branch preservation** — linear redo stack, cleared on edit (`crates/lst-editor/src/tab.rs:318`)
- [x] **Backspace at start of line joins with previous** — `cursor-1..cursor` crosses newline when at col 0 (`crates/lst-editor/src/lib.rs:877`)
- [x] **Delete at end of line joins with next** — `cursor..cursor+1` crosses newline at EOL (`crates/lst-editor/src/lib.rs:894`)
- [x] **Smart indent on Enter** — `line_indent_prefix` preserved (`crates/lst-editor/src/lib.rs:919`)
- [ ] **Auto-dedent on close bracket**
- [ ] **Indent/outdent selection** — `insert_tab_at_cursor` inserts 4 spaces, replacing any selection (`crates/lst-editor/src/lib.rs:1731`)
- [x] **Move line up/down** (`crates/lst-editor/src/lib.rs:1743`)
- [x] **Duplicate line/selection** (`crates/lst-editor/src/lib.rs:1759`)
- [x] **Delete line** (`crates/lst-editor/src/lib.rs:1735`)
- [x] **Join lines with single-space collapse** — `vim_join_lines` trims and joins (`crates/lst-editor/src/lib.rs:1369`)
- [ ] **Transpose**
- [x] **Toggle comment line/block** — by file extension (`crates/lst-editor/src/lib.rs:1767`)
- [ ] **Surround with brackets/quotes** — only text-objects for `c`/`d` (`crates/lst-editor/src/vim.rs:1489`), no surround op
- [ ] **Auto-pair brackets/quotes**

## Clipboard

- [x] **Cut/copy/paste with platform clipboard** — `WriteClipboard`/`ReadClipboard` effects (`crates/lst-editor/src/lib.rs:937`; `apps/lst-gpui/src/runtime.rs:103`)
- [x] **Write primary selection** (X11 middle-click) — `WritePrimary` effect (`crates/lst-editor/src/lib.rs:938`; `apps/lst-gpui/src/runtime.rs:107`; middle-click paste at `interactions.rs:75`)
- [ ] **Cut/copy whole line when no selection** — `copy_selection_inner` returns false without selection (`crates/lst-editor/src/lib.rs:933`)
- [ ] **Paste preserves/normalizes indentation**
- [ ] **Clipboard history / kill ring**
- [ ] **Bracketed paste** (N/A for GUI, primarily a terminal concern)

## Search & Replace

- [x] **Incremental find** — `set_find_query_and_activate` reindexes live (`crates/lst-editor/src/lib.rs:288`; `find.rs:44`)
- [x] **Find next/previous** — modulo-wrap (`crates/lst-editor/src/find.rs:88`, `:95`)
- [ ] **Case sensitivity + smart case** — `line.find(&query)` is always case-sensitive (`crates/lst-editor/src/find.rs:53`)
- [ ] **Whole word** toggle
- [ ] **Regex** toggle with capture groups
- [ ] **Find in selection** scope
- [x] **Replace / replace all** (`crates/lst-editor/src/lib.rs:394`, `:411`)
- [x] **Wrap-around at end** — modulo wrap (`crates/lst-editor/src/find.rs:88`)
- [x] **Highlight all matches** — `matches` vec stored separately from selection (`crates/lst-editor/src/find.rs:17`)
- [x] **Star search** (Vim `*`) — `SearchWordUnderCursor` (`crates/lst-editor/src/vim.rs:139`)

## Text Input

- [x] **IME composition** — full `EntityInputHandler` (marked range, bounds, unmark) (`apps/lst-gpui/src/input_adapter.rs:68`, `:95`, `:131`; model at `crates/lst-editor/src/lib.rs:1471`; test `behavior.rs:822`)
- [~] **Unicode grapheme clusters** — graphemes used in `input_field` widget (`apps/lst-gpui/src/ui/input_field.rs:199`); main editor operates on `char`s
- [~] **Tab → spaces with soft-tab backspace** — inserts 4 spaces (`lib.rs:1732`); backspace deletes one char, not four
- [ ] **Trim trailing whitespace on save**
- [ ] **Ensure final newline on save**
- [x] **Detect/preserve line endings** — `preferred_newline_for_active_tab` scans for `\r\n` vs `\n` (`crates/lst-editor/src/lib.rs:2179`)
- [ ] **Detect/preserve encoding** — no encoding detection

## Rendering & Viewport

- [x] **Soft wrap** with cursor movement across visual lines — `WrapLayout`, `move_display_rows` (`apps/lst-gpui/src/viewport.rs`; `crates/lst-editor/src/wrap.rs`, `lib.rs:684`)
- [x] **Visual vs logical line motion** — `move_display_rows` (visual) vs `move_logical_rows` / `move_line_boundary` (`crates/lst-editor/src/lib.rs:684`, `:1603`)
- [~] **Line numbers** — absolute only, `{:>3}` format (`apps/lst-gpui/src/viewport.rs:487`); no relative/hybrid mode
- [ ] **Ruler / column guides**
- [x] **Current line highlight** — `CURRENT_LINE_BG` painted for row containing cursor (`apps/lst-gpui/src/viewport.rs:594`; theme at `ui/theme.rs:49`)
- [ ] **Cursor blink** respecting OS setting
- [x] **Scroll margin** — `DEFAULT_SCROLLOFF=4`, `DEFAULT_SIDESCROLLOFF=8` (`crates/lst-editor/src/viewport.rs:13`)
- [x] **Visible scrollbar when content overflows** — editor renders a slim vertical scrollbar overlay for overflowing content with thumb drag and track paging, backed by existing GPUI scroll handles (`apps/lst-gpui/src/shell.rs::render_editor_scrollbar`; `apps/lst-gpui/src/ui/scrollbar.rs`; tests `apps/lst-gpui/src/tests.rs::editor_scrollbar_drag_scrolls_without_text_selection`, `::editor_scrollbar_track_click_pages_without_text_selection`, `::editor_scrollbar_is_absent_without_overflow`). This is editor-only for now; tab-strip/general scrollbar reuse may be worth extracting later if more scroll surfaces need the same behavior.
- [x] **Horizontal scroll on long lines** — `sidescrolloff` when wrap off (`crates/lst-editor/src/viewport.rs:27`)
- [ ] **Minimap**
- [ ] **Indent guides**

## File & Buffer

- [x] **Dirty indicator** — `has_unsaved_changes` + `show_modified` (`crates/lst-editor/src/tab.rs`; display at UI layer)
- [x] **Reload on external change prompt** — `reload_tab_from_disk` + `suppress_file_conflict` (`crates/lst-editor/src/lib.rs:2105`, `:2128`)
- [x] **Auto-save** — `autosave_tick` / `AutosaveFile` effect (`crates/lst-editor/src/lib.rs:2021`; `apps/lst-gpui/src/runtime.rs:152`)
- [ ] **Recover from crash via swap/journal**
- [~] **Multiple tabs/buffers** — tabs, new/close/activate (`crates/lst-editor/src/lib.rs:1494`–`:1583`); **no reorder** (no drag, no move_tab action)
- [ ] **Recently closed** reopen

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
- [~] Grapheme-aware motion — only in the input-field widget, not the main editor
- [x] Undo coalescing by word/time
- [x] Scroll margin
- [x] Auto-scroll during drag-selection
- [x] IME composition (EntityInputHandler)
- [x] Current-line highlight

---

## Summary

- **Done:** 43
- **Partial:** 9
- **Missing:** 31

**Strong foundation:** Vim state machine, viewport with scroll margin, soft
wrap, undo coalescing, autosave, find/replace core, drag-select with
auto-scroll, IME composition, current-line highlight, line-ending detection.

**Biggest gaps to close for "idiomatic" feel:**
1. Find toggles: case sensitivity, smart case, whole-word, regex
2. Auto-pair brackets/quotes + surround op
3. Indent/outdent on selection (Tab/Shift+Tab doesn't indent today)
4. Cut/copy whole line when no selection
5. Grapheme-cluster-aware motion in the main editor
6. Cursor blink
7. Trim-trailing-whitespace / ensure-final-newline on save
8. Tab reordering, recently-closed reopen
9. Jump list / last-edit-location
10. Multi-cursor
11. User-configurable keybindings (config file)

---

## Corrections vs. first audit

The first pass (via a search subagent) mis-reported several items. Verified
directly against source:

- **IME composition** — previously MISSING, actually **DONE** (`input_adapter.rs:95`)
- **Current line highlight** — previously MISSING, actually **DONE** (`viewport.rs:594`)
- **Backspace/Delete at line boundary** — previously PARTIAL, actually **DONE** (deletes cross the newline at col 0 / EOL)
- **Line-ending detect/preserve** — previously PARTIAL, actually **DONE** (`lib.rs:2179`)
- **Configurable keybindings** — previously DONE, actually **PARTIAL** (hardcoded, no config file)
- **Tab reordering** — previously DONE, actually **PARTIAL** (tabs exist, no reorder)
- **Go to column** — previously PARTIAL, actually **MISSING** (integer only)
- **Subword motion** — now **DONE** for standard editor movement and inline inputs while whole-word selection/delete behavior stays unchanged (`crates/lst-editor/src/selection.rs`; `crates/lst-editor/src/lib.rs`; `apps/lst-gpui/src/ui/input_field.rs`)
