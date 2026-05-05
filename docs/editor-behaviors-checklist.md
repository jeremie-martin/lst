# Editor Behaviors Checklist

This is the product behavior checklist for `lst`. It describes what the editor
should do from a user's point of view. It is intentionally black-box: behavior
items must not depend on production implementation details.

Status legend: `[ ]` missing · `[~]` partial · `[x]` done

Status last refreshed: 2026-05-05 (spec-first X11 pass).

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
- [x] **Go to column** - the goto panel accepts `line:column` and clamps out-of-range values.
- [ ] **Jump list / navigation history** - jumps between meaningful prior locations.
- [x] **Last edit location** - Vim `gi` / `g;` return to the last edit location, with `gi` entering Insert.

## Selection

- [x] **Shift-motion extends selection** - shift-modified movement extends from the original anchor.
- [x] **Selection anchor preservation** - extending in either direction keeps the expected anchor and moves only the head.
- [x] **Click-drag selection with auto-scroll** - dragging selects text and continues extending while the pointer is beyond the viewport. X11: `real_x11_mouse.rs`.
- [x] **Double-click word** - double-click selects the word under the pointer. X11: `real_x11_mouse.rs`.
- [x] **Triple-click line** - triple-click selects the clicked line. X11: `real_x11_mouse.rs`.
- [x] **Quad-click paragraph** - quad-click selects the paragraph under the pointer. X11: `real_x11_mouse.rs`.
- [ ] **Shift-click extends** - shift-click extends from the existing caret/anchor to the clicked position. X11 TDD: `real_x11_mouse.rs`.
- [ ] **Column / block selection** - a rectangular selection gesture creates one selection or cursor per touched line.
- [x] **Select all** - `Ctrl-A` selects the full buffer, and typing replaces it. X11: `real_x11_modifiers.rs`.
- [ ] **Expand selection to enclosing scope** - smart selection expands to syntactic or textual enclosing scopes.
- [~] **Multi-cursor / multi-selection** - see the dedicated section below.
- [x] **Select line / select paragraph** - keyboard, Vim, and mouse workflows can select whole lines and paragraphs.

## Multi-Cursor & Multi-Selection

Target behavior is VS Code-grade multi-cursor editing: users can create multiple
cursors or selections, move them independently, edit all of them as one action,
and observe all cursors/selections consistently. Tests may use the state trace
for user-visible cursor and selection positions, but not for private model
mechanics.

### Cursor Set Behavior

- [x] **Coherent cursor set** - after each user action, cursors remain ordered, non-overlapping, and never empty from the user's perspective.
- [x] **Atomic multi-cursor edit** - one multi-cursor text action applies to all active cursors together, with no intermediate partial result visible.
- [x] **Stable primary cursor** - the primary cursor remains the reference for user-visible operations that need one primary target.
- [ ] **Per-cursor goal column** - each cursor remembers its own intended column across vertical motion. X11 TDD: `real_x11_motion.rs`.
- [~] **Per-cursor anchor/head direction** - non-empty selections expose separate anchors and heads, but direction survival after edits is not complete.
- [x] **Single undo step per multi-cursor op** - undo restores the text and visible cursor set from before the multi-cursor action.

### Cursor Creation Gestures

- [ ] **Alt-click toggles cursor** - Alt-click adds a cursor at the clicked text position, and Alt-clicking an existing cursor removes it without emptying the set. X11 TDD: `real_x11_mouse.rs`.
- [x] **Ctrl-Alt-Up / Ctrl-Alt-Down** - adds adjacent-line cursors at the active visual column. X11: `real_x11_multi_cursor.rs`.
- [x] **Ctrl-D adds next occurrence** - selects the current word/selection if needed, then adds the next occurrence on repeated presses. X11: `real_x11_multi_cursor.rs`.
- [ ] **Ctrl-K Ctrl-D skips current match** - skips the current occurrence and adds the next one. X11 TDD: `real_x11_multi_cursor.rs`, `real_x11_chord_hold.rs`.
- [x] **Ctrl-Shift-L selects all occurrences** - creates one selection per occurrence of the current word/selection. X11: `real_x11_multi_cursor.rs`.
- [~] **Select all occurrences of word under cursor** - file-wide word selection works through existing occurrence selection, but there is no dedicated word-occurrence binding.
- [ ] **Shift-Alt-I adds line-end cursors** - adds a cursor at the end of every selected line. X11 TDD: `real_x11_multi_cursor.rs`.
- [ ] **Ctrl-U pops last-added cursor** - removes the most recently added cursor from the current multi-cursor set. X11 TDD: `real_x11_multi_cursor.rs`.
- [x] **Esc collapses in stages** - first `Esc` collapses non-empty selections to cursors, second `Esc` drops secondary cursors. X11: `real_x11_multi_cursor.rs`.
- [x] **Plain click collapses** - an unmodified click leaves a single cursor at the clicked position.

### Per-Cursor Movement

- [ ] **Horizontal motions per cursor** - character, word, subword, line-boundary, and smart-home motions move every cursor independently. X11 TDD: `real_x11_multi_cursor.rs`.
- [ ] **Vertical motions per cursor** - line, page, half-page, and document-edge motions move every cursor independently.
- [ ] **Shift-extend per cursor** - shift-modified motion extends each cursor's selection independently.
- [ ] **Smart-expand / smart-shrink per cursor** - smart selection applies to every cursor once smart selection exists.

### Per-Cursor Editing

- [x] **Literal text insert** - typed text applies at every cursor. X11: `real_x11_multi_cursor.rs`.
- [x] **Backspace / delete-forward** - deletion applies at every cursor. X11: `real_x11_multi_cursor.rs`.
- [x] **Word delete** - word deletion applies at every cursor.
- [x] **Auto-pair brackets / quotes** - pair insertion applies at every cursor.
- [x] **Auto-pair surround** - typing an opener around multiple non-empty selections wraps every selection.
- [x] **Auto-dedent on close bracket** - close-bracket dedent applies at every cursor where applicable.
- [x] **Overtype mode** - overtype applies at every cursor.
- [x] **Smart Enter / auto-indent** - Enter inserts a correctly indented line at every cursor. X11: `real_x11_multi_cursor.rs`.
- [ ] **Indent / outdent coalesces by line** - multiple cursors on one line indent that line once.
- [ ] **Toggle line / block comment coalesces by line** - multiple cursors on one line toggle that line once.
- [ ] **Move line up / down coalesces clusters** - adjacent cursor-bearing line groups move as stable clusters.
- [ ] **Duplicate line / selection applies per cursor** - duplicate affects every cursor line or selection once. X11 TDD: `real_x11_multi_cursor.rs`.
- [ ] **Delete line coalesces by line** - multiple cursors on one line delete that line once.
- [ ] **Join lines applies per cursor cluster** - join handles adjacent cursor groups without double edits.
- [ ] **Transpose / case conversion / sort applies per cursor** - text transformations operate per cursor or per selection.
- [ ] **Snippet tabstops** - snippets create one cursor per tabstop and Tab advances tabstops in lockstep.

### Clipboard Semantics

- [x] **Paste broadcasts single fragment** - a one-fragment clipboard is inserted at every cursor.
- [x] **Paste distributes matching line count** - if the clipboard has exactly one line per cursor, each cursor receives its corresponding line. X11: `real_x11_multi_cursor.rs`.
- [x] **Cut / copy collects per cursor** - selected fragments are collected in document order and joined by newlines. X11: `real_x11_multi_cursor.rs`.
- [x] **Copy-paste round-trip identity** - copying from multiple selections and pasting back into the same cursor set reproduces the selected layout.

### Search & Replace Integration

- [ ] **Find scope honors multi-selection** - find-in-selection searches all active selections, not only one selection.
- [ ] **Replace-all in selection honors multi-selection** - replace-all affects matches inside every active selection.
- [x] **Cursor-add gestures read find flags** - occurrence selection respects case, whole-word, and smart-case find settings.

### Mouse Gestures

- [ ] **Alt-click toggle** - add/remove cursor at a clicked text position. X11 TDD: `real_x11_mouse.rs`.
- [~] **Alt-drag additive selection** - additive free-form range selection is not complete.
- [ ] **Shift-Alt-drag column selection** - creates a rectangular cursor/selection set. X11 TDD target.
- [ ] **Middle-click drag column selection** - optional platform-dependent column selection gesture.
- [ ] **Drag extends only target cursor** - dragging one multi-cursor selection leaves the others intact.

### Visual Feedback

- [x] **All selections painted** - every active selection is visibly highlighted.
- [x] **All cursors painted** - every active cursor is visible.
- [ ] **All cursors blink in phase** - cursor blink state is synchronized across cursors.
- [ ] **Primary-cursor distinction** - the primary cursor has a subtle visible distinction.
- [~] **Reveal targets primary or last-moved cursor** - movement keeps the relevant cursor visible, but last-moved behavior is not complete.
- [ ] **Off-screen cursor indicator** - UI indicates when cursors exist outside the viewport.
- [x] **Gutter marker for cursor-bearing lines** - line-number gutter reflects all lines with active cursors.
- [x] **Status-bar metrics** - status reports multi-cursor counts and selection totals. X11: `real_x11_state_trace.rs`.

### Column / Block Selection

- [~] **Shift-Alt-drag creates rectangular selection** - rectangular selection exists, but the canonical gesture is not complete.
- [ ] **Ctrl-Shift-Alt-arrow extends column selection** - keyboard extends the rectangle by row or column.
- [x] **Defined short-line policy** - short lines clamp to their line end and duplicate cursor positions coalesce.
- [x] **Insert in column mode** - insertion applies at every column-aligned cursor.
- [x] **Backspace in column mode** - deletion applies at every column-aligned cursor.

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
- [x] **Auto-dedent on close bracket** - typing a closer on a whitespace-only line dedents when the language expects it.
- [x] **Indent/outdent selection** - Tab and Shift-Tab indent or outdent all touched lines; Vim indent commands follow the same user-visible behavior. X11: `real_x11_vim.rs`.
- [x] **Move line up/down** - line move commands swap the current line or selected block with neighboring lines.
- [x] **Duplicate line/selection** - duplicate command duplicates the selected text, or the active line if there is no selection. X11 TDD for multi-cursor: `real_x11_multi_cursor.rs`.
- [x] **Delete line** - delete-line removes the active line or selected line block.
- [x] **Join lines with single-space collapse** - join removes line breaks and collapses surrounding whitespace appropriately.
- [ ] **Transpose** - transpose adjacent characters or selected units.
- [x] **Toggle comment line/block** - line and block comments toggle according to the active language.
- [x] **Surround with brackets/quotes** - selected text can be surrounded with brackets or quotes; Vim surround commands work. X11: `real_x11_vim.rs`.
- [x] **Auto-pair brackets/quotes** - typing openers inserts matching closers; typing an existing closer steps over it.

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
- [x] **Replace / replace all** - single replace and replace-all update matches predictably.
- [x] **Wrap-around at end** - find navigation wraps at document edges.
- [x] **Highlight all matches** - all matches remain visibly highlighted while find is active.
- [x] **Star search** - Vim `*` searches for the word under the cursor.

## Text Input

- [x] **IME composition** - marked text composition, replacement, and unmarking work as a text input flow.
- [x] **Unicode grapheme clusters** - motion, selection, word behavior, search, and wrapping treat grapheme clusters as indivisible user-visible characters.
- [x] **Tab to spaces with soft-tab backspace** - Tab inserts the language's indentation unit; Backspace in leading indentation removes one indentation unit when appropriate.
- [ ] **Trim trailing whitespace on save** - save can remove trailing whitespace.
- [ ] **Ensure final newline on save** - save can ensure a final newline.
- [x] **Detect/preserve line endings** - files preserve their newline style when saved.
- [ ] **Detect/preserve encoding** - file encoding detection and preservation are not available.

## Rendering & Viewport

- [x] **Soft wrap** - long logical lines wrap visually and cursor movement respects visual rows.
- [x] **Visual vs logical line motion** - the editor distinguishes visual-row and logical-line movement.
- [x] **Line numbers** - absolute, relative, and hybrid line-number modes are available.
- [ ] **Ruler / column guides** - visible column guides can be shown.
- [x] **Current line highlight** - the cursor line is visibly highlighted.
- [ ] **Cursor blink** - cursor blink respects OS or editor settings.
- [x] **Scroll margin** - vertical and horizontal cursor reveal keep margin around the cursor.
- [x] **Visible scrollbar when content overflows** - scrollbars appear and can be used when content overflows.
- [x] **Horizontal scroll on long lines** - with soft wrap off, horizontal scrolling keeps the cursor visible.
- [ ] **Minimap** - a minimap is available for large files.
- [ ] **Indent guides** - indentation structure can be displayed.

## File & Buffer

- [x] **Dirty indicator** - modified buffers show a visible dirty state. X11: `real_x11_state_trace.rs`.
- [ ] **Clean save is state-preserving** - `Ctrl-S` on an unmodified buffer leaves dirty state, cursor position, and visible selection state unchanged. X11 TDD: `real_x11_state_trace.rs`.
- [x] **Reload on external change prompt** - external changes reload clean buffers or prompt on conflicts.
- [x] **Auto-save** - scratchpad and autosave workflows persist edits. X11: `real_x11_smoke.rs`, `real_x11_workflows.rs`.
- [ ] **Recover from crash via swap/journal** - unsaved work can be recovered after a crash.
- [x] **Multiple tabs/buffers** - users can open, close, activate, and reorder buffers.
- [ ] **Recently closed reopen** - recently closed buffers can be reopened.
- [~] **Filetype / language detection** - common languages are detected for syntax and editor behavior, but there is no user-facing language picker or config override UI.

## Accessibility & Input

- [ ] **Screen reader support** - accessibility integration is not available.
- [x] **Keyboard-only operation** - editor actions are reachable from the keyboard and Vim state machine.
- [~] **Configurable keybindings** - keybindings are not user-configurable yet.
- [ ] **Respect OS text settings** - double-click word separators and repeat-rate preferences are not fully integrated.

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

- **Done:** 104
- **Partial:** 9
- **Missing:** 50

**Strong foundation:** Vim editing workflows, viewport motion and scroll
margin, soft wrap, undo/redo, autosave, find/replace, mouse selection,
clipboard and PRIMARY, IME composition, gutter modes, current-line highlight,
line-ending preservation, grapheme-aware text behavior, scrollbars, horizontal
scrolling, keyboard-driven buffer workflows, and the real-display X11 suite.

**Biggest gaps to close for idiomatic behavior:**

1. Multi-cursor per-cursor motion.
2. Multi-cursor line, Vim, find-scope, indent, comment, duplicate, and move-line operations.
3. Multi-cursor creation polish: Ctrl-U history, Ctrl-K Ctrl-D skip, Shift-Alt-I, and a dedicated word-occurrence binding.
4. Column/block selection on the canonical Shift-Alt-drag gesture.
5. Cursor blink respecting OS/editor settings.
6. Trim trailing whitespace and ensure final newline on save.
7. Recently closed buffer reopen.
8. Jump list / navigation history.
9. User-configurable keybindings.
10. User-facing language picker / manual language override UI.
11. Paste indentation, transpose, and clipboard history.
