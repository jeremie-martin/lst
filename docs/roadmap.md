# Roadmap

Keep `lst` minimal, fast, and production-grade. Prefer clear ownership and
observable behavior over feature volume.

## Current Architecture

- `lst-editor`: framework-neutral editor model, document primitives, text
  transactions, undo/redo snapshot history, observable snapshots, effects,
  language detection and overrides, line bookmarks, standard input mode, and
  the optional Vim state machine
- `lst-gpui`: rendering, widgets, input adaptation, dialogs, clipboard, file
  I/O, tree-sitter syntax highlighting (`src/syntax`), DeepSeek text cleanup
  (`src/llm.rs`), benchmark wiring, and desktop integration

Editor-domain behavior should live in `lst-editor`; accepted product behavior
should be specified through the real GPUI app under the X11 harness whenever it
can be driven there. GPUI should adapt desktop events to editor contracts and
render observable state.

## Near-Term Priorities

The daily-driver foundation has landed: standard mode is the default, ordinary
files are manually saved, settings and keybindings are persistent, command and
application menus make behavior discoverable, and find/replace and language
controls are visible. Remaining near-term work is:

- Promote the `x11-tdd` specs that are already wired (line bookmarks, recently
  closed tab reopen) into the blocking `x11` profile, and add real-display
  coverage for syntax-highlighting colors
- Jump list and navigation history
- Direct keybinding editing in the settings surface; TOML overrides and live
  reload are already supported
- Extend tab dragging and context-menu coverage in the real-display harness
- Multi-cursor policy gaps: Vim-mode multi-cursor, join-line clusters, and
  find/replace over multiple selections

## Codebase Shape

- Keep model mutation behind explicit `EditorModel` APIs.
- Keep text mutation behind `EditRequest` / `TextChangeSet` transactions and
  `EditHistory` snapshot boundaries; focused request builders such as `text_input`,
  `multi_selection`, and `line_edit` should return transactions
  instead of mutating `EditorModel` directly. Multi-change batches must choose
  their primary change explicitly through `TextChangeSet::new`.
- Keep clipboard, filesystem, dialogs, focus, and rendering at the GPUI boundary.
- Split modules by real behavior responsibility, not by speculative layering.
- Avoid new traits or crates unless they remove production complexity.

## Quality Gates

- `DISPLAY=:0 cargo nextest run --profile x11 -p lst-gpui --tests --run-ignored only`
  is the canonical accepted-behavior gate.
- `cargo test` and `cargo test --all-features` are fast non-X11 sanity checks,
  not the source of truth for user-visible editor behavior.
- `cargo test -p lst-editor --features internal-invariants` covers optional
  private invariants when a refactor touches those internals.
- Performance work should use one benchmark scenario and one primary metric at a
  time, as described in `docs/performance-optimization.md`.
- UI work follows the layout, typography, color, rendering, and review contract
  in `docs/ui-quality.md`.
