# Editor Behaviors Checklist

This is the product behavior checklist for `lst`. It describes what the editor
should do from a user's point of view. It is intentionally black-box: behavior
items must not depend on production implementation details.

Status legend: `[ ]` missing · `[~]` partial · `[x]` done

Status last refreshed: 2026-07-12 (daily-driver interaction, file safety,
search/replace, settings, and responsiveness pass).

Real-display tests under `apps/lst-gpui/tests/real_x11_*.rs` are the executable
reference for this checklist. When an item links tests, those tests should assert
only user-visible state: file contents, clipboard contents, cursor and selection
positions, visible modes/panels/status text, or viewport-observable geometry.

---

## Cursor Movement & Navigation

- [x] **Snap-to-end on last line** - standard `Down` / `PageDown` eventually land on the last line's end while preserving the intended column for later upward movement. X11: `real_x11_motion.rs`.
- [x] **Snap-to-start on first line** - standard `Up` / `PageUp` eventually land at column 0 on the first line while preserving the intended column for later downward movement.
- [x] **Virtual / sticky column** - vertical motion remembers the intended column across shorter lines and restores it when possible.
- [x] **Word-wise motion** - `Ctrl-Left` / `Ctrl-Right` move across word boundaries without moving backward unexpectedly. X11: `real_x11_motion.rs`.
- [x] **Subword motion** - `Alt-Left` / `Alt-Right` stop at camelCase, snake_case, and digit-transition boundaries. X11: `real_x11_motion.rs`.
- [x] **Smart Home** - `Home` toggles between first non-blank and column 0; `Shift-Home` extends selection with the same target behavior. X11: `real_x11_motion.rs`.
- [x] **End-of-line** - end-of-line motions land at the line's end without overshooting.
- [x] **Document start/end** - `Ctrl-Home` / `Ctrl-End` move to document boundaries. X11: `real_x11_motion.rs`.
- [x] **Page up/down** - page motion is viewport-relative and reaches document edges predictably. X11: `real_x11_motion.rs`.
- [x] **Half-page scroll** - Vim `Ctrl-D` / `Ctrl-U` move by half a viewport.
- [x] **Scroll without moving cursor** - scroll commands can reposition the viewport without changing the cursor.
- [x] **Matching bracket jump** - Vim `%` jumps between matching brackets.
- [x] **Go to line** - the goto panel accepts a line number, moves there, and returns focus to editing. X11: `real_x11_workflows.rs`, `real_x11_state_trace.rs`.
- [x] **Go to column** - the goto panel accepts `line:column` and clamps out-of-range values. X11: `real_x11_workflows.rs`.
- [ ] **Jump list / navigation history** - jumps between meaningful prior locations.
- [x] **Last edit location** - Vim `gi` / `g;` return to the last edit location, with `gi` entering Insert.
- [~] **Line bookmarks** - `Ctrl-Alt-K` toggles a per-buffer bookmark on the current line; `Ctrl-Alt-L` / `Ctrl-Alt-J` jump to the next/previous bookmark with wraparound and recenter the viewport. Bookmarks survive edits and undo/redo. X11: the under-review `real_x11_bookmarks_tdd.rs` spec (x11-tdd lane), pending promotion to a stable name.

## Selection

- [x] **Shift-motion extends selection** - shift-modified movement extends from the original anchor.
- [x] **Selection anchor preservation** - extending in either direction keeps the expected anchor and moves only the head.
- [x] **Click-drag selection with auto-scroll** - dragging selects text and continues extending while the pointer is beyond the viewport. X11: `real_x11_mouse.rs`.
- [x] **Click below last line jumps to end-of-document** - clicking in the empty area below the last painted line moves the caret to the end of the document, regardless of x. X11: `real_x11_mouse.rs`.
- [x] **Double-click word** - double-click selects the word under the pointer. X11: `real_x11_mouse.rs`.
- [x] **Triple-click line** - triple-click selects the clicked line. X11: `real_x11_mouse.rs`.
- [x] **Quad-click paragraph** - quad-click selects the paragraph under the pointer. X11: `real_x11_mouse.rs`.
- [x] **Shift-click extends** - shift-click extends from the existing caret/anchor to the clicked position. X11: `real_x11_mouse.rs`.
- [x] **Column / block selection** - a rectangular selection gesture creates one selection or cursor per touched line. X11: `real_x11_multi_cursor_spec.rs`.
- [x] **Select all** - `Ctrl-A` selects the full buffer, and typing replaces it. X11: `real_x11_modifiers.rs`.
- [ ] **Expand selection to enclosing scope** - smart selection expands to syntactic or textual enclosing scopes.
- [~] **Multi-cursor / multi-selection** - see the dedicated section below.
- [x] **Select line / select paragraph** - keyboard, Vim, and mouse workflows can select whole lines and paragraphs.

## Multi-Cursor & Multi-Selection

Target behavior is VS Code-grade multi-cursor editing: users can create multiple
cursors or selections, move them independently, edit all of them as one action,
and observe all cursors/selections consistently. Tests may use the state trace
for user-visible cursor and selection positions, but not for private model
mechanics. Platform-specific shortcuts target VS Code's default Linux behavior:
Alt-click for mouse cursors, `Shift-Alt-Up/Down` for cursor above/below,
`Ctrl-D`, `Ctrl-K Ctrl-D`, `Ctrl-U`, `Shift-Alt-I`, `Ctrl-Shift-L`, `Ctrl-F2`,
and `Alt-Enter` in the find control.

X11 coverage is split deliberately: common multi-cursor workflows live in
`real_x11_multi_cursor.rs`, mouse-driven cursor behavior lives in
`real_x11_mouse.rs`, and edge-case multi-cursor specifications live in
`real_x11_multi_cursor_spec.rs`. Accepted green specs run in the blocking `x11`
profile. The green `real_x11_multi_cursor_tdd.rs` suite is also included in the
blocking profile and remains TDD-named only as a historical marker. Vim-mode
multi-cursor policy is intentionally separate.

### Cursor Set Behavior

- [x] **Coherent cursor set** - after each user action, cursors remain ordered, non-overlapping, and never empty from the user's perspective.
- [x] **Atomic multi-cursor edit** - one multi-cursor text action applies to all active cursors together, with no intermediate partial result visible.
- [x] **Stable primary cursor** - the primary cursor remains the reference for user-visible operations that need one primary target.
- [x] **Per-cursor goal column** - each cursor remembers its own intended column across vertical motion. X11: `real_x11_motion.rs`.
- [~] **Per-cursor anchor/head direction** - non-empty selections expose separate anchors and heads, but direction survival after edits is not complete.
- [x] **Single undo step per multi-cursor op** - undo restores the text and visible cursor set from before the multi-cursor action.

### Cursor Creation Gestures

- [x] **Alt-click toggles cursor** - Alt-click adds a cursor at the clicked text position, and Alt-clicking an existing cursor removes it without emptying the set. X11: `real_x11_mouse.rs`.
- [x] **Shift-Alt-Up / Shift-Alt-Down** - Linux default VS Code gesture adds adjacent-line cursors at the active visual column, clamps on short lines, and stops at document boundaries without duplicates. X11: `real_x11_multi_cursor.rs`, `real_x11_multi_cursor_spec.rs`.
- [x] **Ctrl-D adds next occurrence** - selects the current word/selection if needed, then adds the next occurrence on repeated presses. X11: `real_x11_multi_cursor.rs`, `real_x11_multi_cursor_spec.rs`.
- [x] **Ctrl-K Ctrl-D skips current match** - skips the current occurrence and adds the next one. X11: `real_x11_multi_cursor.rs`, `real_x11_chord_hold.rs`.
- [x] **Ctrl-Shift-L selects all occurrences** - creates one selection per occurrence of the current word/selection. X11: `real_x11_multi_cursor.rs`, `real_x11_multi_cursor_spec.rs`.
- [x] **Ctrl-F2 selects all occurrences of current word** - creates one selection per occurrence of the word under the cursor without requiring an existing selection. X11: `real_x11_multi_cursor.rs`.
- [x] **Shift-Alt-I adds line-end cursors** - adds a cursor at the end of every selected line. X11: `real_x11_multi_cursor.rs`.
- [x] **Ctrl-U pops last-added cursor** - removes the most recently added cursor from the current multi-cursor set. X11: `real_x11_multi_cursor.rs`.
- [x] **Esc collapses in stages** - first `Esc` collapses non-empty selections to cursors, second `Esc` drops secondary cursors. X11: `real_x11_multi_cursor.rs`.
- [x] **Plain click collapses** - an unmodified click leaves a single cursor at the clicked position.

### Per-Cursor Movement

- [x] **Horizontal motions per cursor** - character, word, line-boundary, and smart-home motions move every cursor independently. X11: `real_x11_multi_cursor.rs`, `real_x11_multi_cursor_spec.rs`, `real_x11_multi_cursor_tdd.rs`.
- [~] **Vertical motions per cursor** - line movement and duplicate-target coalescing are covered; page, half-page, and document-edge multi-cursor policy remains open. X11: `real_x11_motion.rs`, `real_x11_multi_cursor_tdd.rs`.
- [~] **Shift-extend per cursor** - shift-modified character, word, home, and end motions extend each cursor's selection independently; page/document shifted multi-cursor motion remains open. X11: `real_x11_multi_cursor_spec.rs`, `real_x11_multi_cursor_tdd.rs`.
- [~] **Shift-Alt-Right / Shift-Alt-Left smart expand/shrink per cursor** - smart selection applies to every cursor for the covered textual pair cases; richer syntax-aware expansion remains open. X11: `real_x11_multi_cursor_spec.rs`.

### Per-Cursor Editing

- [x] **Literal text insert** - typed text applies at every cursor. X11: `real_x11_multi_cursor.rs`.
- [x] **Backspace / delete-forward** - deletion applies at every cursor, including line-boundary joins. X11: `real_x11_multi_cursor.rs`, `real_x11_multi_cursor_tdd.rs`.
- [x] **Word delete** - word deletion applies at every cursor with VS Code-style full-word, whitespace, separator, line-boundary, and undo semantics. X11: `real_x11_multi_cursor_tdd.rs`.
- [x] **Auto-pair brackets / quotes** - pair insertion applies at every cursor. X11: `real_x11_multi_cursor_tdd.rs`.
- [x] **Auto-pair surround** - typing an opener around multiple non-empty selections wraps every selection. X11: `real_x11_multi_cursor_tdd.rs`.
- [x] **Auto-dedent on close bracket** - close-bracket dedent applies at every cursor where applicable.
- [x] **Overtype mode** - overtype applies at every cursor.
- [x] **Smart Enter / auto-indent** - Enter inserts a correctly indented line at every cursor. X11: `real_x11_multi_cursor.rs`.
- [x] **Indent / outdent coalesces by line** - multiple cursors on one line indent/outdent that line once through covered commands, including `Shift-Tab`. X11: `real_x11_multi_cursor_spec.rs`, `real_x11_multi_cursor_tdd.rs`.
- [x] **Toggle line / block comment coalesces by line** - multiple cursors on one line toggle that line once. X11: `real_x11_multi_cursor_spec.rs`.
- [x] **Move line up / down coalesces clusters** - adjacent cursor-bearing line groups move as stable clusters. X11: `real_x11_multi_cursor_spec.rs`.
- [x] **Duplicate line / selection applies per cursor** - duplicate affects every cursor line or selection once. X11: `real_x11_multi_cursor.rs`, `real_x11_multi_cursor_spec.rs`.
- [x] **Delete line coalesces by line** - multiple cursors on one line delete that line once. X11: `real_x11_multi_cursor_spec.rs`.
- [ ] **Join lines applies per cursor cluster** - join handles adjacent cursor groups without double edits. Vim-mode multi-cursor join behavior is out of scope until the Vim multi-cursor policy is decided.
- [ ] **Transpose / case conversion / sort applies per cursor** - text transformations operate per cursor or per selection.
- [ ] **Snippet tabstops** - snippets create one cursor per tabstop and Tab advances tabstops in lockstep.

### Clipboard Semantics

- [x] **Paste broadcasts single fragment** - a one-fragment clipboard is inserted at every cursor; mismatched multiline clipboards broadcast the whole text at every cursor. X11: `real_x11_multi_cursor_spec.rs`, `real_x11_multi_cursor_tdd.rs`.
- [x] **Paste distributes matching line count** - if a multiline clipboard has exactly one line per cursor, each cursor receives its corresponding line. X11: `real_x11_multi_cursor.rs`, `real_x11_multi_cursor_tdd.rs`.
- [x] **Cut / copy collects per cursor** - selected fragments are collected in document order and joined by newlines. X11: `real_x11_multi_cursor.rs`, `real_x11_multi_cursor_spec.rs`.
- [x] **Copy-paste round-trip identity** - copying from multiple selections and pasting back into the same cursor set reproduces the selected layout.

### Search & Replace Integration

- [ ] **Find scope honors multi-selection** - find-in-selection searches all active selections, not only one selection.
- [ ] **Replace-all in selection honors multi-selection** - replace-all affects matches inside every active selection.
- [x] **Alt-Enter selects all current find matches** - from the find control, `Alt-Enter` creates one selection for every current match. X11: `real_x11_multi_cursor.rs`.
- [x] **Cursor-add gestures read find flags** - occurrence selection respects case, whole-word, and smart-case find settings.

### Mouse Gestures

- [x] **Alt-click toggle** - add/remove cursor at a clicked text position. X11: `real_x11_mouse.rs`.
- [~] **Alt-drag additive selection** - additive free-form range selection is not complete.
- [x] **Shift-Alt-drag column selection** - creates a rectangular cursor/selection set. X11: `real_x11_multi_cursor_spec.rs`.
- [ ] **Middle-click drag column selection** - optional platform-dependent column selection gesture.
- [ ] **Drag extends only target cursor** - dragging one multi-cursor selection leaves the others intact.

### Visual Feedback

- [x] **All selections painted** - every active selection is visibly highlighted.
- [x] **All cursors painted** - every active cursor is visible.
- [ ] **All cursors blink in phase** - cursor blink state is synchronized across cursors.
- [ ] **Primary-cursor distinction** - the primary cursor has a subtle visible distinction.
- [~] **Reveal targets primary or last-moved cursor** - movement keeps the relevant cursor visible, but last-moved behavior is not complete.
- [x] **Off-screen cursor indicator** - UI indicates when cursors exist outside the viewport. X11: `real_x11_off_screen_cursor_tdd.rs`.
- [x] **Gutter marker for cursor-bearing lines** - line-number gutter reflects all lines with active cursors.
- [x] **Status-bar metrics** - status reports multi-cursor counts and selection totals. X11: `real_x11_state_trace.rs`.

### Column / Block Selection

- [x] **Shift-Alt-drag creates rectangular selection** - rectangular selection creates the expected per-line cursor set. X11: `real_x11_multi_cursor_spec.rs`.
- [ ] **Keyboard column selection policy** - VS Code documents column-selection commands but no Linux default shortcut; decide whether `lst` exposes an explicit Linux binding.
- [x] **Defined short-line policy** - short lines clamp to their line end and duplicate cursor positions coalesce.
- [x] **Insert in column mode** - insertion applies at every column-aligned cursor. X11: `real_x11_multi_cursor_spec.rs`.
- [x] **Backspace in column mode** - deletion applies at every column-aligned cursor. X11: `real_x11_multi_cursor_spec.rs`.

### Advanced Interaction

- [x] **IME composition routes to primary only** - composition affects only the primary cursor while secondary cursors stay inert.
- [ ] **Macro recording policy defined** - decide whether macros are single-cursor only or record a whole multi-cursor operation.
- [ ] **Language service responsiveness under multi-cursor** - diagnostics, completions, and highlighting remain stable and responsive with multiple cursors once language services exist.
- [~] **Smooth ordinary multi-cursor rendering** - ordinary cursor counts render without visible stutter, but large-count behavior is not fully specified.
- [ ] **Responsive at 1k cursors** - large cursor counts stay responsive or are capped/warned intentionally.

## Editing Primitives

- [x] **Undo/redo with coalesced typing** - related typed characters undo as a sensible group. X11: `real_x11_modifiers.rs`.
- [x] **Redo branch preservation** - alternate redo branches remain reachable after undoing and making another edit.
- [x] **Backspace at start of line joins previous line** - Backspace at column 0 removes the line break.
- [x] **Delete at end of line joins next line** - Delete at line end removes the following line break.
- [x] **Smart indent on Enter** - Enter preserves the active line's indentation.
- [x] **Modified Enter inserts newline in insert mode** - Ctrl-Enter, Alt-Enter, and Shift-Enter all insert a newline when typing (insert mode), so chorded shortcuts from other apps don't silently disappear. X11: `real_x11_modifiers.rs`.
- [x] **Auto-dedent on close bracket** - typing a closer on a whitespace-only line dedents when the language expects it. X11: `real_x11_language.rs`.
- [x] **Indent/outdent selection** - Tab and Shift-Tab indent or outdent all touched lines; Vim indent commands follow the same user-visible behavior. X11: `real_x11_language.rs`, `real_x11_vim.rs`.
- [x] **Move line up/down** - line move commands swap the current line or selected block with neighboring lines.
- [x] **Duplicate line/selection** - duplicate command duplicates the selected text, or the active line if there is no selection. X11: `real_x11_multi_cursor.rs`.
- [x] **Delete line** - delete-line removes the active line or selected line block.
- [x] **Join lines with single-space collapse** - join removes line breaks and collapses surrounding whitespace appropriately.
- [x] **Transpose** - transpose adjacent characters. X11: `real_x11_transpose_tdd.rs`.
- [x] **Toggle comment line/block** - line and block comments toggle according to the active language. X11: `real_x11_language.rs`, `real_x11_multi_cursor_spec.rs`.
- [x] **Auto-pair brackets/quotes** - typing openers inserts matching closers; typing an existing closer steps over it. X11: `real_x11_language.rs`, `real_x11_multi_cursor_tdd.rs`.

## Clipboard

- [x] **Cut/copy/paste with platform clipboard** - normal clipboard copy, cut, and paste work. X11: `real_x11_workflows.rs`, `real_x11_chord_hold.rs`.
- [x] **Write primary selection** - selected text is available through X11 PRIMARY and middle-click paste works. X11: `real_x11_smoke.rs`, `real_x11_mouse.rs`.
- [x] **Cut/copy whole line when no selection** - copy/cut without a selection uses the current line.
- [ ] **Paste preserves/normalizes indentation** - pasted blocks adapt indentation when appropriate.
- [ ] **Clipboard history / kill ring** - prior clipboard entries can be recalled.
- [ ] **Bracketed paste** - not applicable to the GUI path unless a terminal backend is added.

## Search & Replace

- [x] **Incremental find** - typing in the find panel updates matches live. X11: `real_x11_state_trace.rs`.
- [x] **Find next/previous** - navigation wraps through all matches.
- [x] **Case sensitivity + smart case** - find can be forced case-sensitive, otherwise uppercase queries imply case-sensitive behavior.
- [x] **Whole word toggle** - find can restrict matches to whole words.
- [x] **Regex toggle with capture groups** - find supports regex queries and replacement capture references.
- [x] **Find in selection scope** - find can restrict matches to the active selection.
- [x] **Replace / replace all** - single replace and replace-all update matches predictably. X11: `real_x11_find.rs`, `real_x11_replace_scope_tdd.rs`.
- [x] **Wrap-around at end** - find navigation wraps at document edges.
- [x] **Highlight all matches** - all matches remain visibly highlighted while find is active.
- [x] **Star search** - Vim `*` searches for the word under the cursor.

## Text Input

- [x] **IME composition** - marked text composition, replacement, and unmarking work as a text input flow.
- [x] **Unicode grapheme clusters** - motion, selection, word behavior, search, and wrapping treat grapheme clusters as indivisible user-visible characters. X11: `real_x11_text_input.rs`.
- [x] **Tab to spaces with soft-tab backspace** - Tab inserts the language's indentation unit; Backspace in leading indentation removes one indentation unit when appropriate. X11: `real_x11_language.rs`.
- [~] **Trim trailing whitespace on save** - save can remove trailing whitespace through the current env-gated option; no user settings UI yet. X11: `real_x11_save_options_tdd.rs`.
- [~] **Ensure final newline on save** - save can ensure a final newline through the current env-gated option; no user settings UI yet. X11: `real_x11_save_options_tdd.rs`.
- [x] **Detect/preserve line endings** - files preserve their newline style when saved.
- [ ] **Detect/preserve encoding** - file encoding detection and preservation are not available.

## Rendering & Viewport

- [x] **Soft wrap** - long logical lines wrap visually and cursor movement respects visual rows.
- [x] **Visual vs logical line motion** - the editor distinguishes visual-row and logical-line movement.
- [x] **Line numbers** - absolute, relative, and hybrid line-number modes are available. X11: `real_x11_chrome.rs`.
- [x] **Capacity-aware gutter** - the gutter reserves three digits, grows and shrinks at decimal line-count boundaries, shares the editor background, and emphasizes cursor-bearing line numbers. X11: `real_x11_chrome.rs`; visual: `real_x11_visual.rs`.
- [x] **Zoom controls** - keyboard zoom in/out/reset updates the visible status bar and returns to the default size. X11: `real_x11_chrome.rs`.
- [x] **Theme toggle** - the visible theme control cycles the active theme label. X11: `real_x11_chrome.rs`.
- [x] **Syntax highlighting** - tree-sitter highlighting for Rust, Python, JavaScript/JSX, TypeScript/TSX, JSON, TOML, YAML, Markdown, HTML, and CSS, with incremental reparsing, language injection (e.g. Markdown fenced code), and theme-driven colors. Covered by in-crate parser tests (`apps/lst-gpui/src/syntax`); no real-display color coverage yet.
- [ ] **Ruler / column guides** - visible column guides can be shown.
- [x] **Current line highlight** - the cursor line is visibly highlighted.
- [x] **Identifier occurrence highlight** - editor focus or an explicit caret move onto either edge or the interior of an identifier highlights exact, case-sensitive, Unicode whole-identifier occurrences around painted character windows. Editing clears the passive highlights and does not infer a new query from the post-edit caret; a later focus or caret move retriggers them. Adjacent wrapped windows are merged before identifier-boundary expansion, so an extreme identifier is traversed once rather than once per row. X11: `real_x11_chrome.rs`; visual: `real_x11_visual.rs`.
- [x] **Cursor blink** - all visible carets share the configured editor blink state.
- [x] **Scroll margin** - vertical and horizontal cursor reveal keep margin around the cursor.
- [x] **Visible scrollbar when content overflows** - scrollbars appear and can be used when content overflows.
- [x] **Horizontal scroll on long lines** - with soft wrap off, horizontal scrolling keeps the cursor visible.
- [ ] **Minimap** - a minimap is available for large files.
- [ ] **Indent guides** - indentation structure can be displayed.

## File & Buffer

- [x] **Dirty indicator** - modified buffers show a visible dirty state. X11: `real_x11_state_trace.rs`.
- [x] **Clean save is state-preserving** - `Ctrl-S` on an unmodified buffer leaves dirty state, cursor position, and visible selection state unchanged. X11: `real_x11_state_trace.rs`.
- [x] **External-change resolution** - clean buffers reload in place; dirty buffers show a non-modal per-tab Reload / Keep Mine / Save As / Dismiss banner. X11: `real_x11_workflows.rs`.
- [x] **Auto-save** - scratchpad and autosave workflows persist edits. X11: `real_x11_smoke.rs`, `real_x11_workflows.rs`.
- [ ] **Recover from crash via swap/journal** - unsaved work can be recovered after a crash.
- [x] **Multiple tabs/buffers** - users can open, close, activate, reorder, and reach overflowed buffers through the all-tabs list.
- [x] **Recently closed reopen** - recently closed buffers can be reopened with caret position restored. X11: `real_x11_recently_closed_tdd.rs`.
- [x] **Filetype / language detection and override** - common languages are detected for syntax and editor behavior, with a keyboard-operable language override menu. X11: `real_x11_language.rs`.

## Accessibility & Input

- [ ] **Screen reader support** - accessibility integration is not available.
- [x] **Keyboard-only operation** - editor actions are reachable from the keyboard and Vim state machine.
- [~] **Configurable keybindings** - versioned TOML overrides reload live and Settings exposes searchable bindings/conflicts; direct binding capture/editing remains future work.
- [ ] **Respect OS text settings** - double-click word separators and repeat-rate preferences are not fully integrated.

## AI Assistance

- [x] **LLM text cleanup** - the command-palette action rewrites the current selection through DeepSeek; an unselected whole document requires an explicit in-app data-sharing confirmation. A single undo restores the original. Requires `DEEPSEEK_API_KEY` (model overridable via `DEEPSEEK_MODEL`). X11: `real_x11_llm_cleanup.rs` (with an in-process fake client).

---

## Commonly Missed Fundamentals

- [x] Sticky virtual column across vertical motion
- [x] Smart Home two-stage behavior
- [x] Grapheme-aware motion, selection, search, and wrap
- [x] Undo coalescing by typing group
- [x] Scroll margin
- [x] Auto-scroll during drag selection
- [x] IME composition
- [x] Current-line highlight

---

## Summary

- **Done:** 132
- **Partial:** 13
- **Missing:** 26

**Strong foundation:** Vim editing workflows, viewport motion and scroll
margin, soft wrap, undo/redo, autosave, find/replace, mouse selection,
clipboard and PRIMARY, IME composition, gutter modes, current-line highlight,
line-ending preservation, grapheme-aware text behavior, scrollbars, horizontal
scrolling, keyboard-driven buffer workflows, theme/zoom chrome, tree-sitter
syntax highlighting, AI text cleanup, recently closed tabs, save-option hooks,
and the real-display X11 suite.

**Biggest gaps to close for idiomatic behavior:**

1. Multi-cursor policy gaps: Vim-mode behavior, page/document-edge movement, join-line clusters, and find/replace over multiple selections.
2. Selection polish: syntax-aware expand selection, additive drag selection, target-cursor drag behavior, and optional keyboard column selection.
3. Visual polish: primary-cursor distinction, ruler/indent guides, minimap, and large-cursor-count responsiveness.
4. Jump list / navigation history.
5. User-configurable keybindings and user-facing language override UI.
6. Paste indentation, clipboard history, encoding preservation, and crash recovery.
