# TODO

## Must-have

- [ ] Find & Replace (Ctrl+F / Ctrl+H)
- [ ] Verify undo/redo works (Ctrl+Z / Ctrl+Shift+Z) — iced's text_editor may handle this already
- [ ] Unsaved changes warning on close tab / quit
- [ ] Drag-and-drop file open
- [ ] Derive line height from font metrics instead of hardcoded `20.0` in gutter click

## Should-have

- [ ] File change detection — warn if modified externally
- [ ] Tab reordering via drag
- [ ] Go to line (Ctrl+G)
- [ ] Word wrap toggle
- [ ] Indent / unindent selection (Tab / Shift+Tab)
- [ ] Auto-indent on Enter
- [ ] Large file handling — virtual scrolling for line numbers (currently O(n) per frame)

## Nice ideas

- [ ] Zen mode — Ctrl+Shift+Enter hides tabs and status bar
- [ ] Session restore — remember open tabs and cursor positions across launches
- [ ] Smooth scroll animation
- [ ] Syntax highlighting for more languages (detect from file extension, swap highlighter)
- [ ] Command palette (Ctrl+Shift+P) — fuzzy-search popup for all actions
- [ ] Minimap column showing document structure
