# Roadmap

Current direction: keep `lst` minimal, fast, and production-grade. The GPUI rewrite
is promising, but the next work should focus on trust, responsiveness, and simple
architecture rather than feature volume.

## 1. Interaction Polish And Correctness

This is the highest-priority product work. A text editor feels good only when the
basic interactions are boringly reliable.

Focus areas:

- Mouse selection: double-click, triple-click, drag after word/line selection, shift-click, scroll while selecting.
- Keyboard selection: modifier combinations, vertical selection expansion, page movement, line boundaries.
- Focus behavior: find/replace/goto inputs, tab switching, tab closing, command routing.
- Undo grouping: typing, paste, line operations, replace, Vim commands.
- Tab flows: new tab, close tab, dirty tab handling, active tab preservation.

Success criteria:

- Common editor gestures work without special-case-feeling behavior.
- Regressions have focused tests.
- UI interactions stay simple and predictable under mixed mouse/keyboard use.

## 2. GPUI Performance Benchmarks

The archived iced benchmarks are useful history, but the active GPUI editor
needs its own performance contract.

Benchmarks to add:

- Large paste completion latency.
- Typing latency on medium and large files.
- Scroll responsiveness on highlighted and unhighlighted files.
- Large file open time.
- Search/index latency.
- Syntax highlighting latency using the current production tree-sitter path.

Success criteria:

- Each benchmark has one primary metric.
- Benchmarks wait for completed work, not just dispatched input.
- Results are documented in `docs/performance-optimization.md`.

## 3. Syntax Highlighting V2

The current tree-sitter implementation is the right baseline: broad language
support, background work, revision-gated caches, and per-line spans. The next
step is reducing unnecessary work for very large files.

Focus areas:

- Incremental parsing or visible-range highlighting.
- Better Markdown handling, including inline syntax and fenced code blocks.
- Embedded-language support for HTML, CSS, JS, TSX, and Markdown code fences.
- Cache invalidation that stays revision-safe and easy to reason about.

Success criteria:

- Highlighting never blocks typing, paste, scroll, or paint.
- Large files remain responsive.
- Highlighting correctness improves without reintroducing `syntect` as the default path.

## 4. File And Workspace UX

The editor should handle file lifecycle edge cases cleanly before adding larger
workspace features.

Focus areas:

- External file changes and reload decisions.
- Clear unsaved-close/save prompts.
- Better save/save-as status and error reporting.
- Recent files or lightweight reopen flow.
- A minimal command palette if shortcuts become hard to discover.

Success criteria:

- The editor never silently loses user data.
- File errors are visible and actionable.
- New UI stays minimal and does not add toolbar clutter.

## 5. Codebase Shape

Continue simplifying only where it improves reasoning. Avoid refactoring just to
reduce line count.

Good split candidates:

- File operations and autosave.
- Editing commands.
- Search, replace, and goto.
- Mouse/selection handling.
- Background jobs.

Success criteria:

- Modules map to real behavior boundaries.
- Shared logic is reused instead of duplicated.
- Tests remain close to the behavior they protect.
