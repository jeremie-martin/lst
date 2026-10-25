# Findings and agreed changes

Address applicable findings from the October 23 and 24 reviews and the
subsequent reconciliation. The desired result is a correct, simple, lean,
maintainable and efficient editor. This checklist describes issues and agreed
behavior; it does not prescribe an implementation. The performance follow-up
below is explicitly deferred until the remaining foundation work is addressed.

**Completion criterion:** the whole checklist remains the scope of this work.
Working through one coherent area at a time does not narrow that scope. Address
every applicable confirmed finding and implement every agreed behavior. Record
the change and verification evidence when closing an item; related symptoms
may share one resolution. Keep unresolved items visible. Optional improvements
and claims needing evidence require an explicit disposition, with reasons for
changing the code or leaving it unchanged. Completion of one area is progress
toward completion of the checklist.

**Design context:** resolving a finding should improve the overall codebase.
Prefer clear ownership and fewer unnecessary concepts, branches and lines.
Keep the defensive behavior and invariants the editor needs. A local guard, a
refactoring, or shared code must earn its place through an actual correctness,
clarity or measured performance benefit. A reviewer suggestion is not itself
a reason to add an abstraction, generalize a helper or narrow supported input.

**Revalidation:** all 154 previous checklist entries were reviewed against
`a9a01e0` on 2026-10-25. Descriptions were narrowed, obsolete claims closed and
optional work separated from defects. One additional test-environment
compatibility issue was observed. Entry counts are not counts of independent
bugs; several entries describe related symptoms or coverage.

- **Reproduced:** observed again in the production app, current public model,
  current syntax code or a specified boundary probe.
- **Source-confirmed:** a concrete failure or missing assertion remains in
  current code; this does not claim a fresh end-to-end reproduction.
- **Cost confirmed:** the work exists; its material impact and the benefit of
  changing it still need a controlled comparison.
- **Needs evidence:** do not implement a fix solely on the existing claim.
- **Optional:** no current defect or sufficient benefit requires a change.
- Checked entries are resolved, rejected, covered elsewhere or outside current
  platform scope. Newly implemented resolutions name their regression coverage;
  historical closures do not imply new code changes.

`C` identifiers refer to the October 24 candidate ledger; identifiers prefixed
with `23/` refer to October 23. Duplicate findings share an item.

## Agreed behavior to implement

- [x] **Standard F3 / Shift+F3 (implemented 2026-10-25):** find the next/previous match from the current caret,
  wrapping once. Advance beyond an already selected active match and handle empty regex
  matches without getting stuck. Matches on lines 0, 2 and 4: from line 2, F3 goes to 4
  and Shift+F3 goes to 0. (C27, Standard-mode remainder)
  Resolution: Standard and Vim navigation share the ordered, caret-relative search.
  An exactly selected match contributes its start, including regex matches whose length
  differs from the query. Empty regex matches remain indexed and navigation advances
  strictly past the origin. Covered by
  `f3_and_shift_f3_search_from_the_current_caret_and_wrap` and
  `zero_width_regex_matches_navigate_and_replace_without_stalling`.

- [x] **Supported Vim commands (implemented 2026-10-25):** match the pinned
  Neovim 0.12.6 profile for supported single-cursor commands, including counts,
  registers, failed motions and cursor placement. The 1,109-case oracle is isolated
  from user configuration; intentional differences and the nightly comparison are
  documented in [Vim mode](vim.md).

- [x] **Vim Ctrl+D/U (implemented 2026-10-25):** remember an explicit count for the
  editor viewport, capped at its height. Resizing resets it to half the new height;
  changing tabs or using Ctrl+F/B does not. The reference probe, model workflow and
  `vim_counted_half_pages_remember_the_count_across_opposite_directions` cover this.

- [x] **Vim and multiple cursors (implemented 2026-10-25):** entering Vim retains
  each tab's primary selection. The model enforces one selection through mode changes,
  undo/redo and external sets, and rejects cursor additions with visible feedback.
  `entering_vim_retains_the_primary_selection_and_reports_unavailable_cursor_additions`
  covers mode entry, feedback, editing and undo in the real app; the model workflow
  also checks inactive tabs and direct APIs.

- [x] **Find in Selection (implemented 2026-10-25):** capture all nonempty selected ranges in the active tab.
  Search only within their union, leaving intervening text untouched. Search navigation
  must not redefine the captured scope. Keep the scope aligned through edits and
  undo/redo; switching tabs clears it. Disable activation without a nonempty selection.
  Matches must fit wholly within one range; empty regex matches must not be duplicated
  at shared boundaries.
  Resolution: `CapturedFindScope` is document-owned and maps through the same offset
  maps used by text transactions. History retains compact position maps and exact
  scoped snapshots, including for edits before capture and retained redo branches.
  Disabling a scope clears its saved ranges; later captures cannot revive them.
  The scope chip accepts any nonempty selection, including a secondary one with an
  empty primary caret. Six `captured_scope_*` real-X11 cases cover the accepted
  behavior, saved bytes, navigation, replacement, and history. Source-side invariants
  cover range unions, map inversion/compaction, and the bounded-history fallback:
  a disconnected old branch keeps an empty scope instead of searching the document.

- [x] **Mouse placement (implemented 2026-10-25):** choose the nearest grapheme boundary consistently for shaped
  and cached text. At an exact midpoint choose the earlier boundary. Clicking the right
  half of the final character places the caret after it. (C62)
  Resolution: One nearest-grapheme rule covers shaped text and cached ASCII cells,
  including the trailing boundary and earlier-boundary midpoint ties. The input adapter
  no longer subtracts half a cell. The real-app presentation click case covers ASCII,
  accented text, and combining clusters; the cell algorithm checks exact ties and the
  font probe checks shaped ties.

- [x] **Configuration paths (implemented 2026-10-25):** expand a leading `~/`,
  require custom note directories to resolve to absolute paths, and display invalid-path
  errors in Settings. General shell and environment-variable expansion is unsupported.
  The destination is independent of the launch working directory. (C51)
  Resolution: deserialization validates both custom note directories before settings
  enter the app; unchanged `~/` spelling remains in TOML. The real-app note-path case
  checks creation under HOME, preservation on save, and rejection of an invalid reload
  while retaining the last valid directory. Boundary tests cover both directory fields.

- [x] **Prompt results and tab switches (implemented 2026-10-25):** keep an improvement result available when its
  target document is unchanged, even if another tab is active. Apply must still refuse a
  result whose target document changed. (October 24 final sweep)
  Resolution: the prompt state retains an unchanged target's result independently of
  the active tab. Returning to that tab presents the review; Apply still checks its
  revision. Real-app tests cover other-tab edits/saves, return/apply/undo, changed-target
  rejection, and arrival during another document's close decision.

## Reconfirmed defects

Unchecked entries are applicable problems. Conditions and limits on the evidence
are noted where they matter. Resolutions name their regression coverage.

### Text, line endings and saving

- [x] **Resolved (2026-10-25):** End and Shift+End crossed into the following line on VT/FF/NEL/LS/PS
  terminators; Left and Backspace could get stuck at the following line's start. Related
  Vim commands, including `$`, `A`, `D` and word operators, could join lines or edit the
  wrong line. (C55, C85; 23/AL4)
  Resolution: shared logical-line content helpers exclude the complete Rope terminator
  for position conversion, motion, editing, wrapping, display, and search. Newline
  insertion preserves the first existing style. Covered for all eight endings by
  `end_selection_and_left_motion_respect_every_logical_line_ending`,
  `backspace_removes_one_complete_logical_line_ending_and_undo_restores_it`, the Vim
  logical-line regression, and document/wrap representation tests.

- [x] **Resolved (2026-10-25):** The caret and current-line highlight could appear on both
  sides of those separators. With smooth cursors enabled, animation could keep redrawing.
  (C45)
  Resolution: viewport row endpoints now use the shared line-content boundary, so
  a terminator is not part of either row's displayed text. Covered by
  `separator_boundaries_paint_only_one_current_line_and_settle_with_smooth_cursor`:
  row geometry, screenshot pixels, and redraw quiet with smooth cursors enabled.

- [x] **Resolved (2026-10-25):** Bookmark positions drifted after inserting non-LF separators or
  forming/splitting a CRLF pair at an edit boundary. (C89)
  Resolution: bookmarks now map line-start character anchors through the transaction
  and resolve their lines against the resulting Rope. Covered by
  `bookmarks_follow_pasted_line_separators_through_undo_and_redo` and the
  CRLF-boundary invariant test in `tab.rs`.

- [x] **Resolved (2026-10-25):** Save-time whitespace trimming failed on CRLF and other supported
  line endings. Ensuring a final newline could introduce a bare LF into a CRLF file.
  Existing terminators must remain intact. (C88)
  Review at `a9a01e0`: The pre-fix runtime save-option functions, compiled in isolation, retain CRLF
  trailing spaces/tabs and append bare LF. Both options are off by default.
  Resolution: editor-owned save policies retain exact edits, preserve existing
  terminator bytes, and use the first existing ending (LF if absent) when adding
  a terminator. Covered by the `save.rs` algorithm tests and
  `normalizing_save_preserves_line_endings_and_caret_and_is_undoable`.

- [x] **Resolved (2026-10-25):** Save normalization replaced text without keeping Find counts and
  highlights current. A query for removed whitespace could still report matches. (C81)
  Review at `a9a01e0`: After a normalizing save, model ranges remained 4..5 and 15..16 for a
  query with no remaining matches. Find commands reindex before acting, so this is stale
  display/count state, not demonstrated wrong replacement.
  Resolution: successful normalization uses the normal transaction path and refreshes
  the active Find index. Covered by
  `normalizing_save_refreshes_find_results_without_another_find_command`.

- [x] **Resolved (2026-10-25):** Save normalization did not map cursor/selection positions through
  its text changes. Removing the voice-triggered explicit save avoided that trigger but
  left the general save behavior unresolved. (`apply_saved_body`; related to C19)
  Review at `a9a01e0`: Normalizing ab<space><space><LF>next with the caret at (1,2) leaves it at
  (1,4). The old numeric character offset is retained.
  Resolution: `PreparedSave` keeps the written bytes paired with their exact edits.
  A current completion maps every current selection, preserves reversed selections,
  and creates one undo boundary. Stale completions leave newer edits untouched.
  Covered by the normalizing-save acceptance tests and the in-flight save boundary
  test in `editor_model_workflows.rs`.

### Syntax

- [x] **Resolved (2026-10-25):** Injected highlights stay stale after edits outside the narrow
  refreshed window: Markdown code fences, multiline inline spans and HTML script/style
  content are affected. (C80; 23/A3-3)
  Review: Reran the saved cases with the current production syntax module. The
  partial-cache probe and JS/Markdown structure comparisons still differ from fresh
  results. Rust host highlighting provides a passing control.
  Resolution: Each parsed revision supplies one complete injection inventory. Changed,
  added, and removed regions widen highlight invalidation. Actual lazy-cache tests cover
  fences, multiline inline spans, script/style content, edits and undo, plus 256
  fragmented edit/undo steps across four languages. The real-app injected-comment case
  compares distant rendered pixels and their restoration on undo.

- [x] **Resolved (2026-10-25):** The remaining JavaScript edit history produces incremental bracket
  structure different from a fresh parse. Reproduced after the first review's fixes.
  (23/A3-4 residual JavaScript counterexample)
  Review: Reran the saved cases with the current production syntax module. The
  partial-cache probe and JS/Markdown structure comparisons still differ from fresh
  results. Rust host highlighting provides a passing control.
  Resolution: Structural reuse validates the complete current injection inventory and
  verifies changed injected bracket streams. The retained two-step counterexample and
  the broader edit/undo cache-equivalence suite now agree with fresh parsing.

- [x] **Resolved (2026-10-25):** The remaining Markdown backtick example also produces incremental
  bracket structure different from a fresh parse. This is separate from stale colours.
  (23/A3-4 residual Markdown counterexample)
  Review: Reran the saved cases with the current production syntax module. The
  partial-cache probe and JS/Markdown structure comparisons still differ from fresh
  results. Rust host highlighting provides a passing control.
  Resolution: Fresh and incremental structure now parse each inline node independently,
  matching the grammar adapter and the existing highlight path. The combined inline
  parser and its special collection path were removed. The retained backtick history and
  fragmented edit/undo cases agree with fresh parsing.

### Settings

- [x] **Implemented (2026-10-25):** Saving an unrelated setting removes inline comments
  and value formatting. (C48)
  Resolution: the typed settings schema drives a diff applied to the original TOML.
  Unchanged values keep their spelling; changed values keep key/value comments.
  `changing_one_setting_preserves_other_values_comments_and_keybindings` checks exact
  saved bytes; a boundary test also covers inline tables and quoted keys.

- [x] **Implemented (2026-10-25):** Saving an unrelated setting removes comments and
  commented-out entries from `[keybindings]`. (C49)
  Resolution: settings writes no longer clear/rebuild the table. The same saved-file
  regression preserves a commented-out shortcut, array formatting, and inline comment.

- [x] **Implemented (2026-10-25):** A failed reset changes the settings store before
  writing succeeds, leaving the store, running app and disk inconsistent. (C29)
  Resolution: reset commits its settings, document, and reload baseline only after a
  successful write. Reset and reload share application of loaded values; resetting zoom
  no longer invokes a second save. The real-app failure/recovery case checks nondefault
  values, failed writes, recovery, reset defaults, and reload after clearing launch overrides.

- [x] **Implemented (2026-10-25):** Save/reset errors are hidden behind Settings while
  the page says “Saved automatically.” (C50)
  Resolution: the store owns persistence errors, displayed above the Settings results
  independently of search and scrolling. Failed-reset confirmation dismisses to reveal
  the error. Failed ordinary saves explicitly say changes are active but unsaved; a
  successful save or valid reload clears the error. Covered by failure/recovery and an
  inspected screenshot; the sidebar no longer claims automatic saving during an error.

- [x] **Implemented (2026-10-25):** Atomic settings writes mishandle symlinks,
  permissions or temporary-file cleanup. (C32)
  Resolution: preserve existing symlinks and update their resolved file targets; reject
  dangling links, non-files, and read-only files. A same-directory temporary file owns
  cleanup, preserves existing permissions, syncs its contents, and atomically replaces
  the target. Real-app coverage checks a relative symlink and mode 0600; boundary tests
  cover failed writes, dangling links, non-files, read-only targets, and absence of debris.

- [x] **Implemented (2026-10-25):** Empty/relative `XDG_CONFIG_HOME` is used without
  validation. (C52)
  Resolution: only absolute configuration roots are accepted. Empty/relative XDG roots
  fall back to an absolute HOME; otherwise persistence is unavailable with a visible
  error. The real app loads distinct nondefault fallback settings for both invalid XDG
  cases; boundary tests include unusable HOME and a valid XDG root.

### Voice

- [x] **Implemented (2026-10-25):** Finalization errors can still delete captured audio.
  Sample-write recovery was added, but `finalize()` can return early and drop the
  recording. Retain recoverable bytes and accurately report whether transcription is
  possible. (C8)
  Resolution: finalization failure disables automatic deletion and reports the recovery
  file beside the note, explicitly saying that segment was not transcribed. A faulting
  WAV seek boundary checks retained sample bytes; the real-app Finish/failure case
  checks the notice, absence of an upload, closing, and retained recovery bytes.

- [x] **Implemented (2026-10-25):** The fixed 120-second upload deadline cannot accommodate long
  segments on slow links. Repeated retries can fail while blocking all later segments.
  (C7)
  Resolution: allow two minutes of processing plus transfer time at 16 KiB/s for the
  recorded byte count; retain a 15-second connection deadline and bounded response
  body. Dropping the request cancels network work and releases audio, including a
  stalled upload. Boundary tests check the full-segment budget and actual local HTTP
  cancellation; existing real-app retry/cancel cases exercise the queue. This is a
  bounded transfer policy, not a guarantee for arbitrarily slow links.

- [x] **Implemented (2026-10-25):** Transcript delivery clears unrelated status messages and can
  reclaim X11 PRIMARY despite being a background update. (C17)
  Resolution: transcript insertion uses the common background-update path, preserving
  unrelated status and never publishing PRIMARY as a foreground selection change.
  The real-app regression holds a response, changes PRIMARY externally, produces an
  unrelated settings error, then checks delivered text, selections, status and PRIMARY.

- [x] **Implemented (2026-10-25):** Dismissing a voice message can leave keyboard focus on an
  invisible root surface, so typing appears unresponsive. (C37)
  Resolution: Dismiss is a voice action with the same explicit editor-focus handoff
  as the other controls. The real-pointer regression dismisses the message, types,
  and checks saved text.

- [x] **Implemented (2026-10-25):** Capture failure after Finish can restore Paused and keep closing
  blocked. (C10)
  Resolution: capture completion honors the requested Finished transition on success
  and failure. The real-app finalization-failure case needs only one Finish, reports
  the failure and recovery path, and closes the note without another Finish.

### Prompts, closing and focus

- [x] **Implemented (2026-10-25):** Voice controls remain interactive during prompt review. Show
  Note can switch tabs and make an otherwise usable review stale; the conflict banner
  has a related interaction gap. (C16)
  Resolution: voice actions and buttons respect pending decisions. Review dims and
  blocks the voice toolbar and file-conflict banner. A real-pointer case attempts
  Show Note, Cancel Pending and Reload, uses handled review keys as synchronization,
  and verifies the target, revision, recording and conflict remain intact.

- [x] **Implemented (2026-10-25):** A tab can enter its close flow while prompt improvement is
  running; the arriving result can coexist with the close prompt and leave a stale
  review covering another tab. (C4)
  Resolution: one prompt state owns confirmation, running work or review and its
  target identity. Closing that target is refused before a close flow can start.
  Another tab's close decision takes precedence over a retained result. Real-app
  cases cover both the dirty target and a different dirty document.

- [x] **Implemented (2026-10-25):** Closing the last blank scratchpad during improvement can delete
  its file before quitting is refused, leaving an open tab with a missing-file warning.
  (C54)
  Resolution: close eligibility is checked before scratchpad cleanup; the last-tab
  path delegates cleanup to successful quitting. A gated real-app regression checks
  that refusal preserves the whitespace file, then discards the result and edits/saves.

- [x] **Implemented (2026-10-25):** Clicking the goto bar's label or padding passes through to
  document text, moves the caret and redirects subsequent typing into the document.
  (C87)
  Resolution: the whole bar occludes the document and hands primary clicks to its
  field. The pointer regression clicks the label and padding using current field
  geometry, submits a line number and verifies navigation without text changes.

### Vim and shortcuts

- [x] **Resolved 2026-10-25:** Normal/Visual counted `$` reaches the counted line,
  including Visual's end-of-line endpoint. (C28)
  Resolution: counted-end oracle rows cover plain, indented, blank and Unicode text.

- [x] **Resolved 2026-10-25:** backward linewise yanks place the cursor on the first
  operated line with the reference's intended column, including tabs. (C67)
  Resolution: the linewise oracle grid checks cursor, text and register together.

- [x] **Resolved 2026-10-25:** linewise deletes preserve the intended column;
  exclusive motions converted at column zero use first-nonblank placement. (C68)
  Resolution: linewise placement is shared by explicit and converted operators;
  the oracle grid covers both directions, indentation and buffer boundaries.

- [x] **Resolved 2026-10-25:** `^` and screen first-nonblank motions stop at the last
  character of whitespace-only lines; `I` still inserts after their indentation. (C90)
  Resolution: first-nonblank oracle rows and the viewport model case cover both roles.

- [x] **Resolved 2026-10-25:** Vim indentation tests recognize ASCII spaces/tabs,
  so Unicode whitespace no longer incorrectly converts deletes to linewise. (C83)
  Resolution: Unicode-space operator rows preserve the reference text and register.

- [x] **Resolved 2026-10-25:** editing no longer forgets `f/F/t/T`; counted `;` and
  `,` preserve direction/till semantics. Character-find history is separate from
  transient pending/Visual state. Oracle rows cover edits and repeated searches.

- [x] **Resolved 2026-10-25:** explicit `/` and `?` searches initialize the pinned
  profile's case-sensitive literal policy, clearing whole-word and regex settings
  and reindexing the retained query. `*`/`#` cannot silently restrict them. (23/V9)
  Resolution: mixed-case/subword oracle queries follow both word-search directions.

- [x] **Resolved 2026-10-25:** Improve Prompt uses the same configurable command
  registry as other optional shortcuts. (C15)
  Resolution: `configured_improve_prompt_shortcut_opens_confirmation` drives the
  configured shortcut into the real confirmation UI.

- [x] **Resolved 2026-10-25:** an overridden Shift+Enter for `find.previous` retains
  Find-only context, while Shift+F3 remains available from the editor. (C41)
  Resolution: `overridden_shift_enter_finds_previous_only_in_find_fields` checks
  newline insertion and navigation from both contexts; it fails on the old binary.

- [x] **Resolved 2026-10-25:** released Ctrl no longer turns ordinary Vim letters
  into Ctrl+D/U/F/B. The expired-chord timer/state and replay path are removed. (C53)
  Resolution: `released_ctrl_then_plain_d_starts_a_vim_operator` fails on the old
  binary and passes now; real held Ctrl paging retains separate acceptance coverage.

- [x] **Resolved 2026-10-25:** held-arrow capture recognizes parsed custom arrow
  bindings regardless of accepted spelling or position in a key sequence. (C84)
  Resolution: `mixed_case_arrow_override_runs_the_configured_command` verifies the
  overridden action changes the real document.

- [x] **Resolved 2026-10-25:** root mouse capture clears pending keyboard chord state
  before pointer handlers, including occluding surfaces. (23/AL7)
  Resolution: `pointer_interaction_cancels_the_select_next_skip_prefix` verifies
  subsequent Ctrl+D adds the next occurrence instead of skipping it.

- [x] **Resolved 2026-10-25:** overflowing parsed counts saturate; line arithmetic is
  bounded, percentages above 100 fail, and explicit `1%` is a percentage. (C56)
  Resolution: oracle boundary rows cover motions, deletes, yanks, indentation,
  replacement and joins at 1, 100, 101, u64::MAX and beyond; huge-count replay no
  longer panics. Scroll model checks also cover a count beyond u64::MAX.

### Rendering, runtime and boundaries

- [x] **Implemented (2026-10-25):** Non-UTF-8 command-line arguments can panic during launch. (C1)
  Resolution: Launch parsing preserves OS-string bytes for paths and validates UTF-8
  only for textual title options. Real X11 opens, edits and saves a non-UTF-8 filename;
  boundary tests cover both scratchpad-directory forms and malformed text options.

- [x] **Implemented (2026-10-25):** The build script repeatedly recompiles the app when its watched
  Git ref is packed or belongs to a linked worktree. (C30)
  Review: A throwaway Git/Cargo fixture using current build.rs rebuilds because
  .git/refs/heads/main is missing after pack-refs. The linked-worktree path also joins
  refs under the wrong Git directory; that part was source-checked.
  Resolution: Git resolves common/worktree metadata paths. Missing ref paths watch
  their existing parent; optional packed refs are watched only when present. Metadata
  reads do not refresh the index. Real Git/Cargo fixtures cover packed refs, linked
  worktrees, detached HEAD, dirty/deleted tracked files and fresh unchanged rebuilds.

- [x] **Implemented (2026-10-25):** Non-ASCII window titles are displayed incorrectly. (C59)
  Review: Real X11 launch with a Japanese/en-dash --title has no _NET_WM_NAME and its
  WM_NAME contains UTF-8 bytes decoded as Latin-1. This affects launch overrides, not
  ordinary title updates.
  Resolution: Creation and updates share asynchronous title writes: UTF-8 EWMH
  and Latin-1 legacy fallback. The real-app test reads both properties.

- [x] **Resolved (2026-10-25):** Tab-switch cursor reveal can use old geometry and finish
  while the caret is still off-screen after resizing or zooming another tab. (C61)
  Review: Per-tab geometry survives invalidation. The immediate reveal reads that old
  geometry and can clear the pending reveal before the new prepaint writes current
  bounds.
  Resolution: Geometry retains display measurements for chrome but explicitly loses
  layout validity on activation and layout invalidation. Reveal waits for current
  geometry. The real-app return-to-tab case resizes and zooms the other tab, then
  requires the restored caret to be inside the new viewport.

- [x] **Resolved (2026-10-25):** Smooth Vim carets can snap because tiny floating-point size
  differences reset their animation. (C46)
  Review: Motion::retarget resets on exact size inequality. Widths formed by subtracting
  two floating-point positions need not be bitwise equal across columns. The symptom
  depends on font, zoom and position.
  Resolution: Glyph width changes update the caret shape without discarding its
  positional spring. The motion test retains both position and velocity across a
  fractional-width change; layout changes still reset motion through its context.

- [x] **Resolved (2026-10-25):** Geometry-dependent status-bar details can remain stale until
  another unrelated event. (C65)
  Review: Status props are collected before viewport prepaint updates geometry;
  correction notifies during prepaint. The current GPUI notify path marks dirty_views
  there without scheduling an ordinary subsequent draw. Existing verifier probe supports
  the same mechanism.
  Resolution: Geometry changes schedule chrome correction after the current paint. The
  trace now reports rendered status text, so it can detect stale props. The real-app
  resize case requires status columns to match actual wrapped rows without another
  input.

- [x] **Implemented (2026-10-25):** Post-input drawing can refresh hidden/obscured windows during
  activity elsewhere. The verifier rejected the destroyed-window part of this claim.
  (C69)
  Review: On a private Xvfb server, a root modifier event causes another app frame after
  the window is unmapped and its trace is quiet. Window attributes still report
  IsUnmapped.
  Resolution: Immediate draws and root modifier routing share the periodic timer
  visibility predicate. A real-app case proves no frames while unmapped despite
  modifier input, then edits successfully after remapping. A separate Xvfb probe
  with Composite disabled confirms FULLY_OBSCURED stops frames. Composite-enabled
  ARGB windows in this environment do not report physical occlusion; this fix uses
  server-reported visibility and does not introduce compositor occlusion tracking.

- [x] **Resolved (2026-10-25):** Drag autoscroll speed depends on mouse event rate and
  changes when the refresh rate drops after inactivity. (C74)
  Review: Document drag autoscroll applies a fixed distance per callback. Dirty
  post-input draws also run next-frame callbacks; there is no elapsed-time
  normalization. The magnitude under different devices was not remeasured.
  Resolution: Both axes share an edge-velocity rule integrated over elapsed time,
  including callback work. The real-app held-drag case compares speed during and after
  the post-input refresh period and checks that release stops scrolling and selection
  growth.

- [x] **Implemented (2026-10-25):** Reopening an input field with unchanged text preserves undo
  history from its previous use. (C86)
  Review: Real app: type abc, erase it, close/reopen the palette, then Ctrl+Z. The
  screenshot shows a restored a in the newly opened palette.
  Resolution: Opening a field explicitly starts a new local edit session, clearing history,
  composition, drag and scroll state even when its text is unchanged. Ordinary model echoes
  retain local history. The real-app reopening test exercises undo and redo in palette,
  Settings, recent-file search and Find.

- [x] **Implemented (2026-10-25):** SIGTERM/SIGHUP can leave nested-display child processes,
  including the private D-Bus daemon, running. (23/V13)
  Review: The entry points install no SIGTERM/SIGHUP handlers. Children use separate
  process sessions and normal cleanup is in the context manager/KeyboardInterrupt path,
  which default termination bypasses.
  Resolution: Both entry points unwind on termination signals and always reap
  owned command groups, including descendants after leader exit. Tests cover both
  signals, both command runners and a descendant ignoring SIGTERM. Full nested-runner
  probes leave no Xephyr, lwm, private D-Bus or command process alive.

- [x] **Implemented (2026-10-25):** Optional startup warm-up can panic through
  infallible thread spawning. (C5)
  Resolution: use a named, fallible thread builder and skip optional warm-up if thread
  creation fails. Voice and prompt workers also report spawn failure. This is a
  source-verified boundary change; host thread exhaustion was not induced.

- [x] **Implemented (2026-10-25):** Prompt-add output/error buffering is unbounded. (C9)
  Resolution: stdout is limited to the larger of 1 MiB or four times the input size;
  exceeding that bound uses the existing process-group cleanup. Stderr retains at
  most 64 KiB while draining the pipe; displayed diagnostics are single-line and at
  most 1,024 characters plus an ellipsis. Boundary tests cover runaway output,
  simultaneous full pipes, large input, and bounded Unicode diagnostics.

- [x] **Implemented (2026-10-25):** Blocking prompt-add occupies a shared background-executor
  worker. (C11)
  Resolution: a fallibly spawned dedicated thread owns process exchange and review
  preparation. GPUI awaits a one-shot result without occupying a shared worker. The
  real-app other-tab save and retained-review workflow also runs with app CPU affinity
  restricted to one CPU, giving GPUI one shared background worker.

- [x] **Implemented (2026-10-25):** The contributor guide still describes an app-owned DeepSeek
  client. (C18; stale wording observed)
  Resolution: AGENTS.md now names prompt-add process execution and voice transcription
  at the app boundary; the architecture document records their actual ownership.

- [x] **Resolved (2026-10-25):** Wide CJK characters count incorrectly in wrapping. (C57)
  Review: Current wrapping counts each non-tab grapheme as one cell. Current GPUI
  shaping with JetBrains Mono produces an 80-character CJK row wider than the computed
  80-cell viewport; the default-font control does not show it.
  Resolution: Row counts, segmentation, and cursor navigation share one traversal using
  Unicode grapheme display widths. Wide CJK and emoji clusters occupy two cells and
  remain indivisible in narrow viewports. Algorithm and real-app tests cover row
  boundaries and navigation; the explicitly loaded JetBrains Mono probe confirms the
  retained CJK example fits. This remains a monospace cell policy, not font-specific
  pixel wrapping.

- [x] **Resolved (2026-10-25):** Cursor animation does not reset on some font/gutter/wrap
  changes. (C63)
  Review: MotionContext resets on tab/bounds/scale/row height, but omits some layout
  changes. Typography invalidation clears caches without resetting motion. The
  documented layout-reset contract is broader; visible impact was not freshly recorded.
  Resolution: Motion context includes font identity, code padding/gutter geometry, and
  wrap columns alongside tab, bounds, scale, and row height. The motion algorithm checks
  each reset and separately verifies scroll translation.

- [x] **Resolved (2026-10-25):** Glyph-cell tokenization loses contextual alternates across
  classes. (C64)
  Review: With JetBrains Mono, 12:30 has different full versus token-shaped glyphs; 1+2
  and the installed default font agree. The current actual cell builder accepts the
  affected fast path. Narrow the claim to demonstrated font/text combinations.
  Resolution: Cached tokens keep entire non-space runs; GPUI retains ownership of
  shaping boundaries within them. Explicit JetBrains Mono probes compare cached glyph
  IDs, clusters, and positions with full shaping for 12:30 and additional mixed tokens.
  Nonuniform tokens still fall back to whole-segment shaping.

- [x] **Implemented (2026-10-25):** Degenerate monitor timing can yield a zero refresh period.
  (C77)
  Review: mode_refresh_rate guards zero fields but not a zero computed millihertz or
  microsecond period. Representable mode values can divide by zero or yield a
  nonadvancing timer. No real monitor emitting such data was observed.
  Resolution: One checked timing conversion computes a positive period directly;
  unusable inputs select the existing 60 Hz fallback. Three isolated tests cover
  normal modes, zero/submicrosecond values and maximum field widths.

## Confirmed costs

The work below exists, but its current end-to-end impact has not been measured.
It is a reason to investigate an appropriate scenario, not an instruction to
add caching, pooling or another layer.

- [x] **Implemented (2026-10-26):** CRLF search used a slower indexing path. (23/EF3)
  Resolution: one byte scanner handles ASCII LF, CRLF and CR; other separators retain
  Ropey's parser. Chunk/line equivalence coverage includes dense and mixed endings.
  A release model probe (one warm-up, seven measured runs) reduced median query time
  on 500,000 CRLF lines from 33.15 to 24.13 ms. LF changed from 23.53 to 24.15 ms
  (about 3% slower in this run). This modest tradeoff keeps one scanner and avoids
  a cache or another retained representation. Whole-app latency is not measured here.
  Evidence: `/home/holo/agent-exports/foundation-costs-2026-10-25/scan-{before,after}.log`.

- [x] **Implemented (2026-10-26):** Tab-label construction repeatedly scans all tabs
  and allocates names, producing quadratic work per render. (23/EF4; October 24 EF8)
  Resolution: Display names borrow the existing path/name storage. Both the tab strip
  and tab list count duplicate names once, then allocate only their resulting labels.
  No retained cache, invalidation protocol or extra source lines were added. Isolated
  release measurements at 10/50/200/1,000 unique tabs show the computation improves
  across the range; this does not claim an end-to-end frame speedup. Fast all-feature
  tests pass; combined X11 acceptance remains pending. Source, individual runs and
  methodology: `/home/holo/agent-exports/foundation-costs-2026-10-25`.

- [x] **Resolved (2026-10-25):** Syntax changes can unnecessarily remeasure
  unchanged layout through the end of a large construct. (23/AL2; October 24 EF1)
  Review: SyntaxInvalidation::Lines supplies a syntax-widened window to the layout
  caches. The unnecessary remeasurement mechanism remains; no new end-to-end timing was
  taken.
  Resolution: Syntax invalidation carries separate text and highlight windows. Layout
  and display-text caches consume only text changes, including when highlighting widens
  over an injected block. A cache test proves distant unchanged text retains its
  allocation. Background highlight completion does not invalidate text. Performance
  comparisons are recorded in the presentation checkpoint.

- [ ] **Model cost measured; design review open:** Wrapped H/M/L scans the lines above the
  viewport on every invocation, making its cost grow with document depth.
  Review: screen_lines walks from line zero to the viewport in wrapped documents.
  A release public-model probe now measures about 108 ms per H/M/L key near the end
  of 500,000 lines, versus 0.008 ms at the start. This is a measured model cost,
  not a full-app latency result. The app already owns incremental wrap geometry;
  resolve ownership before adding another cache or caller-maintained viewport hint.

- [ ] **Cost confirmed; impact unmeasured:** Background polling, blinking, PRIMARY
  publication, retained Find queries and per-request HTTP-client creation were reported
  as unnecessary work. Their current costs and useful behavior need checking. (October
  24 EF2-EF7)
  Review: Background work is present, but some is necessary for live meters, maintenance
  and searches. Separate measured redundant work from useful behavior; the current
  evidence does not justify removing those mechanisms wholesale.
  Progress (2026-10-26): hidden Find now retains only its query and clears obsolete
  ranges; reopening, F3 and explicit match selection rebuild through existing
  revision validation. Removed duplicate text-input reindexing. A 500,000-line
  release model probe falls from 34.22 ms/key to 0.00068 ms/key with a retained query;
  this isolates model work, not display latency. Real-X11 coverage passes hidden
  edits, reopen, forward/back navigation, undo and tab switching. Other mechanisms
  in this grouped finding still need their individual dispositions.

- [ ] **Cost confirmed; impact unmeasured:** Allocation occurs inside the realtime
  microphone callback. (C13)
  Review: downmix allocates its PCM Vec in the realtime callback. The allocation exists;
  no overrun attributable to it was reproduced. Measure before introducing buffer-pool
  machinery.

- [x] **Implemented (2026-10-25):** Unbuffered WAV output performs many tiny
  writes. (C14)
  Resolution: wrap the file in BufWriter. The same one-second, 48 kHz mono PCM boundary
  produces byte-identical 96,044-byte WAV files with 14 underlying writes instead of
  48,003. This verifies write granularity, not end-to-end latency. Callback buffer
  pooling remains a separate, unmeasured item.

## Test and benchmark findings

These are defects or specific gaps in verification. The passing baseline does
not demonstrate the missing observations.

- [x] **Implemented (2026-10-25):** Close, cleanup-confirmation and Settings tests can accept
  premature dismissal as successful ignored-key handling. (23/B3-1; October 24 AT1, AT2,
  T4)
  Resolution: Modal cases assert the still-open modal after a later resize event; Settings
  observes the typed search selection before closing. Dismissal can no longer serve as the
  successful observation of ignored input.

- [x] **Implemented (2026-10-25):** Multi-cursor goal tests give every cursor the same goal, so
  they cannot prove independent preferred columns. (23/B3-2)
  Resolution: Two independent preferred columns (eight and three) clamp on short lines, then restore their distinct columns when moving down and back up.

- [x] **Implemented (2026-10-25):** Malformed-settings reload coverage uses default
  values and cannot prove that nondefault active settings survive. (23/B3-3)
  Resolution: the malformed-reload acceptance case now starts with wrapping disabled
  and a cursor limit of seven, and requires both to survive the rejected reload.

- [x] **Resolved (2026-10-25):** The below-document click test clicks far right of the last
  short line, making two different hit-test behaviors indistinguishable. (23/B3-4)
  Review: The helper clicks the horizontal center of a wide viewport below a
  seven-character final line. Both end-of-document and x-clamped-last-row behavior
  return the same position.
  Resolution: The real-app test now clicks near the left edge below the final row and
  still requires end-of-document placement, distinguishing it from x-clamping to the
  last row.

- [x] **Resolved (2026-10-25):** Save-trimming tests typed Tab keys that inserted spaces; actual
  tab characters were untested. (23/B3-5)
  Resolution: the mixed-ending save regression seeds actual tab characters in its
  file fixture and checks the saved bytes, undo, and redo.

- [x] **Resolved (2026-10-25):** Language-to-syntax mapping coverage was removed for several
  languages. Language detection tests do not cover that separate mapping. (23/B3-7)
  Resolution: The mapping match explicitly handles every Language variant, including
  plain shell variants, so new variants require a decision. A boundary test covers all
  17 languages, shared JSON/JSONC and CSS/SCSS backends, and the no-language case.

- [x] **Implemented (2026-10-25):** Held-chord repetition tests cannot distinguish three
  successful presses from one. (23/B3-8; October 24 AT4)
  Resolution: The shifted-symbol case keeps Control and Shift held while asserting each of three alternating bracket destinations independently.

- [ ] **Source-confirmed:** The Keep Mine test never introduces a newer disk version
  after the conflict banner, leaving the version guard untested. (October 24 AT3)

- [x] **Resolved (2026-10-25):** Local field redo was overwritten before its result was
  asserted. (October 24 AT5)
  Resolution: `standard_boundary_shortcuts_and_redo_use_local_field_history` now
  asserts the undone and redone query before replacing it, and the redone replacement
  before cutting it.

- [x] **Implemented (2026-10-25):** Dismiss tests cannot prove suppression lasts for the
  observed disk version, and no longer check that Dismiss preserves external file
  contents. (October 24 AT6, T7)
  Resolution: Three successive clean-tab reloads prove later external-file observation passes
  completed. Each return to the dismissed tab requires no banner, retained local modifications
  and unchanged external bytes; a newer external version still raises a banner.

- [x] **Resolved (2026-10-25):** Find-field mouse tests could use caret geometry from before
  the preceding key action. (October 24 AT7)
  Resolution: every `caret_point` caller supplies the expected field offset; the
  helper waits for that exact collapsed selection and its rendered caret geometry.

- [x] **Implemented (2026-10-25):** Horizontal-scroll tests mix logical and physical coordinates
  and may mistake an unprocessed click for the rightmost scroll position. (October 24
  AT8)
  Review: The helper scales only the inset, rather than the whole logical coordinate,
  and treats an unchanged offset as proof that scrolling reached the end. The current
  scale-one suite does not exercise the scaling difference.
  Resolution: The trace exposes painted track/thumb bounds. Tests scale whole coordinates, drag
  the actual thumb, and wait for its right edge to reach the track end instead of accepting an
  unchanged offset.

- [x] **Implemented (2026-10-25):** Coverage no longer checks that multiline selections ending
  at column zero exclude that final line when indenting or adding line-end cursors.
  (October 24 T1)
  Resolution: A real-app case ends a multiline selection at column zero, checks saved
  indentation excludes that line, then checks line-end cursor positions and resulting edits
  after undo.

- [x] **Resolved (2026-10-25):** Replace All coverage did not check that Find results
  refresh after replacement. (October 24 T2)
  Resolution: both document-scope and selection-scope acceptance tests assert a
  nonzero match count before replacement and zero afterward, alongside saved bytes.

- [ ] **Source-confirmed:** Quit-review coverage no longer checks input blocking while
  scratchpads are saving. (October 24 T3)
  Review: QuitReview::is_running includes saving_scratchpad, but current tests do not
  hold that phase while sending cancellation input. The guard is present; this is
  coverage, not a confirmed quit bug.

- [x] **Implemented (2026-10-25):** Multi-cursor indent coverage checks text but not retained
  cursor count. (October 24 T5)
  Resolution: Indent and outdent now assert both cursor positions and saved text.

- [x] **Implemented (2026-10-25):** Ctrl+F while Search is focused with Replace open is no
  longer covered. (October 24 T6)
  Resolution: The Find case invokes Ctrl+F with Search already focused and Replace expanded, then observes new query input while both conditions remain true.

- [x] **Implemented (2026-10-25):** Trace-schema checks leave optional input records empty, so
  nested field mismatches escape the contract check. (C33)
  Review: Optional nested field records remain None in the trace contract fixture, so
  their nested keys never enter field_paths. App/harness field definitions currently
  agree.
  Resolution: The schema fixture populates Go To, Settings search and both Find input records, including their nested selection and geometry fields.

- [x] **Resolved (2026-10-25):** Partial syntax-cache checks miss the injected-language cases
  that still produce stale visible results.
  Resolution: Tests now exercise ViewportCache and lazy line requests after injected-
  language edits and undo, comparing all materialized highlights and structural
  snapshots with fresh parsing. The visible injected-color/undo test protects the
  corresponding user behavior.

- [x] **Resolved 2026-10-25:** all 912 existing oracle rows, including the 23
  additions, regenerate unchanged on stable Neovim 0.12.6. The explicit profile
  records the formerly implicit `nostartofline` behavior. User configuration and
  persisted state are excluded; UTF-8 byte columns are converted at the boundary.
  Resolution: repeated and reversed-order generation agree. Latest-nightly comparison
  and the four undo-cursor differences are recorded in [Vim mode](vim.md).

- [x] **Implemented (2026-10-25):** Settings tests use fixed, unscaled pixels; the older-schema
  find helper also estimates field positions. (23/CV2, CV3)
  Review: Current settings tests contain unscaled pixel assumptions. The estimated
  find-field fallback is limited to older schemas and is intentional compatibility
  behavior; that part is not a current-schema defect.
  Resolution: Settings clicks use captured category/search geometry and the shared whole-coordinate scaling helper. Older-schema Find fallback remains compatibility behavior.

- [x] **Resolved (2026-10-25):** Navigation benchmarks at `--position end` mostly time no-op
  Down keys. (October 24 H1)
  Review: operation_line(End) starts near EOF and the latency loop always presses Down,
  sixty times. Most samples become no-ops even though frames are still produced.
  Resolution: Navigation latency alternates real movements around the selected location,
  starting downward only at line zero. A state-traced run verifies that all 60 timed
  arrows move near EOF.

- [x] **Resolved (2026-10-25):** Benchmarks inherit personal settings and write temporary
  corpus files into the real recent-files list. (October 24 H2)
  Review: spawn_editor sets display/trace variables but inherits HOME and XDG paths;
  ordinary startup records corpus paths in recent files. This is a real isolation
  problem.
  Resolution: Every app launch receives private temporary HOME and XDG
  config/data/cache/state directories. A hostile-parent-profile run verifies default
  input/wrap settings, distinct child paths, and byte-for-byte preservation of parent
  settings and recent-file sentinels.

- [x] **Resolved (2026-10-25):** Benchmark medians can mix viewport sizes while reporting the
  first run's size. (October 24 H5)
  Review: A window-size mismatch only warns and both sizes still enter the same median;
  the reported size can come from the unmeasured priming run.
  Resolution: A size mismatch now aborts the scenario before emitting a combined median;
  priming and measured runs must have the same client dimensions.

- [x] **Resolved (2026-10-25):** Paste timing includes trace reading/parsing before input
  injection. (October 24 H6)
  Review: The paste start timestamp precedes read_editor_trace, so unrelated trace I/O
  is included before Ctrl+V is injected.
  Resolution: The existing paste-application count is read before either start
  timestamp; both clocks start immediately before Ctrl+V injection.

- [x] **Implemented (2026-10-25):** Killing a test while arrow keys are held can leave them
  pressed for later tests; existing stale-key cleanup releases only modifiers. (October
  24 H8)
  Review: HeldKeys releases in Drop only; signal termination skips it. The next test
  releases modifiers only, not held arrows. The server-side stuck-key case is relevant
  to teardown even when ordinary tests pass.
  Resolution: Before spawning the next app, the harness releases every pressed
  key reported by X11. The real-app test kills an input process holding two arrows
  and Shift, verifies the stuck state, then verifies recovery and saved editing.

- [x] **Implemented (2026-10-25):** The stock nested runner is incompatible with this host's
  installed lwm command line and configuration schema.
  Review: It passes the fixture as a positional argument; current lwm requires --config.
  The fixture also contains the removed workspaces.count key. The passing audit used an
  external shim and a temporary fixture, not a repository fix. State the supported
  version or resolve compatibility.
  Resolution: Use --config, --check-config and --log-target stderr with the
  current workspace-names fixture. Focused acceptance and signal probes pass against
  installed stock lwm 0.1.0, without the earlier compatibility shim.

## Claims requiring evidence before implementation

The unchecked claims below remain unconfirmed in the stated sense. Establish an
applicable failure, a required contract or a material measurement error before
changing code. Rejection is a valid outcome.

- [x] **Reviewed (2026-10-25):** The claim that every diff hunk needs another lst patch marker
  is not established by the repository rule. Check logical patch coverage before adding
  vendor comments. (23/CV1, CV5; October 24 K1, K3)
  Review: The reported missing-hunk counts overstate the evidence. Existing field/helper
  comments mark the logical cache/scanner changes, and manifests describe them. Identify
  a logical change without a covering marker before requiring repetitive vendor
  comments.
  Disposition: Audited all three patches against their pinned crates.io archives.
  Each applies and reproduces the vendored source exactly (Blade also includes an
  added license file). Logical changes have source markers and manifest coverage.
  Repeating a marker on every hunk would add noise without identifying a missing patch.

- [x] **Coverage strengthened (2026-10-26):** Clipboard preservation checks may
  finish before a late clipboard owner overwrites the selection. (23/B3-6)
  Disposition: The regular-file quit case now makes any nonempty persistence attempt
  fail at the existing process boundary, which would keep the app open. Successful
  quit plus unchanged CLIPBOARD and PRIMARY proves this path did not attempt to spawn
  an owner; an early sentinel read cannot mask a late acquisition. Existing scratchpad
  cases separately cover acquisition and the visible failure/retry behavior. No
  production acknowledgement protocol was added.

- [ ] **Needs evidence:** Damage receipt timestamps may pair an old re-presentation with
  a newer frame, understating key latency. (October 24 H3, candidate)
  Review: The algorithm uses event receipt time, so queued older damage can in principle
  be accepted. Its occurrence and effect on reported latency have not been reproduced;
  validate the measurement before redesigning it.

- [ ] **Needs evidence:** Startup damage may be window mapping/resizing rather than the
  first displayed editor frame. (October 24 H4, candidate)
  Review: The stamp/damage comparison alone cannot prove a compositor displayed the
  intended frame. The claimed mismeasurement was not reproduced; keep it as a
  measurement-validity question.

- [ ] **Needs evidence:** Harness key synchronization predicts document changes and may
  miss actions whose observable state is outside those predictions. (23/AL6, plausible)
  Review: The harness observes several semantic state changes but cannot predict every
  action. No new missed-action failure was established. Avoid inventing a general
  acknowledgement layer without a reproducer.

- [ ] **Needs evidence:** Settings selected-row contrast is inadequate. (C43)
  Review: Active category colours have relatively low contrast, especially in Dark, but
  the ledger does not establish a readability failure or accepted visual criterion. Keep
  this as a visual review question, not an automatic palette redesign.

- [ ] **Needs evidence:** Continuous input plus drawing can starve the main event loop.
  (C75)
  Review: Adding draws inside the drain loop can prolong it, but actual foreground-task
  starvation under sustained input has not been demonstrated. A scheduling rewrite is
  not yet justified.

- [x] **Implemented (2026-10-25):** RandR failure can leave the app without a refresh timer. (C78)
  Review: The screen-resource error branch returns without installing a refresh timer.
  End-to-end reachability/impact after successful window creation was not reproduced;
  keep this as a specific display-failure case to establish.
  Resolution: Reproduced with RANDR disabled on a private Xvfb server: the mapped,
  focused baseline stops at two frames; the fixed app continues periodic frames.
  Resource-query errors, absent extension and unusable modes now share a positive
  fallback instead of returning without a timer.

## Optional improvements

Every open opportunity below deserves a concrete assessment as part of the
foundation work. Implement improvements that demonstrably simplify ownership,
reduce unnecessary work, improve usability or strengthen coverage. Otherwise,
record why retaining the current design is preferable; the optional label alone
is not a disposition. Checked entries record completed assessments or changes.
Some duplication is real, but shared code may be more complex or join boundaries
with different semantics. No blanket refactoring or new subsystem is implied.

- **Optional:** Passive-occurrence refresh is duplicated during focus handling and can
  recreate a query that was just cleared. (23/S4)
  Review: The refresh can run both for a focus callback and an explicit editor-focus
  request. Some requests intentionally restore passive highlights. No new harmful case
  was established; removing the tail unconditionally could change useful behavior.

- [x] **Retained after review (2026-10-25):** Width remeasurement keeps its Rope and
  borrowed-string paths. (23/S2)
  Disposition: the Rope path measures unchanged line topology without copying ordinary
  lines; the visitor path handles changed topology and full rebuilds. Both now use the
  same line-content boundary. Sharing their small adapters or repeated `LineChange`
  validation would add plumbing without removing an invalid state or demonstrated cost.

- **Optional:** Tab strip and status bar duplicate cached-view plumbing. (23/S3, R6;
  October 24 S5)
  Review: Cached-view plumbing remains similar, but each view has different props and
  layout. Share only if that actually reduces production complexity.

- [x] **Retained after review (2026-10-25):** Input fields and documents keep their own
  grapheme-boundary adapters. (23/S5, R7)
  Disposition: fields use UTF-8 byte offsets into one string; documents use Rope
  character offsets, line context, and separate terminator handling. Both delegate
  segmentation to `GraphemeCursor`. Sharing the adapters would introduce coordinate
  conversion and coupling without removing duplicated segmentation logic.

- [x] **Assessed (2026-10-25):** Unused theme-button trace state gives no evidence that a theme button is
  absent. The theme-application assertion itself remains meaningful. (23/S6)
  Review: The trace field is always None. Its absence assertion is vacuous, but the same
  test still checks theme application and can fail for that. Cleanup should preserve
  trace compatibility.
  Disposition: Removed the unused app state and vacuous absence assertion. The old trace key remains null for compatibility; the meaningful theme-application assertion stays.

- [x] **Retained after review (2026-10-25):** Find keeps its short match-position
  conversions. (23/S7)
  Disposition: public position conversion clamps to document boundaries, while indexed
  ranges use already validated line offsets. Their distinct contracts are useful; a
  helper for constructing two `Position` values would not simplify ownership or
  improve the measured navigation path.

- **Optional:** Benchmark trace truncation/polling logic is duplicated. (23/S8)
  Review: The benchmark has repeated trace/polling scaffolding. No independent defect
  was established; timing correctness is tracked separately below.

- [x] **Assessed (2026-10-25):** Test helpers duplicate input synthesis already owned by the harness.
  (23/R5, CV4)
  Review: Some held-key and drag helpers duplicate harness operations, while also owning
  test scope/cleanup. No blanket migration is justified. The killed-test key-release
  defect is separate.
  Disposition: Retained the small scoped HeldKeys adapter: it allows observable assertions and
  mouse actions while modifiers remain down, and releases them on scope exit. The key DSL owns
  complete sequences and cannot provide this lifetime. The strengthened held-chord test
  exercises the distinction; a new shared input abstraction would add plumbing without removing
  production complexity.

- [x] **Assessed (2026-10-25):** Settings tests duplicate trace-to-window scaling. (23/R8)
  Review: Coordinate conversion appears in multiple test helpers. The actual scaling
  error is tracked separately; sharing helpers is optional.
  Disposition: Settings category and search clicks now use the existing shared click_trace_bounds_center helper. No new coordinate abstraction was introduced.

- **Optional:** GraphemeCell's increased size adds per-cell memory cost. Assess its
  impact; the suggested narrower integer representation is not a correctness
  requirement. (23/EF7)
  Review: Current GraphemeCell is 32 bytes on this 64-bit target. Narrowing offsets
  introduces representable-size constraints; no current benefit justifies that choice by
  itself.

- [x] **Assessed (2026-10-26):** Test boundaries reviewed individually. (October 24 K4, K5, K8)
  Disposition: retain public BufferDelta replay as a consumer-equivalence contract.
  Move regex replacement workflow coverage to real X11: whitespace matching, capture
  expansion and replacement preserve all eight document line separators in saved
  files. The physical-display case passes; remove its source-side workflow duplicate.
  Keep small algorithm and representation tests in their current lanes.

- **Optional:** Additional duplicated logic was reported in code-line builders,
  command-modifier declarations, settings metadata, line-edit builders and invalidation
  sequences. Assess each concrete duplication without presuming a new abstraction.
  (October 24 S4, S6-S8, K6, R8)
  Review: Similar builders/switches remain, but no independent failure was established
  for the broad cleanup group. Specific shortcut, invalidation and metadata defects
  retain their own entries.

- [x] **Retained and documented (2026-10-25):** Physical-display selection owners
  retain the last clipboard contents.
  (October 24 H7)
  Review: Physical-display tests intentionally replace selections. The last X11 owner
  must stay alive to serve its clipboard; merely killing it would not restore the user
  clipboard. Nested teardown isolates this. Treat desktop restoration/documentation as
  test hygiene, not a demonstrated growing owner leak.
  Disposition: Documented physical focus/selection side effects and nested isolation.
  Killing the final owner would lose the copied contents; restoring arbitrary prior
  clipboard formats would add a separate desktop-restoration feature. Retain ownership.

- [x] **Implemented (2026-10-26):** Microphone sample-format portability. (C12)
  Resolution: dispatch all ten formats exposed by the pinned CPAL release through
  the existing generic capture/downmix path. No new conversion abstraction is needed.
  Boundary tests cover the new formats' silence conversion. This verifies conversion,
  not every physical microphone; unknown future formats still return a clear error.

- [x] **Implemented (2026-10-26):** Normalize voice API credentials at the boundary. (C20)
  Resolution: trim once before emptiness validation and header construction. The real
  voice workflow's local HTTP server verifies that a whitespace-padded fixture key
  arrives as the exact trimmed header. Configuration documentation states the behavior.

- **Optional:** Status language labels use debug formatting. (C34)
  Review: Debug formatting is present, but current labels are readable enum names and no
  incorrect language choice was shown. Display naming polish does not require a new
  abstraction.

- **Optional:** Non-LF parser-position translation still adds linear rescans. (C47)
  Review: Ascending edits use one memoized LF scan and share position_after_insertion.
  Non-LF translation requires work, but no remaining excessive-cost case was
  established. The original quadratic fallback was already fixed.

- **Optional:** Grapheme rounding copies long lines repeatedly. (23/EF5, plausible)
  Review: Long-line fallback copying exists, but its contribution to real movement cost
  is unmeasured and other segmentation work can dominate. Preserve grapheme correctness;
  no automatic chunk adapter rewrite.

- [x] **Retained after review (2026-10-25):** Wrap updates keep separate topology-change
  and in-place branches. (23/S1, R1)
  Disposition: changed topology splices a replacement index; unchanged topology updates
  existing entries and skips the suffix entirely when row counts do not change. A common
  replacement-vector path would add allocation and suffix work to ordinary edits.
  Existing incremental/full equivalence tests protect both paths.

- [x] **Addressed / retained after review (2026-10-25):** Line-content boundaries are
  shared; syntax line-window adapters remain local. (23/R2, R3)
  Disposition: the confirmed line-ending inconsistency is resolved above. Syntax's
  edited-text window, invalidation window, and layout window serve different consumers;
  a full highlight invalidation can still permit a narrow layout update. Combining
  their range expressions would obscure that distinction. The confirmed stale syntax
  defects were subsequently addressed in the syntax and text presentation checkpoint.

## Upstream alignment after the runtime boundary batch

After completing and committing the current runtime boundary batch, review
upstream updates and carry out justified upgrades autonomously. If no upgrade is
warranted, record why and continue directly with the remaining findings and
optional assessments; resume that work after any upgrades as well. The focused
typing-performance follow-up remains deferred until the foundation work is done.

- [ ] **Upstream review and rebasing:** Implementation candidate prepared; acceptance
  remains pending the full display gates and controlled performance comparison.
  - GPUI 0.2.2 is still the newest non-yanked registry release. Retain that released
    base and its documented patches; upstream main now splits platform work into
    additional workspace crates, requiring a separate unreleased-platform migration.
  - Upgrade Blade graphics 0.7.1 to 0.9.0 and blade-util 0.3.0 to 0.5.0. The released
    ray-tracing opt-in replaces our backport, allowing removal of the entire Blade
    vendor copy and workspace override. GPUI needs only explicit descriptor fields,
    optional device selection, wait-result adaptation and two WGSL validation fixes.
  - Upgrade Tree-sitter 0.26.8 to 0.27.1. Its parser/query and Rust API fixes are useful;
    grammar ABIs remain supported. The changed-range optimization applies unchanged
    and remains absent from upstream master, so retain that single patch.
  - All-feature tests pass on the candidate. Real-app launch exposed and verified the
    WGSL adjustments; focused runtime/presentation checks pass except one resize
    timeout whose three isolated reruns pass. Do not count the upgrade accepted until
    the broad gates, visual review and performance evidence are complete.


## Deferred performance follow-up

Schedule this after the remaining nonoptional findings have been addressed and
the open optional improvements have each been implemented or given a reasoned
disposition. Those changes may alter the relevant paths; measure the resulting
foundation, including the upstream-alignment review above, before choosing
further optimizations.

- [ ] **Measured regression to revisit:** Reduce avoidable sustained-typing overhead
  introduced by the syntax and text presentation changes in `87729c9`, while
  preserving their correctness guarantees and the simpler ownership model.
  The controlled Rust and injected-Markdown runs became slower; injected-Markdown
  paste improved. Throughput measurements do not establish individual-key latency
  or everyday perceptibility. The checkpoint and external evidence bundle retain
  the original comparison, including the pre-change `e063af7` baseline.

1. Reconfirm the regression on the completed foundation using the same isolated
   runner, corpora, viewport and build profile for both sides, following
   [Performance](performance.md). Keep historical results separate from new runs
   and repeat enough measurements to distinguish persistent cost from variation.
2. Profile the remaining cost before selecting a change. Attribute time to query
   discovery, structural verification, parsing, layout and painting; do not assume
   the subtree cache is still the dominant cause after the intervening work.
3. Prefer removing redundant work or simplifying ownership. Preserve complete
   injection discovery, incremental/fresh equivalence, grapheme behavior and
   visible correctness. Any additional cache or fast path must earn its complexity
   through a measured benefit and a clear validity rule.
4. Verify sustained throughput and individual-key latency separately, including
   slower samples, then check paste responsiveness and memory for tradeoffs.
   Record the comparison and required regression checks. Close this follow-up with
   either a verified improvement or an explicit, evidence-backed explanation of
   any remaining cost; do not treat the current regression as inherently necessary.

## Closed, already resolved or outside current scope

Keep these dispositions to avoid reintroducing completed work or treating
intentional behavior as a defect.

- [x] **Closed after review:** Parser-position fallback duplicates insertion-position
  calculations. (23/R4)
  Review: Obsolete claim: current ParserPositions::at_byte already calls
  position_after_insertion. No duplicate insertion-position implementation remains
  there.

- [x] **Already resolved / verified:** Real-X11 acceptance has now run for the baseline:
  286 nested tests passed. The two physical-display visual tests were not run.
  Review: The run used temporary wmctrl/xsel tools, a compatibility shim for this host's
  newer lwm CLI/schema, and software Vulkan. The production app and repository test
  sources were unchanged. The standard runner compatibility limitation is recorded
  separately.

- [x] **Closed after review:** Unsorted deduplication in grammar warm-up has no
  demonstrated correctness consequence. (C6)
  Review: Repeated nonadjacent languages can survive dedup(), but grammar initialization
  uses LazyLock. There is no incorrect output or repeated compilation; no correctness
  fix is required.

- [x] **Closed after review:** Incoming transcripts invalidate a simultaneous
  improvement result. Clarify its relationship to the document genuinely changing. (C35)
  Review: A transcript changes the target revision. Rejecting the older proposal is
  correct. Preserve the separate complaints about background side effects and loss on a
  tab switch without an edit.

- [x] **Closed after review:** Voice actions redirect focus even when the user is
  working elsewhere. (C36)
  Review: voice_action is called by explicit user commands/buttons and intentionally
  returns to the editor/note. The missing focus restoration on Dismiss is a separate
  reproduced defect; automatic background focus theft was not established here.

- [x] **Closed after review:** No current settings-default discrepancy was found.
  Additional default-pinning coverage is optional. (C44)
  Review: Serialized current defaults match every value in the documented TOML schema.
  The GuideMode default variant does not override EditorSettings explicit Off values.
  More default-pinning tests are optional coverage, not a current default defect.

- [x] **Closed after review:** Settings keyboard navigation stops at category
  boundaries. (C66)
  Review: Settings intentionally filters rows to the selected category unless search is
  active. That matches the documented sidebar/category navigation; cross-category arrows
  would be an additional feature.

- [x] **Closed after review:** Input-triggered frame drawing is unpaced. (C70)
  Review: Immediate dirty post-input drawing is an intentional latency optimization.
  Lack of a monitor-rate cap alone is not a defect; actual starvation and redundant
  hidden draws are separate entries.

- [x] **Inactive platform:** The idle shortcut skips frame completion on Wayland,
  currently disabled. (C71)
  Review: Wayland is disabled by this app dependency configuration. Keep the observation
  for a future port; it is not current X11 remediation.

- [x] **Inactive platform:** Glyph tiles can survive DirectX device loss on Windows,
  currently outside the supported target. (C72)
  Review: The active app uses Unix-specific process APIs and X11. The DirectX
  observation is outside the current supported target, and was not verified on Windows.

- [x] **Closed after review:** Refresh selection uses unrelated monitors' rates. (C73)
  Review: The patch deliberately chooses the fastest active CRTC, as documented.
  Per-window monitor selection could improve efficiency, but no current correctness
  failure justifies reversing that choice.

- [x] **Covered elsewhere:** Other applications' XKB events unnecessarily sustain fast
  refresh. (C76)
  Review: Global XKB events reach the input batch path. Hidden rendering is reproduced
  in C69; an independent material fast-refresh cost was not measured. Keep one shared
  issue rather than count another defect.

- [x] **Closed after review:** GPU initialization starts before later fallible startup
  work. (C79)
  Review: The GPU task can start before fallible X11 setup, but failure aborts app
  initialization and releases the receiver. No persistent leak or useful-work defect was
  demonstrated; do not serialize startup merely to change the ordering.

- [x] **Closed after review:** No current cached/intrinsic status-bar height discrepancy
  was found. (23/AL8)
  Review: Cached height currently uses the tallest control, the same vertical padding,
  and the same unscaled top border as intrinsic layout. No mismatch was established; the
  original verifier also said the formula matches today.

- [x] **Already resolved / verified:** Scratchpad launch override survives settings
  reload. (C2)

- [x] **Already resolved / verified:** Transcript delivery no longer explicitly saves
  and normalizes text under the user's caret. (C19; the general normalization resolution is recorded
  above)

- [x] **Already resolved / verified:** Wrapped H/M/L targeting; backward inclusive
  bracket operations; left/right line-edge operators; failed buffer-edge motions;
  explicit `1G`. (C21, C22, C23, C24, C25, C26)

- [x] **Already resolved / verified:** Vim n/N searches from the caret. (C27, Vim part)

- [x] **Already resolved / verified:** Wrapped vertical motion and per-cursor
  wrapped-row columns. (C38, C39)

- [x] **Already resolved / verified:** Vim page commands consume counts and cancel
  pending operators. (C40; remembering half-page counts is an agreed change above)

- [x] **Already resolved / verified:** X11 fallback respects keybinding overrides. (C58)

- [x] **Already resolved / verified:** WM close cannot stack a hidden quit prompt under
  Improve confirmation. (C82)

- [x] **Already resolved / verified:** October 23 fixes: deleted-injection remap crash
  and identified incorrect remaps, quadratic syntax work, stale modifier state,
  accidental plain q/w fallback, external settings overwrite, stale Vim search origins
  and accepted matches, operator ranges and cw, J/Find separators, endless horizontal
  reveal, autosave-cleared status, private-bus environment and tab-strip zoom height.
  Residual syntax and related cases are separately unchecked above.

- [x] **Already resolved / verified:** October 23 quadratic counted J and redundant
  Vim-search submit reindex were addressed within that fix batch. (23/EF2, EF6)

- [x] **Closed after review:** Rejected: pasted trailing field line breaks becoming
  spaces matches the documented normalization contract. (C3)

- [x] **Closed after review:** Rejected: invalid override entries still replace
  defaults, as documented. Displaying invalid entries more clearly remains optional
  polish. (C31)

- [x] **Closed after review:** Rejected: Vim reserves Ctrl+D/U/F/B in its motion modes
  intentionally. (C42)

- [x] **Closed after review:** Rejected: render-time focus is intentionally not forced
  back after an ordinary mouse focus change. Specific Dismiss/goto defects remain above.
  (C60)

- [x] **Closed after review:** Rejected: empty PID output cannot make the prompt-add
  process test pass silently. (23/V15)

- [x] **Closed after review:** Rejected: the claimed D-Bus startup readline hang did not
  hold up. (23/V14)

## Verification and evidence

### Implementation checkpoint: field sessions and observable acceptance (2026-10-25)

- Fresh field sessions clear local history; ordinary model synchronization preserves it.
- Acceptance cases now observe independent cursor goals, each held-key repetition,
  selection boundaries, retained cursor positions, modal persistence, Find focus,
  version-scoped Dismiss, and actual painted geometry. Trace schema 13 adds scrollbar
  and Settings category bounds; older optional trace fields remain readable.
- The 71-case focused nested run passes, as do the separate confirmation, Dismiss
  and Find checks. Three affected Settings/scrollbar cases also pass at 150% scaling.
  The new field-session test fails against the preserved pre-fix release binary.
- All 219 fast all-feature tests, 32 harness tests, Clippy and formatting pass (only
  existing warnings). The first close-prompt check used the wrong input helper and
  timed out waiting for an intentionally absent state change; the corrected case
  uses send_keys_settle plus the resize observation and passes. Full combined
  acceptance and runtime environment qualification remain pending.
- Evidence: `/home/holo/agent-exports/foundation-acceptance-2026-10-26` contains
  focused/scaled logs, the old-binary failure and fast/tooling results.
- The final save boundary also checks that an external version written after staging
  a replacement survives. The separate Keep Mine UI race and in-flight scratchpad
  quit coverage remain open; this narrower test does not claim to cover them.

### Implementation checkpoint: launch and X11 runtime boundaries (2026-10-25)

- Ten runtime/tooling entries implemented or audited; physical clipboard-owner
  hygiene assessed and documented. The broader acceptance and environment
  qualification gates remain pending at this implementation checkpoint.
- Four new real-app cases pass. Fast all-feature tests, harness tests, the ten
  Python tooling tests and the three standalone vendor timing tests pass; Clippy
  reports only existing warnings. All three vendor patches apply to the pinned
  archives and reproduce their source changes.
- Separate probes verify continued redraw without RandR, server-reported occlusion
  without Composite, and complete nested-runner teardown on SIGTERM and SIGHUP.
  The standard hidden-window test uses unmapping because Composite ARGB windows
  on this server continue to report unobscured visibility beneath another window.
- Evidence and final broad-gate results will be appended before this batch is
  considered accepted. Upstream alignment follows this batch; performance
  optimization remains deferred until the remaining foundation work is assessed.

### Implementation checkpoint: syntax and text presentation (2026-10-25)

- Addressed 19 checklist entries: the nearest-grapheme mouse contract; three
  residual syntax defects; tab reveal, caret motion, status, drag speed, wide
  wrapping and shaping context; text-layout invalidation; three coverage gaps;
  and four benchmark defects. This brings the ledger to 106 checked entries,
  with 34 nonoptional entries and 16 optional improvements still open.
- Each parse owns a complete injection inventory. Unchanged Tree-sitter subtrees
  reuse query results; changed containers are queried again, and non-rooted query
  patterns retain a full-query fallback. This replaces overlapping edit-proximity
  heuristics. Tests compare the cached inventory with an independent whole-tree
  query after every tested update. Fresh and incremental Markdown structure now
  share individual inline parses, removing the combined-inline special path.
- Text and highlight invalidation have separate owners, so distant color changes
  preserve unchanged text measurements. Actual lazy-cache checks cover injected
  edits and undo, including 256 fragmented edit/undo steps across four languages.
  A separate Markdown history exercises recreated section wrappers and Unicode
  shifts. Both retained structural counterexamples agree with fresh parsing.
- Counting, segmentation and cursor navigation share one wrapping traversal with
  Unicode grapheme cell widths. This is a monospace cell contract, not font-specific
  pixel wrapping. Shaped and cached hit testing share nearest-boundary semantics;
  cached glyph tokens preserve context across identifier/punctuation boundaries.
  Explicitly loaded JetBrains Mono probes compare glyph IDs, clusters and positions
  with full shaping; presentation screenshots were visually inspected.
- Six new real-input cases cover nearest-grapheme clicks, distant injected colors
  and undo, wide wrapping/navigation, resized-tab reveal, rendered status after
  resize, and held-drag speed/release. The preserved pre-change app reproduces the
  first, second and fourth defects. Its wide-text comparison stopped during resize
  setup and is inconclusive; its old computed status trace cannot verify rendered
  chrome. The below-document mouse case now distinguishes end-of-document placement
  from clamping to the last row at a large x coordinate.
- Both release binaries were measured with the same corrected, isolated benchmark
  runner, corpus, viewport and three measured repetitions after priming. Injected
  Markdown paste improved; large paste stayed close to baseline. Sustained Rust
  and injected-Markdown typing became slower. The measured absolute costs and
  individual runs are retained in the evidence bundle, including the earlier
  regression that prompted the subtree cache. These are host-specific comparisons,
  not a general performance claim. A separate traced run verifies that all 60 EOF
  navigation samples move and that hostile parent settings/recent files are untouched.
- Final verification: `cargo test --all-features` (217 passed), harness tests
  (32 passed), Clippy with existing warnings, and focused X11 checks (22/22, then
  6/6 including held drag). The full nested-X11 gate passed all 326 cases in
  2,759 seconds. Formatting and diff checks pass.
  The same temporary host-tool compatibility wrapper and software Vulkan are used;
  stock-runner compatibility and physical-display visual baselines remain outside
  this batch.
- Evidence: `/home/holo/agent-exports/foundation-presentation-2026-10-25` contains
  comparison metadata and logs, regression checks, the isolated syntax/font probes,
  profile-isolation evidence and screenshots.

### Implementation checkpoint: Vim semantics and input contracts (2026-10-25)

- Addressed all 13 retained Vim/shortcut defects, the three agreed Vim contracts,
  and oracle provenance: 17 checklist entries. The remaining findings stay in scope.
- Installed official stable Neovim 0.12.6 and the newest available nightly,
  `v0.13.0-dev-1946+g659164699f`, checking release-asset SHA256 digests. Stable is the
  pinned reference. All 912 previous oracle cases reproduce unchanged; 197 additions
  exercise counts, linewise cursor/register behavior, Unicode indentation, character
  finds and search-state transitions. Repeated and reversed-order generation agree
  with a deliberately failing personal config excluded. Nightly differs in four
  undo cursor positions, documented in `docs/vim.md`; text and registers agree.
- The model owns Vim's single-selection invariant and remembered viewport distance.
  Operator cursor placement is shared rather than patched at individual commands.
  The app removes released-modifier replay and puts optional shortcuts through the
  existing command registry. Root pointer capture cancels pending chord state.
- Seven focused X11 checks pass. Three fail against preserved `481dfdd`: mode entry
  with multiple selections, released Ctrl followed by plain Vim `d`, and an overridden
  Shift+Enter intercepting editor newline insertion. The corrected released-Ctrl test
  replaces an older test that explicitly expected the faulty replay behavior.
- Same-workload Criterion comparisons against the pre-change baseline: column-zero
  vertical motion improves 25–32% (1,024 motions per iteration); the delete/next-key
  workload improves about 3%, and whole-document delete/undo about 2%. Other measured
  workloads range from about 3% faster to 3% slower. Small copy/paste is about 1.8%
  slower (148.6 microseconds for 16 cycles); large word search is about 3% slower
  (7.7 milliseconds for 32 cycles). These are model microbenchmarks, not end-to-end
  typing claims. Expanded oracle replay is excluded from before/after timing because
  its workload changed. Logs retain both the initial tab-column regression and the
  final simple column-zero fast path.
- Final verification: `cargo test --all-features` (208 passed), harness tests
  (32 passed), Clippy, formatting and diff checks, and the full nested-X11 gate
  (320/320 passed in 2,790 seconds). Existing compiler/vendor warnings remain. The lane uses the same
  temporary wmctrl/xsel tools and compatible lwm wrapper as earlier checkpoints;
  the stock-runner compatibility item remains open.
- Evidence: `/home/holo/agent-exports/foundation-vim-2026-10-25` contains release
  provenance, oracle comparisons, reference scroll probe, regression logs and
  benchmark output.


### Implementation checkpoint: voice and prompt lifecycle (2026-10-25)

- Addressed the retained-result contract; voice recovery, timeout, background effects
  and focus (C8, C7, C17, C37, C10); review/close/goto interaction (C16, C4, C54, C87);
  worker/output boundaries and ownership documentation (C5, C9, C11, C18); and buffered
  WAV output (C14): 15 checklist entries. The remaining findings stay in scope.
- A single prompt state replaces three independently mutable flags/options. Reviews
  belong to their target document; visibility and focus follow the active tab and
  pending close/save decisions. Background transcript delivery uses the existing
  background model-update path without publishing PRIMARY.
- Voice requests stream asynchronously on an owned worker and cancel when dropped.
  Recording finalization failures retain recoverable bytes beside the note, while
  Finish still ends the session. Prompt process work uses a dedicated fallible worker,
  bounded output and bounded single-line diagnostics.
- Eight added real-app cases cover the interacting workflows. Three selected cases
  fail against the preserved pre-change binary: inactive-target result retention,
  refused blank-tab close preserving its file, and background status preservation.
  Four cases also pass with the app restricted to one CPU, including saves while
  prompt-add waits and both voice/conflict pointer suppression during review.
- Boundary checks cover cancellation releasing the HTTP connection and audio,
  finalization failure preserving sample bytes, transfer budgeting, runaway process
  output and simultaneous full pipes. Buffering produces byte-identical 96,044-byte
  WAV output with 14 underlying writes versus 48,003 for one second at 48 kHz.
  This is a write-count result, not an end-to-end performance claim; C13 remains open.
- Final verification: `cargo test --all-features` (205 passed), harness tests (32
  passed), Clippy, formatting and diff checks, and the full nested-X11 gate (314/314
  passed in 2,741 seconds). Existing compiler/vendor warnings remain. The nested lane
  used the documented temporary wmctrl/xsel tools and compatible lwm wrapper; the
  stock-runner compatibility item remains open.
- The inspected screenshot shows the active review with disabled voice and conflict
  controls. Logs, baseline failure states, the one-CPU launcher, write counts and the
  screenshot are in `/home/holo/agent-exports/foundation-lifecycle-2026-10-25`.


### Implementation checkpoint: settings persistence (2026-10-25)

- Closed configuration paths (C51), the six Settings findings (C48, C49, C29,
  C50, C32, C52), and malformed-reload coverage (23/B3-3): eight checklist entries.
  The remaining checklist stays in scope.
- The typed settings schema now drives targeted TOML updates instead of a second
  hand-maintained assignment list. Saves preserve unchanged formatting and comments;
  omitted defaults stay omitted. Reset and reload share application of settings, and
  reset commits once before applying defaults.
- Five added real-app regressions fail against the preserved pre-change binary.
  All 14 focused settings tests passed. The final reset/reload refactor passed the
  strengthened failure/recovery case with nondefault zoom, a launch override, and
  subsequent external reload. Full-suite results below validate the final code.
- The malformed-reload case now starts with nondefault wrap and cursor-limit values.
  Boundary tests cover inline-table preservation, path resolution, symlinks, mode
  preservation, read-only and non-file targets, dangling links, and temporary cleanup.
- The inspected screenshot shows the active preference and visible unsaved-change
  error together. The error remains above the results regardless of search/scrolling;
  the sidebar does not claim saving succeeded. No visual baseline was replaced.
- `cargo test --all-features`: 200 passed, zero failed. The harness's own suite passed
  all 32 tests. Formatting, whitespace checks, and `cargo clippy --all-targets
  --all-features` passed with existing warnings only.
- Full serial nested X11 lane: 306 passed, zero failed, with no retries. Production
  code stayed unchanged throughout the 2,670-second run. The run used the same
  temporary newer-lwm compatibility shim and software Vulkan as earlier checkpoints. Physical-display visual tests are outside this run; runner
  compatibility remains open. Logs, baseline results, screenshot, and the final patch
  are retained externally under `foundation-settings-2026-10-25`.

### Implementation checkpoint: captured Find scope (2026-10-25)

- Implemented the Find-in-Selection contract. The capture belongs to the document,
  includes every nonempty selection, and stays stable through navigation, query
  changes, edits, undo/redo, and retained alternate history branches. Switching tabs
  clears it. The remaining checklist stays in scope.
- Transactions own one invertible position map shared by selection, bookmark, and
  scope updates, then transferred to history. Single edits store their map inline;
  ordinary typing and deletion compact within undo groups. Saved ranges are released
  when their capture ends, preventing older scopes from reappearing.
- All six new real-app regressions fail against the preserved pre-change binary.
  Eight focused scope/replacement cases passed before the final map optimization;
  the full desktop gate validates the final implementation. Source invariants cover
  range union, both mapping affinities, inversion, compaction, cache normalization,
  and a disconnected old branch retaining empty scope until undo restores it.
- The inspected screenshot shows the enabled In Sel chip with a nonempty secondary
  selection and an empty primary caret. No visual baseline was replaced.
- Isolated CPU-pinned model benchmarks in both run orders show typing about 6%
  slower (approximately 0.05 microseconds per character) and 1,000-cursor deletion
  about 1% slower (approximately 1 microsecond per operation). These are model
  measurements, not end-to-end latency. Earlier larger regressions were removed by
  sharing the derived map and simplifying right-affinity lookup. Raw measurements,
  commands, host details, screenshot, and verification logs are retained in the
  external `foundation-scope-2026-10-25` evidence directory.
- `cargo test --all-features`: 197 passed, zero failed. The harness's own suite
  passed all 32 tests. Formatting, whitespace checks, and `cargo clippy --all-targets
  --all-features` passed with existing warnings only.
- Full serial nested X11 lane: 301 passed, zero failed, with no retries. Production
  code was unchanged throughout the 2,633-second run. The run used the same temporary newer-lwm compatibility shim and software Vulkan
  as the earlier checkpoints. Physical-display visual tests remain outside this run;
  the runner compatibility finding remains open.

### Implementation checkpoint: line boundaries and Find navigation (2026-10-25)

- Closed the logical-line motion/editing defects (C55, C85, 23/AL4), duplicate
  caret/current-line rendering (C45), Standard Find navigation (C27), and
  Find-test gaps AT5, AT7, and T2. These are six checklist entries; related
  candidate IDs share resolutions. The remaining checklist stays in scope.
- The line-content rule now has one editor-domain owner. Display, wrapping,
  search, selection bounds, and line-oriented edits use that rule. Standard
  Find navigation shares the caret-relative path already used by Vim.
- All five new real-app regressions fail against the preserved pre-change
  binary and pass after the fixes. The F3 harness escape was added before the
  valid baseline comparison; the earlier unsupported-escape failure is not
  counted as evidence of an editor defect. The strengthened selected-match
  assertion is included in the full run.
- Five optional duplication suggestions have explicit dispositions above:
  line-content logic is shared, while coordinate adapters, width adapters,
  syntax windows, and wrap-update branches retain their distinct contracts.
- The inspected separator screenshot shows one current-line highlight and
  one caret. The acceptance case also checks geometry and redraw quiet with
  smooth cursors enabled; no visual baseline was replaced.
- An isolated, CPU-pinned model benchmark comparison in both run orders finds
  typing within noise and unwrapped page navigation about 4% faster. Correct
  caret-relative Find stepping is about 26–27% slower in the isolated loop,
  approximately 0.3 microseconds more per step. This is not an end-to-end
  latency measurement. Commands, host details, raw results, the screenshot,
  and verification logs are retained in the external
  `foundation-navigation-2026-10-25` evidence directory.
- `cargo test --all-features`: 193 passed, zero failed. The harness's own
  suite passed all 32 tests. Formatting, whitespace checks, and `cargo clippy
  --all-targets --all-features` passed with existing warnings only.
- Full serial nested X11 lane: 295 passed, zero failed, including the strengthened
  field-geometry, redo, replacement-count, and selected-match assertions. Production
  code was unchanged throughout the full run.
- The run used the same temporary newer-lwm compatibility shim and software
  Vulkan as the earlier checkpoints. The two physical-display visual tests
  were outside this run; the runner compatibility finding remains open.

### Implementation checkpoint: saving and bookmark mapping (2026-10-25)

- Closed C89, C88, C81, save-normalization selection mapping, and the actual-tab
  trimming coverage gap (23/B3-5). The remaining checklist stays in scope.
- Save normalization is owned by `lst-editor` and reaches the document as one
  undoable transaction after a successful, current write. The desktop runtime
  shares one guarded write path for explicit saves and unchanged autosaves.
- `cargo test --all-features`: 190 passed, zero failed. `cargo clippy
  --all-targets --all-features` passed with existing warnings; formatting and
  whitespace checks passed.
- Full serial nested X11 lane: 290 passed, zero failed. After tightening the
  dirty-buffer preconditions in the three new save tests, all three were
  rerun successfully. Production code was unchanged from the full run.
- All four new acceptance regressions fail against the preserved pre-change
  binary. The three strengthened save cases were also checked against that
  binary and still fail for the original defects.
- The full run used the same temporary newer-lwm compatibility shim and
  software Vulkan as revalidation. Physical-display visual tests were outside
  this run. A concurrent-display experiment was cancelled because the harness
  serializes sessions with a global lock; the reported full result is serial.
- An isolated, CPU-pinned Criterion comparison of
  `editor_typing/type_chars/small` measured about 1–2% slower typing in both run
  orders (roughly 272–274 µs before and 276–278 µs after per 320-character
  sequence). This is a small measured cost, not a speedup or an end-to-end
  latency claim. The first comparison overlapped desktop tests and was discarded.
  This measured cost was accepted for the correctness and maintainability gains.

### Revalidation baseline (`a9a01e0`, 2026-10-25)

- `cargo test --all-features`: 185 passed, zero failed; 288 display-dependent
  tests are ignored in this lane.
- Nested real-X11 acceptance: 286 passed, zero failed. The two physical-display
  visual tests were outside this run.
- The nested run used the production app and unchanged repository tests. Local
  tools and a temporary newer-lwm compatibility shim were required; software
  Vulkan supplied the presentation backend. The stock-runner limitation is
  separately listed above.
- Fresh probes cover the outstanding model, settings, syntax and I/O cases,
  plus real-app shortcut, modal, focus, hidden-window and input-history cases.
  Probes that print a discrepancy can exit successfully: their output, not
  their exit code alone, establishes the defect.
- No production fixes were made during this revalidation. Performance claims
  have not been upgraded from historical or structural evidence to new
  end-to-end benchmark results.

The detailed JSON, probe sources, trace/log evidence and transcript exports are
supporting snapshots. This Markdown file is the human-readable checklist.

Original Claude sessions:

- October 23: `dde6aba0-c0c3-4599-bbd7-3777dfecfd80`, background review
  `a1c265f45df9f3762`; fixes committed as `2007639`.
- October 24: `a0cff1d9-a787-4bc4-80ee-25aca58c8ad0`, background review
  `a39130628f8913821`; fixes committed as `a9a01e0`.

The background transcripts exported with `agent-export --include-tools` contain
investigations omitted from the short parent-session summaries. Source
identifiers above refer to their candidate ledgers and verifier results.
