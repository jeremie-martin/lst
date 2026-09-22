# Architecture

This document describes the current system and the boundaries contributors must
preserve. Product behavior is specified by the application and its real-X11
tests, not by a separate feature checklist.

## Workspace

The root `Cargo.toml` is a workspace manifest. It has three members:

- `crates/lst-editor` is the framework-neutral editor domain.
- `apps/lst-gpui` is the executable desktop application (`lst`).
- `crates/lst-x11-harness` is test infrastructure for driving the executable.

The app and editor are `default-members`. The X11 harness is compiled only when
selected directly or used by the app's integration tests.

`vendor/gpui` carries documented platform/rendering changes. A narrow upstream
backport in `vendor/blade-graphics` lets GPUI decline unused Vulkan ray-tracing
capabilities. Each vendor directory records its exact diff, rationale, and
removal criteria in `lst.patch` and `LST_PATCHES.md`.

## Runtime data flow

```text
desktop input -> GPUI adapter -> EditorCommand / model API -> EditorModel
                                                           |          |
                                              observable state   EditorEffect
                                                           |          |
                                                    GPUI render <- runtime I/O
```

The GPUI app translates keys, text input, pointer gestures, menus, and dialogs
into the public editor surface. `EditorModel` performs editor-domain state
transitions and queues `EditorEffect` values for work it cannot perform itself,
such as reading the clipboard or saving a file. The app executes those effects
and reports their results back through model APIs.

Rendering reads model state plus app-owned per-tab layout and tree-sitter
caches. It does not own a second editor state machine.

## Editor domain

`crates/lst-editor` owns:

- non-empty tab state, active-tab identity, and file/scratchpad origin
- Rope-backed documents, character/line conversion, and line caches
- ordered selection sets, cursor goals, multi-cursor normalization, and IME
  marked ranges
- edit transactions, undo boundaries, history, and alternate redo branches
- motions, text input, line operations, find/replace, wrapping, language
  detection and language-specific editing behavior
- viewport intent, bookmarks, standard input mode, and the Vim state machine
- typed requests for clipboard, file, focus, and reveal effects

The crate does not open windows, read files, invoke the clipboard, show dialogs,
call network services, or depend on GPUI or tree-sitter.

Text mutations are constructed as `EditRequest` values containing a non-empty,
ordered, disjoint `TextChangeSet`. Applying one request determines the resulting
selection and one undo boundary. Editing helpers return transactions instead of
mutating unrelated model fields in stages.

Core collections encode their invariants:

- `TabSet` always contains a tab and owns active-index and tab-ID validity.
- `SelectionSet` is non-empty, ordered, non-overlapping, and has a valid primary
  selection.
- `TextChangeSet` is non-empty, ordered, disjoint, and identifies its primary
  change.
- `TabOrigin` distinguishes ordinary files, scratchpads, and untitled buffers,
  including save expectations and missing backing files.

Keep new editor behavior behind focused `EditorModel` operations and keep each
invariant in one module. Validate data at a public or external boundary rather
than scattering fallback checks through the core.

## Desktop application

`apps/lst-gpui` owns all framework and operating-system work:

- `main.rs` assembles application state and per-tab view state.
- `startup.rs` prepares explicit launch files alongside platform initialization;
  creating a scratchpad remains conditional on successfully opening a window.
- `workspace_action.rs` defines command IDs, default keybindings, and command
  palette metadata; `input.rs` adapts text, IME, and pointer input.
- `runtime.rs` and `runtime/` execute file, autosave, clipboard, close, and quit
  workflows. Saves use file stamps and generation tickets so stale or
  conflicting writes cannot silently replace a newer version.
- `settings.rs` owns the versioned TOML boundary; `recent.rs` owns recent and
  recently closed file state.
- `syntax/` owns tree-sitter parsers, highlighting, injections, and structural
  snapshots. Parser-derived ranges cross into `lst-editor` only as validated
  character-coordinate types.
- `viewport.rs`, `editor_view.rs`, `shell.rs`, and `ui/` own layout, painting,
  hit testing, and widgets. `cursor_motion.rs` optionally animates the painted
  caret position; document positions, hit testing, reveal, and IME geometry always
  use the actual selection. One critically damped spring moves the caret
  without changing its shape. Scrolling translates motion with the document;
  tab and layout changes reset it. Blinking preserves the last caret position.
- `prompt_add.rs` runs the installed prompt-add filter through stdin/stdout. The runtime confirms whole-document sharing
  and prepares a response only if the tab and revision still match the request.
- `prompt_review.rs` owns read-only comparison presentation: immutable original
  and proposed text, cached adaptive word highlights, and virtualized review rows
  inside the existing editor area. It uses editor typography and line spacing.
  Apply rechecks the tab and revision before the existing model replacement;
  Discard never mutates the document.

External failures remain explicit at this boundary. A clean file changed on
disk reloads in place. A dirty file changed on disk stays open with a per-tab
resolution notice. A deleted backing file remains a save-or-discard state.

## Rendering rules

UI state should have one geometric and visual owner:

- Derive coordinates once for painting, hit testing, wrapping, scrolling,
  cursor reveal, IME bounds, and the observable test trace.
- Give each border or separator one owner.
- Use the stable UI font for chrome and the configured editor font for document
  text and gutter digits.
- Use semantic theme roles for focus, selections, matches, brackets, guides,
  and primary versus secondary carets.
- Bound cursor- and decoration-driven work by visible rows or painted character
  windows. Cache keys must include every input that changes visible output.

Exact-pixel X11 baselines are regression checks, not design approval. Inspect a
changed image before updating a baseline.

## Deliberately unresolved behavior

Two existing surfaces do not yet have a complete product contract:

- Vim commands with multiple active cursors have no accepted semantics.
- Find-in-selection currently scopes to the primary selection; behavior across
  several selections is undecided.

Do not infer either policy from incidental model behavior. Decide the user
contract first and add real-X11 acceptance coverage with the implementation.
Direct keybinding capture is also not implemented; the Settings UI displays
bindings and the TOML file is the editing interface.

## Detailed sources of truth

- Workspace membership and features: `Cargo.toml` and crate manifests
- Public model vocabulary: `crates/lst-editor/src/lib.rs`, `command.rs`, and
  `vim.rs`
- Settings schema and defaults: `apps/lst-gpui/src/settings.rs`
- Command IDs and bindings: `apps/lst-gpui/src/workspace_action.rs`
- Accepted behavior: `apps/lst-gpui/tests/real_x11_*.rs`
- Benchmark interface: `bench_editor_x11 --help`
