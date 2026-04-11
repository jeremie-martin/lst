# Roadmap

Current direction: keep `lst` minimal, fast, and production-grade. The GPUI
rewrite should prioritize reliable editing behavior and simple ownership over
feature volume.

## 1. Interaction Polish And Correctness

Focus areas:

- mouse selection: word/line selection, shift-click, and drag autoscroll
- keyboard selection: modifier combinations, page movement, and line boundaries
- focus behavior: find/replace/goto inputs, tab switching, and command routing
- undo grouping: typing, paste, line operations, replace, and Vim commands
- tab flows: new tab, close tab, dirty tab handling, and active tab preservation

Success criteria:

- common editor gestures work predictably
- regressions have focused behavioral tests
- mixed mouse/keyboard use does not require special-case reasoning

## 2. Codebase Shape

The active split is:

- `lst-editor`: framework-neutral editor model, document primitives, effects, and Vim state
- `lst-gpui`: GPUI rendering, widgets, input adaptation, runtime effects, and desktop integration

Focus areas:

- keep model mutation behind direct `EditorModel` APIs
- keep clipboard, files, dialogs, focus, and rendering at the GPUI boundary
- split large files only by real behavior responsibility
- avoid new traits or packages unless they remove production complexity

Success criteria:

- tests exercise real production paths through model APIs, effects, and snapshots
- modules map to behavior boundaries
- app code cannot mutate editor state through hidden side doors

## 3. File And Workspace UX

Focus areas:

- external file changes and reload decisions
- clear unsaved-close/save prompts
- better save/save-as status and error reporting
- lightweight reopen flow if needed

Success criteria:

- the editor never silently loses user data
- file errors are visible and actionable
- new UI stays minimal

## 4. Syntax Highlighting

The current production path is tree-sitter highlighting in the GPUI app.

Focus areas:

- keep highlighting off the critical editing path
- improve cache invalidation only when behavior or responsiveness requires it
- preserve broad language support unless there is a clear maintenance cost

Success criteria:

- typing, paste, scroll, and paint remain responsive
- highlighting failures degrade to plain text rather than breaking editing
