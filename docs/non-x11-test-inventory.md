# Non-X11 Test Inventory

Status: 2026-05-14. This is the source-side test map after the first
aggressive pruning pass.

X11 is the behavior gate. A source-side test does not survive merely because it
is a pure invariant. It needs a narrower justification: it is test
infrastructure, an external-boundary adapter, or a temporary structural check
with a clear path to either X11 coverage or deletion.

## Pruned Or Ported

- `apps/lst-gpui/src/tests.rs` was deleted earlier. It mixed GPUI snapshots,
  direct model mutation, keybinding introspection, rendering internals, and
  product behavior in one large white-box suite.
- `apps/lst-gpui/src/ui/input_field.rs` no longer has source-side tests. The
  removed tests asserted private input selection state, word/subword helper
  outputs, and stale-drag internals. Visible panel input replacement is now
  covered by `real_x11_find.rs::find_query_ctrl_a_replaces_existing_query`.
- `apps/lst-gpui/src/viewport.rs` no longer has source-side tests. The removed
  tests asserted paint-helper bookkeeping: syntax style keys, wrap-column math,
  row-local search slicing, and cursor paint entries. User-visible viewport
  behavior is covered through `real_x11_viewport.rs`, `real_x11_chrome.rs`,
  `real_x11_find.rs`, and multi-cursor X11 suites.
- `apps/lst-gpui/src/ui/scrollbar.rs` no longer has source-side tests. The
  removed tests pinned private thumb geometry and track-click math instead of a
  user workflow. Scrolling behavior is covered through viewport and off-screen
  cursor X11 specs; scrollbar-drag behavior should be added as X11 if it becomes
  a product contract.
- `crates/lst-editor/src/document.rs` no longer has source-side tests. The old
  position-conversion clamp assertion is represented as visible go-to
  line/column behavior in
  `real_x11_workflows.rs::goto_line_column_moves_to_requested_column_and_clamps`.
- `crates/lst-editor/src/selection.rs` no longer has source-side tests. The old
  word/subword and cursor-coalescing assertions are represented by X11 motion,
  mouse, and multi-cursor behavior. This pass also added
  `real_x11_motion.rs::ctrl_right_crosses_decomposed_grapheme_word_without_splitting_it`.
- `crates/lst-editor/src/find.rs` no longer has source-side tests. The old
  grapheme-boundary assertion moved to
  `real_x11_find.rs::find_respects_grapheme_boundaries_for_combining_clusters`.
- Runtime save tests for executable-mode preservation, symlink saves, and failed
  safe-save preservation were already removed because the same user-visible
  behavior is covered by `real_x11_regressions.rs`.
- Recent-panel query preservation and input-field keybinding registration unit
  tests were already removed in favor of X11 delivery tests that exercise the
  actual shortcuts and panel focus path.

## Remaining Map

There are 89 non-X11 `#[test]` functions left. They are not preferred behavior
coverage; they are the remaining exceptions and migration targets.

### Test Infrastructure

- `crates/lst-x11-harness/src/editor.rs` has 23 parser/self-tests for the
  key-sequence DSL used by X11 suites.
- `crates/lst-x11-harness/src/state_trace.rs` has 7 JSONL reader tests for
  missing files, partial lines, truncation, offset advancement, and latest-record
  semantics.
- `apps/lst-gpui/examples/bench_editor_x11.rs` has 6 benchmark-runner tests for
  scenario parsing, generated corpora, metric selection, medians, and trace
  aggregation.

These are not editor behavior specs. They protect the tools that make the X11
lane usable.

### Boundary Adapters

- `apps/lst-gpui/src/launch.rs` has 4 CLI parser tests for `--window-title` and
  `--scratchpad-dir`, including missing-value errors.
- `apps/lst-gpui/src/input.rs` has 2 IME UTF-16 conversion tests for surrogate
  pairs and clamping past the buffer end.
- `apps/lst-gpui/src/llm.rs` has 2 OpenAI response-parser tests for content
  extraction and trailing-newline preservation.
- `apps/lst-gpui/src/diagnostics.rs` has 3 log-format/filesystem tests for
  session headers, panic entries, and append behavior.
- `apps/lst-gpui/src/state_trace.rs` has 1 serialization round-trip test.
- `apps/lst-gpui/src/syntax/{mod.rs,catalog.rs}` has 9 tree-sitter adapter
  tests for language mapping, injection registration, highlight-role contracts,
  and multiline parser context.

These should stay small and boundary-shaped. If a test starts asserting editor
behavior rather than adapter correctness, move the behavior to X11 and delete
the source-side assertion.

### Migration Targets

- `apps/lst-gpui/src/runtime/tests.rs` has 17 filesystem/runtime tests:
  scratchpad filename collision handling, save-as cleanup rules, open/save
  result shapes, stale save tickets, external conflicts, deleted backing files,
  and autosave temp-file completion. Many of these are user-visible enough to
  move to X11; the remaining job-ticket checks should disappear as the runtime
  makes stale states unrepresentable.
- `apps/lst-gpui/src/recent.rs` has 6 recent-file persistence tests: dedupe,
  state round-trip, corrupt state, prune, cap, and content-search cap. Panel
  behavior is already X11-covered; persistence across actual app launches and
  cap behavior are good next ports.
- `crates/lst-editor/src/wrap.rs` has 6 wrapping algorithm tests: row segments,
  wide trailing cells, visual-row mapping, vertical target preservation, and
  grapheme-cluster integrity. These are user-visible rendering/motion behavior
  and should be ported with seeded wide-character fixtures plus state-trace row
  assertions where practical.
- `crates/lst-editor/src/transaction.rs` has 3 structural validation tests for
  text-change ordering and inserted-range mapping. These are not accepted
  behavior specs; keep only until the edit request API makes invalid change sets
  unrepresentable enough that the tests add no signal.
