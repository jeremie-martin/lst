# Non-X11 Test Inventory

This inventory explains the remaining non-X11 tests after pruning the legacy
GPUI app snapshot suite. The goal is to keep `cargo test` as fast compile and
domain feedback, not as a second behavior-spec lane beside real-display tests.

Pure invariant tests are allowed exceptions, not a preferred alternative to
X11. When a feature can be driven through the real app, the source-side test
should be deleted or reduced to the smallest representation invariant that X11
cannot express.

## Removed Or Ported

- `apps/lst-gpui/src/tests.rs` was deleted. It mixed GPUI snapshots, direct
  model mutation, keybinding introspection, rendering internals, and product
  behavior in one large white-box suite.
- Recent-panel behavior formerly covered through app snapshots now has X11
  specs:
  - query can find entries beyond the initial visible batch
  - query text is preserved across close and reopen
- Viewport behavior formerly covered through GPUI internals now has X11 specs:
  - find/goto overlays do not resize the text viewport
  - `Alt+Z` no-wrap reveals a far-right cursor, and wrapping back on clears
    horizontal scroll
  - typing at the end of a long wrapped line keeps the cursor visible
- Find behavior formerly covered through `FindState` internals now has X11
  specs:
  - lowercase queries use smart-case matching
  - uppercase queries are case-sensitive
  - submitting an active query advances to the next match
- Small contracts that still earn their keep were moved to owned modules:
  - launch argument parsing lives in `apps/lst-gpui/src/launch.rs`
  - syntax highlighting contracts live in `apps/lst-gpui/src/syntax/mod.rs`
  - UTF-16/char range conversion lives in `apps/lst-gpui/src/input.rs`
- Runtime save tests for executable-mode preservation, symlink saves, and
  failed safe-save preservation were removed because the same user-visible
  behavior is already covered by `apps/lst-gpui/tests/real_x11_regressions.rs`.
- Recent-panel query preservation and input-field keybinding registration unit
  tests were removed in favor of X11 delivery tests that exercise the actual
  shortcuts and panel focus path.

## Remaining Non-X11 Tests

- `crates/lst-editor/src/{document,selection,tab_set,transaction,wrap}.rs`
  contains pure editor-domain invariants: Unicode boundaries, wrapping,
  transaction validation, and small state-container behavior.
- `crates/lst-editor/src/find.rs` keeps a grapheme-boundary invariant plus
  watch-listed find-option checks until the X11 harness has stable coverage for
  option controls and non-ASCII query input. These option checks are not the
  long-term preferred home for user-facing find behavior.
- `apps/lst-gpui/src/runtime/tests.rs` covers filesystem boundary contracts
  that are awkward or brittle to force through a display: scratchpad filename
  collision handling, save/open result shapes, conflict detection, stale save
  tickets, and autosave temp-file completion.
- `apps/lst-gpui/src/recent.rs` covers recent-file persistence format,
  normalization, pruning, caps, and content-search limits. Panel behavior
  belongs in X11.
- `apps/lst-gpui/src/syntax/{mod.rs,catalog.rs}` covers tree-sitter language
  registration, injection configuration, and highlight-role contracts. Visual
  color correctness is intentionally not tested.
- `apps/lst-gpui/src/input.rs` covers UTF-16 conversion at the IME boundary.
- `apps/lst-gpui/src/ui/input_field.rs` and
  `apps/lst-gpui/src/ui/scrollbar.rs` contain widget model and geometry
  contracts, not accepted editor behavior.
- `apps/lst-gpui/src/viewport.rs` contains rendering-geometry helper contracts:
  wrap-column math, row-local search slicing, syntax style keys, and cursor
  paint entries.
- `apps/lst-gpui/src/{diagnostics,llm,state_trace}.rs` covers formatting,
  response parsing, and state-trace serialization contracts.
- `crates/lst-x11-harness/src/{editor,state_trace}.rs` contains harness
  self-tests for key-sequence parsing and JSONL trace reading.
- `apps/lst-gpui/examples/bench_editor_x11.rs` contains benchmark harness
  parser, corpus, metric, and trace aggregation tests.

## Watch List

- `apps/lst-gpui/src/ui/input_field.rs`: the remaining text-boundary tests are
  acceptable as widget model checks, but any panel-level behavior should move
  to X11.
- `apps/lst-gpui/src/viewport.rs`: keep only geometry contracts that cannot be
  asserted through the trace without making tests brittle.
- `apps/lst-gpui/src/runtime/tests.rs`: when a filesystem behavior can be
  cleanly driven through the app, prefer a real X11 spec and delete the helper
  test.
- `crates/lst-editor/src/find.rs`: move case-sensitive, whole-word, and regex
  option behavior to X11 once the real app exposes a stable keyboard path or
  harness support for clicking the find-panel controls.
