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

GPUI core and platform packages use the exact `gpui-pre` 0.3.10 snapshot family
(Zed revision `08cbb88a152c3751a43e853425a70e9f631a5918`). The app selects the X11
platform explicitly. Linux rendering uses wgpu; Blade is no longer a dependency.
`vendor/gpui-linux` retains X11 boundary fixes, and `vendor/gpui-wgpu` avoids path
render-target allocation for scenes without paths. GPUI core uses the published
package. `vendor/tree-sitter` makes `changed_ranges` stop descending into every
subtree it skips. Each vendor directory records its exact diff, rationale and
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

GPUI owns actual keyboard focus. Model focus effects are one-shot requests, not
persistent desired-focus state; editor and Vim input routing checks the actual
editor handle. Rendering maintains focus only for dominant modal/workspace
surfaces and redirects a focused field when it is removed (Replace to Search,
closed find/goto to the editor). A newer primary mouse interaction supersedes
queued nonmodal focus before its handlers run, including through occluding
surfaces. Rendering does not undo ordinary mouse focus changes.

## Editor domain

`crates/lst-editor` owns:

- non-empty tab state, active-tab identity, and file/scratchpad origin
- Rope-backed documents and character/line conversion
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

Save text policies also produce exact `TextChangeSet` edits in `lst-editor`.
`PreparedSave` carries those edits with the bytes written by the app. A successful
save applies them only to the requested revision, through the same transaction
and undo boundary, mapping the current selections and refreshing Find results.
Bookmarks follow line-start character anchors through edits; Rope determines
their resulting logical lines, including CRLF pairs formed at edit boundaries.

Logical line boundaries follow Rope, including CRLF, CR, LF, VT, FF, NEL,
LS, and PS. The editor's `line_content` helpers exclude the complete terminator
for cursor bounds, display text, wrapping, and search. The app uses those same
helpers when measuring and painting rows. Newline insertion preserves the first
existing line-ending style, with LF as the default for a document without one.

Find navigation searches from the current caret and wraps. An exactly selected
match supplies its start as the origin, so backward navigation advances past
that match too. Empty regex matches participate in the ordered index; strict
next/previous position comparisons ensure progress. Hidden Find retains its query,
but document changes clear obsolete result ranges instead of rescanning the buffer.
Opening the panel, navigating matches, or explicitly selecting matches validates
and rebuilds the revision-keyed index on demand.

Find-in-selection belongs to the document. It captures the union of all
nonempty selections, independently of later caret movement, query changes,
or closing the Find bar. Matches must fit wholly inside that union; shared
boundaries do not duplicate empty regex matches. Text transactions map its
character ranges with left affinity at the start and right affinity at the
end, so replacement text stays inside the captured region.

Each transaction owns one derived position map, shared by selection, bookmark,
and scope updates and then transferred to history. Single edits store their
map inline. History keeps these compact, invertible maps alongside its existing Rope
snapshots. This supports scopes captured after earlier edits and movement
between retained redo branches without guessing a diff from repeated text.
Snapshots restore exact scoped ranges after deletion; ordinary typing and
repeated deletion compact their position maps. Mapping storage and saved
scope ranges count toward the existing undo budget. Disabling a capture or
switching tabs releases its saved ranges, so an older capture cannot reappear.
Disk reload also clears the capture. If a branch's connecting history has been
evicted, its scope stays empty rather than widening to the document; undoing
that jump restores the prior capture.

Core collections encode their invariants:

- `TabSet` always contains a tab and owns active-index and tab-ID validity.
- `SelectionSet` is non-empty, ordered, non-overlapping, and has a valid primary
  selection.
- `TextChangeSet` is non-empty, ordered, disjoint, and identifies its primary
  change.
- `TabOrigin` distinguishes ordinary files, scratchpads, and untitled buffers,
  including save expectations and missing backing files.
- `WrapLayout` owns wrapped row offsets and the no-wrap identity mapping.
  Its checked update operations keep line counts, row starts, and wrap widths
  consistent; the app owns cache revisions and supplies invalidated ranges.
  Row counting, segmentation, and cursor navigation share one traversal.
  Non-tab graphemes use Unicode display-cell widths (at least one cell); wide
  CJK and emoji clusters occupy two cells. Graphemes remain indivisible in
  narrow viewports. This is a monospace cell policy, not font-specific shaping.
  `LineChange` validates corresponding old/new windows when line counts change,
  shared by wrapped-row and measured-width updates.

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
- `settings.rs` owns the versioned TOML boundary, path validation, and persistence
  errors. Typed settings define the schema; saves patch changed values into the
  original TOML without rebuilding unrelated fields. The persisted document and
  reload baseline change only after a successful atomic write. Ordinary preference
  changes remain active on save failure; reset applies defaults only after success.
  `recent.rs` owns recent and recently closed file state.
- `syntax/` owns tree-sitter parsers, highlighting, injections, and structural
  snapshots. Parser-derived ranges cross into `lst-editor` only as validated
  character-coordinate types.
- `tab_strip.rs` and `status_bar.rs` own chrome as cached GPUI child views.
  Complete render inputs determine reuse; callbacks operate on current
  application state. Test state traces publish after root prepaint so sibling
  geometry belongs to the same completed layout.
- `viewport.rs`, `editor_view.rs`, `shell.rs`, and `ui/` own layout, painting,
  hit testing, and widgets. `cursor_motion.rs` optionally animates the painted
  caret position; document positions, hit testing, reveal, and IME geometry always
  use the actual selection. One critically damped spring moves the caret
  without changing its shape. Scrolling translates motion with the document;
  tab and layout changes reset it. Blinking preserves the last caret position.
- `voice/` owns native microphone capture, temporary WAV files, the ordered
  batch transcription queue, and the recording toolbar. A session owns one
  target tab; unfinished sessions prevent that tab from closing. Successful
  segments are appended once through `EditorModel::append_text_to_tab`, with
  one history boundary per segment and no replacement of earlier text. Failed
  segments block later uploads until explicitly retried or cancelled. Only
  finalized text crosses into the editor domain. Recording and HTTP work run
  off the UI thread; the UI polls only while a session exists. Dropping a
  session stops capture and cancels its HTTP request, releasing the worker and
  audio files. Upload deadlines scale with the recorded byte count. WAV writes
  are buffered; failed finalization leaves recoverable bytes beside the note
  and reports their location without treating them as a transcribable segment.
  Background delivery preserves unrelated status and X11 PRIMARY ownership.
- `prompt_add.rs` runs the installed prompt-add filter with a 60-second deadline
  covering input, output, and exit. Nonblocking pipes keep every phase bounded;
  failure terminates its private process group and reaps the child. Stdout is
  bounded relative to the input; stderr is drained with bounded retention and
  single-line diagnostics. A dedicated, fallibly spawned worker handles the
  blocking exchange and comparison preparation, leaving GPUI workers available.
  One prompt state owns confirmation, running work, or review. Its target tab
  cannot close until that work is resolved. Results survive tab switches when
  the target revision still matches; only the active target presents a review,
  and another close/save decision takes precedence.
- `prompt_review.rs` owns read-only comparison presentation: immutable original
  and proposed text, cached adaptive word highlights, and virtualized review rows
  inside the existing editor area. It uses editor typography and line spacing.
  Apply rechecks the tab and revision before the existing model replacement;
  Discard never mutates the document. Presenting a review first dismisses competing
  focus surfaces, including Settings and its value editor, so the review that
  receives keyboard input is also visible.

External failures remain explicit at this boundary. A clean file changed on
disk reloads in place. A dirty file changed on disk stays open with a per-tab
resolution notice. A deleted backing file remains a save-or-discard state.

## Rendering rules

UI state should have one geometric and visual owner:

- Derive coordinates once for painting, hit testing, wrapping, scrolling,
  cursor reveal, IME bounds, and the observable test trace. Shared input fields
  publish their current geometry during prepaint, before root trace observation.
  Field selections own their pointer drag beyond hitbox bounds and schedule
  horizontal autoscroll only while that drag can make progress.
  Document drag autoscroll integrates velocity over elapsed time. Hit testing
  chooses the nearest grapheme boundary, with ties going to the earlier one.
  Inactive or invalidated tab geometry cannot complete a cursor reveal.
  Rendering and reveal scheduling share the document-viewport visibility rule.
  Replacement surfaces retain pending reveals without retrying each frame;
  the next document layout resumes them when the viewport returns.
  Geometry-dependent chrome requests its correction after the current paint;
  the status trace reports the rendered text.
- Syntax updates distinguish text changes from highlight invalidation. Only
  text changes invalidate text measurements; a changed injected-language span
  can invalidate colors throughout its region. Each parse owns one complete
  root-injection inventory. Rooted queries reuse results only for unchanged
  subtree identities below a two-level partition; container-level matches are
  queried separately and non-rooted patterns use the full query. Source tests compare this cache with
  whole-tree queries. Fresh structure and reuse verification parse
  individual injections the same way, including Markdown inline nodes.
- Cached ASCII glyphs retain full non-space shaping context, including across
  identifier and punctuation boundaries. Nonuniform advances fall back to
  shaping the whole segment.
- Give each border or separator one owner.
- Use the stable UI font for chrome and the configured editor font for document
  text and gutter digits.
- Use semantic theme roles for focus, selections, matches, brackets, guides,
  and primary versus secondary carets.
- Bound cursor- and decoration-driven work by visible rows or painted character
  windows. Cache keys must include every input that changes visible output.

Exact-pixel X11 baselines are regression checks, not design approval. Inspect a
changed image before updating a baseline.

## Behavior contracts

Vim permits one selection per tab. The model's effective selection limit retains
each primary selection when entering Vim and applies to restored history and
external selection sets. Cursor-addition commands and gestures report that they
are unavailable. Standard mode keeps its independently configured cursor limit.

Implement the recorded contracts with real-X11 acceptance coverage; incidental
current behavior does not supersede those decisions.
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
