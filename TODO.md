# TODO

## Must-have

- [x] Find & Replace (Ctrl+F / Ctrl+H)
- [x] Undo/redo (Ctrl+Z / Ctrl+Shift+Z) — snapshot-based, batched by edit kind
- [ ] Unsaved changes warning on close tab / quit
- [ ] Drag-and-drop file open
- [x] Derive line height from shared constant (LINE_HEIGHT_PX) for gutter + editor sync

## Should-have

- [ ] File change detection — warn if modified externally
- [x] Tab reordering (Ctrl+Shift+PageUp/PageDown)
- [ ] Go to line (Ctrl+G)
- [x] Word wrap toggle (Alt+Z or status bar button)
- [x] Indent / unindent selection (Tab / Shift+Tab)
- [x] Auto-indent on Enter
- [ ] Large file handling — virtual scrolling for line numbers (currently O(n) per frame)

## Nice ideas

- [ ] Zen mode — Ctrl+Shift+Enter hides tabs and status bar
- [ ] Session restore — remember open tabs and cursor positions across launches
- [ ] Smooth scroll animation
- [ ] Syntax highlighting for more languages (detect from file extension, swap highlighter)
- [ ] Command palette (Ctrl+Shift+P) — fuzzy-search popup for all actions
- [ ] Minimap column showing document structure
