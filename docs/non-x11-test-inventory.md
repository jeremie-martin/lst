# Non-X11 Test Inventory

Status: 2026-06-17. This is the source-side test map after further pruning plus
the Vim-rewrite and incremental-syntax work.

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
- `apps/lst-gpui/src/viewport.rs` once had its paint-helper tests removed, but
  it now carries a small set of structural paint/line-cache checks again (see
  the Remaining Map). User-visible viewport behavior is still covered through
  `real_x11_viewport.rs`, `real_x11_chrome.rs`, `real_x11_find.rs`, and
  multi-cursor X11 suites; the source-side tests only guard the incremental
  paint/cache bookkeeping.
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

About 58 inline source-side `#[test]` functions remain, plus the model-level
suites in `crates/lst-editor/tests` described at the end. The inline tests are
not preferred behavior coverage; they are the remaining exceptions and
incremental-bookkeeping guards. Many files that used to appear here
(`runtime/tests.rs`, `recent.rs`, `wrap.rs`, `transaction.rs`, `launch.rs`,
`document.rs`, `selection.rs`, `find.rs`, app-side `input.rs`/`llm.rs`/
`diagnostics.rs`/`state_trace.rs`) have since had their source-side tests pruned
in favor of X11 coverage.

### Test Infrastructure

- `crates/lst-x11-harness/src/editor.rs` has 23 parser/self-tests for the
  key-sequence DSL used by X11 suites.
- `crates/lst-x11-harness/src/state_trace.rs` has 7 JSONL reader tests for
  missing files, partial lines, truncation, offset advancement, and latest-record
  semantics.
- `crates/lst-x11-harness/src/screenshot.rs` has 1 image-capture self-test.
- `apps/lst-gpui/examples/bench_editor_x11.rs` has 10 benchmark-runner tests for
  scenario parsing, generated corpora, metric selection, medians, and trace
  aggregation.

These are not editor behavior specs. They protect the tools that make the X11
lane usable.

### Boundary Adapters

- `apps/lst-gpui/src/syntax/mod.rs` and `catalog.rs` have 8 tree-sitter adapter
  tests for language mapping, injection registration, highlight-role contracts,
  multiline parser context, and incremental-reparse equivalence, plus extra
  `catalog.rs` checks gated behind the `internal-invariants` feature.

These should stay small and boundary-shaped. If a test starts asserting editor
behavior rather than adapter correctness, move the behavior to X11 and delete
the source-side assertion.

### Incremental Paint / Cache Guards

These guard the incremental syntax-highlighting and paint-cache machinery, where
the invariant (incremental result equals a fresh computation) is hard to express
through a black-box display test.

- `apps/lst-gpui/src/viewport.rs` has 5 paint/line-cache structural tests.
- `apps/lst-gpui/src/main.rs` has 1 syntax-cache rebuild test.
- `crates/lst-editor/src/tab.rs` has 7 line-cache, buffer-delta, and
  undo-snapshot tests that back incremental reparsing.

### Model-Level Behavior Lanes

Unlike the inline tests above, these are deliberate non-X11 behavior lanes (see
`docs/vim-feature-inventory.md`), not migration targets. They drive the public
`EditorModel`/Vim input surface and assert observable outcomes where exhaustive
coverage through a real display would be too slow.

- `crates/lst-editor/tests/vim_behavior.rs`: 28 grouped Vim parity tests.
- `crates/lst-editor/tests/vim_oracle.rs`: replays the generated 715-case
  Neovim oracle fixture (`tests/fixtures/vim_oracle.json`).
- `crates/lst-editor/tests/editor_model_workflows.rs`: 5 clipboard, find, and
  multi-cursor model workflows.
