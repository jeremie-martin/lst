# Findings and agreed changes

Address applicable findings from the October 23 and 24 reviews and the
subsequent reconciliation. The desired result is a correct, simple, lean,
maintainable and efficient editor. This checklist describes issues and agreed
behavior; it does not prescribe an implementation or an order of work.

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

- [ ] **Standard F3 / Shift+F3:** find the next/previous match from the current caret,
  wrapping once. Advance beyond an already selected active match and handle empty regex
  matches without getting stuck. Matches on lines 0, 2 and 4: from line 2, F3 goes to 4
  and Shift+F3 goes to 0. (C27, Standard-mode remainder)
  Review: Fresh model probe still advances from the old active index: with the caret on
  line 2, FindNext returns line 2 rather than line 4.

- [ ] **Supported Vim commands:** match a pinned Neovim reference for the supported
  single-cursor commands, including counts, register types, failed motions and resulting
  cursor positions. Document intentional exceptions.

- [ ] **Vim Ctrl+D/U:** remember an explicit count for the current editor viewport,
  following the reference's resize behavior. Initially use half a viewport. Ctrl+F/B
  counts remain page counts. With enough text, `5<C-d><C-d>` moves five lines twice.
  Review: Fresh model probe with eight viewport rows: 5<C-d><C-d> reaches line 9,
  showing that the second movement uses the default four lines.

- [ ] **Vim and multiple cursors:** entering Vim keeps the primary selection and removes
  secondary selections. Adding cursors in Vim is unavailable with a visible explanation.
  Single-selection Visual mode remains supported.

- [ ] **Find in Selection:** capture all nonempty selected ranges in the active tab.
  Search only within their union, leaving intervening text untouched. Search navigation
  must not redefine the captured scope. Keep the scope aligned through edits and
  undo/redo; switching tabs clears it. Disable activation without a nonempty selection.
  Matches must fit wholly within one range; empty regex matches must not be duplicated
  at shared boundaries.
  Review: Current FindScope captures one primary range with fixed offsets. A fresh
  insertion leaves that captured range unchanged; all-selection, edit-tracking behavior
  is additional agreed work.

- [ ] **Mouse placement:** choose the nearest grapheme boundary consistently for shaped
  and cached text. At an exact midpoint choose the earlier boundary. Clicking the right
  half of the final character places the caret after it. (C62)

- [ ] **Configuration paths:** expand a leading `~/`, require custom note directories to
  resolve to absolute paths, and display invalid-path errors in Settings. General shell
  and environment-variable expansion is unsupported. The destination must be independent
  of the launch working directory. (C51)

- [ ] **Prompt results and tab switches:** keep an improvement result available when its
  target document is unchanged, even if another tab is active. Apply must still refuse a
  result whose target document changed. (October 24 final sweep)
  Review: runtime.rs apply_cleanup_result still rejects a result solely because another
  tab is active.

## Reconfirmed defects

Unchecked entries are applicable problems. Conditions and limits on the evidence
are noted where they matter. Resolutions name their regression coverage.

### Text, line endings and saving

- [ ] **Reproduced:** End and Shift+End cross into the following line on VT/FF/NEL/LS/PS
  terminators; Left and Backspace can get stuck at the following line's start. Related
  Vim commands, including `$`, `A`, `D` and word operators, can join lines or edit the
  wrong line. (C55, C85; 23/AL4)
  Review: Current model probes fail for all five named separators; LF and CR controls
  pass. selection.rs display_line_char_len and line_display_text only exclude CR/LF.

- [ ] **Source-confirmed:** The caret and current-line highlight can appear on both
  sides of those separators. With smooth cursors enabled, animation can keep redrawing.
  (C45)
  Review: viewport.rs marks the separator-containing row inclusive at its end while the
  next row starts at that same character. Both row membership tests accept the caret.
  Animation consequences remain conditional on smooth_cursor.

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

- [ ] **Reproduced:** Injected highlights stay stale after edits outside the narrow
  refreshed window: Markdown code fences, multiline inline spans and HTML script/style
  content are affected. (C80; 23/A3-3)
  Review: Reran the saved cases with the current production syntax module. The
  partial-cache probe and JS/Markdown structure comparisons still differ from fresh
  results. Rust host highlighting provides a passing control.

- [ ] **Reproduced:** The remaining JavaScript edit history produces incremental bracket
  structure different from a fresh parse. Reproduced after the first review's fixes.
  (23/A3-4 residual JavaScript counterexample)
  Review: Reran the saved cases with the current production syntax module. The
  partial-cache probe and JS/Markdown structure comparisons still differ from fresh
  results. Rust host highlighting provides a passing control.

- [ ] **Reproduced:** The remaining Markdown backtick example also produces incremental
  bracket structure different from a fresh parse. This is separate from stale colours.
  (23/A3-4 residual Markdown counterexample)
  Review: Reran the saved cases with the current production syntax module. The
  partial-cache probe and JS/Markdown structure comparisons still differ from fresh
  results. Rust host highlighting provides a passing control.

### Settings

- [ ] **Reproduced:** Saving an unrelated setting removes inline comments and value
  formatting. (C48)

- [ ] **Reproduced:** Saving an unrelated setting removes comments and commented-out
  entries from `[keybindings]`. (C49)

- [ ] **Reproduced:** A failed reset changes the settings store before writing succeeds,
  leaving the store, running app and disk inconsistent. (C29)

- [ ] **Source-confirmed:** Save/reset errors are hidden behind Settings while the page
  says “Saved automatically.” Applied-but-unsaved settings need visible feedback. (C50)

- [ ] **Reproduced:** Atomic settings writes mishandle symlinks, permissions or
  temporary-file cleanup. (C32)
  Review: Current settings.rs changed a 0600 file to the temporary file default mode,
  replaced a symlink without updating its target, and left the temp file after a failed
  rename. Permission/cleanup behavior is actionable; decide intended symlink semantics
  explicitly before changing it.

- [ ] **Reproduced:** Empty/relative `XDG_CONFIG_HOME` is used without validation. (C52)
  Review: The current config-path helper returns lst/config.toml for empty
  XDG_CONFIG_HOME and a cwd-relative path for relative values. This is a
  configuration-boundary issue.

### Voice

- [ ] **Source-confirmed:** Finalization errors can still delete captured audio.
  Sample-write recovery was added, but `finalize()` can return early and drop the
  recording. Retain recoverable bytes and accurately report whether transcription is
  possible. (C8, partially fixed)
  Review: Current capture still uses finalize()? with the NamedTempFile owned locally.
  An isolated faulting-WAV boundary reproduces finalization failure followed by file
  deletion; native device capture was not fault-injected.

- [ ] **Source-confirmed:** The fixed 120-second upload deadline cannot accommodate long
  segments on slow links. Repeated retries can fail while blocking all later segments.
  (C7)
  Review: Provider uses a 120-second total request deadline; capture permits 15 minutes
  of device-rate, 16-bit mono PCM. That payload can require longer than the deadline on
  a slow uplink. No new slow-link throughput measurement was made.

- [ ] **Source-confirmed:** Transcript delivery clears unrelated status messages and can
  reclaim X11 PRIMARY despite being a background update. (C17)
  Review: poll_voice calls update_model, which clears cleanup_message and compares
  PRIMARY identity using the changed revision, even when selected text is unchanged.

- [ ] **Reproduced:** Dismissing a voice message can leave keyboard focus on an
  invisible root surface, so typing appears unresponsive. (C37)

- [ ] **Reproduced:** Capture failure after Finish can restore Paused and keep closing
  blocked. (C10)
  Review: Included current Session against a faulting recorder boundary: Finish plus
  capture error leaves done=false and can_toggle=true; a second Finish is needed. This
  is a session-state reproduction, not a real microphone failure.

### Prompts, closing and focus

- [ ] **Source-confirmed:** Voice controls remain interactive during prompt review. Show
  Note can switch tabs and make an otherwise usable review stale; the conflict banner
  has a related interaction gap. (C16)

- [ ] **Reproduced:** A tab can enter its close flow while prompt improvement is
  running; the arriving result can coexist with the close prompt and leave a stale
  review covering another tab. (C4)
  Review: Real app with a gated prompt-add process: closing the dirty tab opens
  close_prompt; releasing the result opens prompt_review too. Both remain present, with
  focus on close_prompt.

- [ ] **Reproduced:** Closing the last blank scratchpad during improvement can delete
  its file before quitting is refused, leaving an open tab with a missing-file warning.
  (C54)
  Review: Real app with a whitespace scratchpad and gated improvement: Ctrl+W deletes
  the backing file, keeps the tab open and reports it missing.

- [ ] **Reproduced:** Clicking the goto bar's label or padding passes through to
  document text, moves the caret and redirects subsequent typing into the document.
  (C87)

### Vim and shortcuts

- [ ] **Reproduced:** Normal/Visual counted `$` ignores the count. `2$` stays on the
  current line instead of reaching the following line. Operator counted `$` was fixed
  separately. (C28)
  Review: Fresh public-model probes reproduce the stated text/cursor/search discrepancy;
  the current Vim regression suite still passes, so those cases need coverage.

- [ ] **Reproduced:** Backward linewise yanks leave the cursor on the wrong line. (C67)

- [ ] **Reproduced:** Some deletes converted to linewise leave the cursor at column zero
  instead of the reference's first nonblank or preserved column. (C68)

- [ ] **Reproduced:** `^` and related first-nonblank motions land past whitespace-only
  lines, causing subsequent commands to do nothing or act on the wrong blank. (C90)

- [ ] **Reproduced:** Unicode indentation can cause `db` and related operators to become
  linewise and delete more text than intended. (C83)
  Review: Fresh public-model probes reproduce the stated text/cursor/search discrepancy;
  the current Vim regression suite still passes, so those cases need coverage.

- [ ] **Reproduced:** Editing forgets the previous `f/F/t/T`, so `;` and `,` cannot
  repeat it.

- [ ] **Reproduced:** `*`/`#` leave whole-word/case/regex flags affecting subsequent `/`
  and `?` searches, silently excluding matches. (23/V9)
  Review: Fresh public-model probes reproduce the stated text/cursor/search discrepancy;
  the current Vim regression suite still passes, so those cases need coverage.

- [ ] **Reproduced:** An Improve Prompt shortcut appears in Settings/palette but is
  never bound. (C15)

- [ ] **Reproduced:** A `find.previous` override containing Shift+Enter is bound in the
  wrong context and can intercept newline insertion in the editor. (C41)

- [ ] **Reproduced:** The released-Ctrl workaround can replay Vim Ctrl+D/U/F/B for
  ordinary letters within its timing window. The plain `q`/`w` issue was fixed
  separately. (C53)
  Review: Real X11 input: release Ctrl, then plain d in Normal mode moves from line 3 to
  line 25 with no pending operator.

- [ ] **Source-confirmed:** Custom arrow bindings accepted with nonlowercase spelling
  can be ignored by held-arrow handling. (C84)

- [ ] **Source-confirmed:** A pending Ctrl+K prefix survives pointer interaction,
  unexpectedly turning a later Ctrl+D into Skip Next Occurrence. (23/AL7)

- [ ] **Reproduced:** Extremely large Vim counts can overflow or reverse direction;
  recheck after the committed motion changes. (C56)
  Review: From line 2 of a four-line document, 18446744073709551615dd panics at
  vim_engine.rs:199 in debug. j/k counts are now bounded; the broad old
  direction-reversal claim was narrowed to remaining arithmetic paths.

### Rendering, runtime and boundaries

- [ ] **Reproduced:** Non-UTF-8 command-line arguments can panic during launch. (C1)

- [ ] **Reproduced:** The build script repeatedly recompiles the app when its watched
  Git ref is packed or belongs to a linked worktree. (C30)
  Review: A throwaway Git/Cargo fixture using current build.rs rebuilds because
  .git/refs/heads/main is missing after pack-refs. The linked-worktree path also joins
  refs under the wrong Git directory; that part was source-checked.

- [ ] **Reproduced:** Non-ASCII window titles are displayed incorrectly. (C59)
  Review: Real X11 launch with a Japanese/en-dash --title has no _NET_WM_NAME and its
  WM_NAME contains UTF-8 bytes decoded as Latin-1. This affects launch overrides, not
  ordinary title updates.

- [ ] **Source-confirmed:** Tab-switch cursor reveal can use old geometry and finish
  while the caret is still off-screen after resizing or zooming another tab. (C61)
  Review: Per-tab geometry survives invalidation. The immediate reveal reads that old
  geometry and can clear the pending reveal before the new prepaint writes current
  bounds.

- [ ] **Source-confirmed:** Smooth Vim carets can snap because tiny floating-point size
  differences reset their animation. (C46)
  Review: Motion::retarget resets on exact size inequality. Widths formed by subtracting
  two floating-point positions need not be bitwise equal across columns. The symptom
  depends on font, zoom and position.

- [ ] **Source-confirmed:** Geometry-dependent status-bar details can remain stale until
  another unrelated event. (C65)
  Review: Status props are collected before viewport prepaint updates geometry;
  correction notifies during prepaint. The current GPUI notify path marks dirty_views
  there without scheduling an ordinary subsequent draw. Existing verifier probe supports
  the same mechanism.

- [ ] **Reproduced:** Post-input drawing can refresh hidden/obscured windows during
  activity elsewhere. The verifier rejected the destroyed-window part of this claim.
  (C69)
  Review: On a private Xvfb server, a root modifier event causes another app frame after
  the window is unmapped and its trace is quiet. Window attributes still report
  IsUnmapped.

- [ ] **Source-confirmed:** Drag autoscroll speed depends on mouse event rate and
  changes when the refresh rate drops after inactivity. (C74)
  Review: Document drag autoscroll applies a fixed distance per callback. Dirty
  post-input draws also run next-frame callbacks; there is no elapsed-time
  normalization. The magnitude under different devices was not remeasured.

- [ ] **Reproduced:** Reopening an input field with unchanged text preserves undo
  history from its previous use. (C86)
  Review: Real app: type abc, erase it, close/reopen the palette, then Ctrl+Z. The
  screenshot shows a restored a in the newly opened palette.

- [ ] **Source-confirmed:** SIGTERM/SIGHUP can leave nested-display child processes,
  including the private D-Bus daemon, running. (23/V13)
  Review: The entry points install no SIGTERM/SIGHUP handlers. Children use separate
  process sessions and normal cleanup is in the context manager/KeyboardInterrupt path,
  which default termination bypasses.

- [ ] **Source-confirmed:** Thread exhaustion can panic through infallible thread
  spawning. (C5)
  Review: The startup warm-up still calls std::thread::spawn, which panics on
  thread-creation failure. That optional warm-up should not make launch fail. Resource
  exhaustion was not induced on this host.

- [ ] **Source-confirmed:** Prompt-add output/error buffering is unbounded. (C9)
  Review: read_chunk appends without an output bound until the deadline. Error/warning
  text is also retained wholesale and shown in a single-line surface. A deadline does
  not bound memory; no destructive exhaustion test was run.

- [ ] **Source-confirmed:** Blocking prompt-add occupies a shared background-executor
  worker. (C11)
  Review: The blocking process exchange runs on the shared executor. Its worker count
  can be one; in that supported configuration, a waiting prompt can delay
  saves/maintenance for its full deadline. No starvation was induced in the daily app.

- [ ] **Source-confirmed:** The contributor guide still describes an app-owned DeepSeek
  client. (C18; stale wording observed)
  Review: AGENTS.md still calls this an app-owned DeepSeek client; current prompt
  improvement invokes prompt-add and voice uses ElevenLabs. This is stale ownership
  documentation.

- [ ] **Reproduced:** Wide CJK characters count incorrectly in wrapping. (C57)
  Review: Current wrapping counts each non-tab grapheme as one cell. Current GPUI
  shaping with JetBrains Mono produces an 80-character CJK row wider than the computed
  80-cell viewport; the default-font control does not show it.

- [ ] **Source-confirmed:** Cursor animation does not reset on some font/gutter/wrap
  changes. (C63)
  Review: MotionContext resets on tab/bounds/scale/row height, but omits some layout
  changes. Typography invalidation clears caches without resetting motion. The
  documented layout-reset contract is broader; visible impact was not freshly recorded.

- [ ] **Reproduced:** Glyph-cell tokenization loses contextual alternates across
  classes. (C64)
  Review: With JetBrains Mono, 12:30 has different full versus token-shaped glyphs; 1+2
  and the installed default font agree. The current actual cell builder accepts the
  affected fast path. Narrow the claim to demonstrated font/text combinations.

- [ ] **Source-confirmed:** Degenerate monitor timing can yield a zero refresh period.
  (C77)
  Review: mode_refresh_rate guards zero fields but not a zero computed millihertz or
  microsecond period. Representable mode values can divide by zero or yield a
  nonadvancing timer. No real monitor emitting such data was observed.

## Confirmed costs

The work below exists, but its current end-to-end impact has not been measured.
It is a reason to investigate an appropriate scenario, not an instruction to
add caching, pooling or another layer.

- [ ] **Cost confirmed; impact unmeasured:** CRLF search uses a slower indexing path.
  The first review's optimization was deliberately reverted to stay within its report
  limit. (23/EF3)
  Review: CRLF chunks still take Ropey line parsing rather than the LF-only fast path.
  The historical slowdown is not a fresh timing result; an optimization needs a current
  controlled comparison.

- [ ] **Cost confirmed; impact unmeasured:** Tab-label construction repeatedly scans all
  tabs and allocates labels, producing quadratic work per render. (23/EF4; October 24
  EF8)
  Review: Every tab label scans all other tabs during prop construction, even when the
  cached child is reused. Quadratic work is present; material cost depends on tab count.

- [ ] **Cost confirmed; impact unmeasured:** Syntax changes can unnecessarily remeasure
  unchanged layout through the end of a large construct. (23/AL2; October 24 EF1)
  Review: SyntaxInvalidation::Lines supplies a syntax-widened window to the layout
  caches. The unnecessary remeasurement mechanism remains; no new end-to-end timing was
  taken.

- [ ] **Cost confirmed; impact unmeasured:** Wrapped H/M/L scans the lines above the
  viewport on every invocation, making its cost grow with document depth.
  Review: screen_lines walks from line zero to the viewport in wrapped documents. Linear
  work is present; measure its actual latency before choosing a change.

- [ ] **Cost confirmed; impact unmeasured:** Background polling, blinking, PRIMARY
  publication, retained Find queries and per-request HTTP-client creation were reported
  as unnecessary work. Their current costs and useful behavior need checking. (October
  24 EF2-EF7)
  Review: Background work is present, but some is necessary for live meters, maintenance
  and searches. Separate measured redundant work from useful behavior; the current
  evidence does not justify removing those mechanisms wholesale.

- [ ] **Cost confirmed; impact unmeasured:** Allocation occurs inside the realtime
  microphone callback. (C13)
  Review: downmix allocates its PCM Vec in the realtime callback. The allocation exists;
  no overrun attributable to it was reproduced. Measure before introducing buffer-pool
  machinery.

- [ ] **Cost confirmed; impact unmeasured:** Unbuffered WAV output performs many tiny
  writes. (C14)
  Review: Current hound/File output performs 48,000 sample writes for one second at 48
  kHz, confirmed with a counting I/O boundary. This establishes excessive write
  granularity, not a measured end-to-end latency regression.

## Test and benchmark findings

These are defects or specific gaps in verification. The passing baseline does
not demonstrate the missing observations.

- [ ] **Source-confirmed:** Close, cleanup-confirmation and Settings tests can accept
  premature dismissal as successful ignored-key handling. (23/B3-1; October 24 AT1, AT2,
  T4)

- [ ] **Source-confirmed:** Multi-cursor goal tests give every cursor the same goal, so
  they cannot prove independent preferred columns. (23/B3-2)

- [ ] **Source-confirmed:** Malformed-settings reload coverage uses default values and
  cannot prove that nondefault active settings survive. (23/B3-3)

- [ ] **Source-confirmed:** The below-document click test clicks far right of the last
  short line, making two different hit-test behaviors indistinguishable. (23/B3-4)
  Review: The helper clicks the horizontal center of a wide viewport below a
  seven-character final line. Both end-of-document and x-clamped-last-row behavior
  return the same position.

- [x] **Resolved (2026-10-25):** Save-trimming tests typed Tab keys that inserted spaces; actual
  tab characters were untested. (23/B3-5)
  Resolution: the mixed-ending save regression seeds actual tab characters in its
  file fixture and checks the saved bytes, undo, and redo.

- [ ] **Source-confirmed:** Language-to-syntax mapping coverage was removed for several
  languages. Language detection tests do not cover that separate mapping. (23/B3-7)

- [ ] **Source-confirmed:** Held-chord repetition tests cannot distinguish three
  successful presses from one. (23/B3-8; October 24 AT4)

- [ ] **Source-confirmed:** The Keep Mine test never introduces a newer disk version
  after the conflict banner, leaving the version guard untested. (October 24 AT3)

- [ ] **Source-confirmed:** Local field redo is overwritten before its result is
  asserted. (October 24 AT5)

- [ ] **Source-confirmed:** Dismiss tests cannot prove suppression lasts for the
  observed disk version, and no longer check that Dismiss preserves external file
  contents. (October 24 AT6, T7)

- [ ] **Source-confirmed:** Find-field mouse tests may use caret geometry from before
  the preceding key action. (October 24 AT7)
  Review: caret_point accepts any collapsed caret with geometry, including a record
  predating Home/Right. It does not pin the requested field offset.

- [ ] **Source-confirmed:** Horizontal-scroll tests mix logical and physical coordinates
  and may mistake an unprocessed click for the rightmost scroll position. (October 24
  AT8)
  Review: The helper scales only the inset, rather than the whole logical coordinate,
  and treats an unchanged offset as proof that scrolling reached the end. The current
  scale-one suite does not exercise the scaling difference.

- [ ] **Source-confirmed:** Coverage no longer checks that multiline selections ending
  at column zero exclude that final line when indenting or adding line-end cursors.
  (October 24 T1)

- [ ] **Source-confirmed:** Replace All coverage no longer checks that Find results
  refresh after replacement. (October 24 T2)

- [ ] **Source-confirmed:** Quit-review coverage no longer checks input blocking while
  scratchpads are saving. (October 24 T3)
  Review: QuitReview::is_running includes saving_scratchpad, but current tests do not
  hold that phase while sending cancellation input. The guard is present; this is
  coverage, not a confirmed quit bug.

- [ ] **Source-confirmed:** Multi-cursor indent coverage checks text but not retained
  cursor count. (October 24 T5)

- [ ] **Source-confirmed:** Ctrl+F while Search is focused with Replace open is no
  longer covered. (October 24 T6)

- [ ] **Source-confirmed:** Trace-schema checks leave optional input records empty, so
  nested field mismatches escape the contract check. (C33)
  Review: Optional nested field records remain None in the trace contract fixture, so
  their nested keys never enter field_paths. App/harness field definitions currently
  agree.

- [ ] **Source-confirmed:** Partial syntax-cache checks miss the injected-language cases
  that still produce stale visible results.

- [ ] **Source-confirmed:** The 23 new oracle rows need verification against the
  intended pinned Neovim version/options. All 889 older rows are unchanged; metadata
  moved from 0.12.5 to the locally installed 0.9.5.
  Review: Compared fixture JSON again: all 889 older rows are unchanged; 23 additions
  and the metadata use local Neovim 0.9.5. Version/config provenance needs resolving,
  not automatic fixture regeneration.

- [ ] **Source-confirmed:** Settings tests use fixed, unscaled pixels; the older-schema
  find helper also estimates field positions. (23/CV2, CV3)
  Review: Current settings tests contain unscaled pixel assumptions. The estimated
  find-field fallback is limited to older schemas and is intentional compatibility
  behavior; that part is not a current-schema defect.

- [ ] **Source-confirmed:** Navigation benchmarks at `--position end` mostly time no-op
  Down keys. (October 24 H1)
  Review: operation_line(End) starts near EOF and the latency loop always presses Down,
  sixty times. Most samples become no-ops even though frames are still produced.

- [ ] **Source-confirmed:** Benchmarks inherit personal settings and write temporary
  corpus files into the real recent-files list. (October 24 H2)
  Review: spawn_editor sets display/trace variables but inherits HOME and XDG paths;
  ordinary startup records corpus paths in recent files. This is a real isolation
  problem.

- [ ] **Source-confirmed:** Benchmark medians can mix viewport sizes while reporting the
  first run's size. (October 24 H5)
  Review: A window-size mismatch only warns and both sizes still enter the same median;
  the reported size can come from the unmeasured priming run.

- [ ] **Source-confirmed:** Paste timing includes trace reading/parsing before input
  injection. (October 24 H6)
  Review: The paste start timestamp precedes read_editor_trace, so unrelated trace I/O
  is included before Ctrl+V is injected.

- [ ] **Source-confirmed:** Killing a test while arrow keys are held can leave them
  pressed for later tests; existing stale-key cleanup releases only modifiers. (October
  24 H8)
  Review: HeldKeys releases in Drop only; signal termination skips it. The next test
  releases modifiers only, not held arrows. The server-side stuck-key case is relevant
  to teardown even when ordinary tests pass.

- [ ] **Source-confirmed:** The stock nested runner is incompatible with this host's
  installed lwm command line and configuration schema.
  Review: It passes the fixture as a positional argument; current lwm requires --config.
  The fixture also contains the removed workspaces.count key. The passing audit used an
  external shim and a temporary fixture, not a repository fix. State the supported
  version or resolve compatibility.

## Claims requiring evidence before implementation

These eight claims remain unconfirmed in the stated sense. Establish an
applicable failure, a required contract or a material measurement error before
changing code. Rejection is a valid outcome.

- [ ] **Needs evidence:** The claim that every diff hunk needs another lst patch marker
  is not established by the repository rule. Check logical patch coverage before adding
  vendor comments. (23/CV1, CV5; October 24 K1, K3)
  Review: The reported missing-hunk counts overstate the evidence. Existing field/helper
  comments mark the logical cache/scanner changes, and manifests describe them. Identify
  a logical change without a covering marker before requiring repetitive vendor
  comments.

- [ ] **Needs evidence:** Clipboard preservation checks may finish before a late
  clipboard owner overwrites the selection. This race remains plausible. (23/B3-6)
  Review: There is a possible interval between spawning the clipboard owner and
  selection acquisition. No wrong result was reproduced; do not add a new
  synchronization protocol solely on this claim.

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

- [ ] **Needs evidence:** RandR failure can leave the app without a refresh timer. (C78)
  Review: The screen-resource error branch returns without installing a refresh timer.
  End-to-end reachability/impact after successful window creation was not reproduced;
  keep this as a specific display-failure case to establish.

## Optional improvements

These are reviewed opportunities, not required fixes. Some duplication is real,
but shared code may be more complex or join boundaries with different semantics.
No blanket refactoring or new subsystem is implied.

- **Optional:** Passive-occurrence refresh is duplicated during focus handling and can
  recreate a query that was just cleared. (23/S4)
  Review: The refresh can run both for a focus callback and an explicit editor-focus
  request. Some requests intentionally restore passive highlights. No new harmful case
  was established; removing the tail unconditionally could change useful behavior.

- **Optional:** Width remeasurement duplicates paths and line-change calculations.
  (23/S2)
  Review: The repeated LineChange validation and two width-measurement paths remain.
  They handle borrowed lines and changed topology differently. Duplication alone does
  not justify a new abstraction.

- **Optional:** Tab strip and status bar duplicate cached-view plumbing. (23/S3, R6;
  October 24 S5)
  Review: Cached-view plumbing remains similar, but each view has different props and
  layout. Share only if that actually reduces production complexity.

- **Optional:** Input fields duplicate grapheme-boundary rounding. (23/S5, R7)
  Review: These helpers operate at different boundaries: field UTF-8 byte offsets versus
  Rope character offsets. They are not interchangeable copies. No behavior defect
  follows just from having both.

- **Optional:** Unused theme-button trace state gives no evidence that a theme button is
  absent. The theme-application assertion itself remains meaningful. (23/S6)
  Review: The trace field is always None. Its absence assertion is vacuous, but the same
  test still checks theme application and can fail for that. Cleanup should preserve
  trace compatibility.

- **Optional:** Find repeats match-position conversions. (23/S7)
  Review: Repeated Position construction is short and has no demonstrated invalid state
  or measurable cost. A conversion helper is optional, not required remediation.

- **Optional:** Benchmark trace truncation/polling logic is duplicated. (23/S8)
  Review: The benchmark has repeated trace/polling scaffolding. No independent defect
  was established; timing correctness is tracked separately below.

- **Optional:** Test helpers duplicate input synthesis already owned by the harness.
  (23/R5, CV4)
  Review: Some held-key and drag helpers duplicate harness operations, while also owning
  test scope/cleanup. No blanket migration is justified. The killed-test key-release
  defect is separate.

- **Optional:** Settings tests duplicate trace-to-window scaling. (23/R8)
  Review: Coordinate conversion appears in multiple test helpers. The actual scaling
  error is tracked separately; sharing helpers is optional.

- **Optional:** GraphemeCell's increased size adds per-cell memory cost. Assess its
  impact; the suggested narrower integer representation is not a correctness
  requirement. (23/EF7)
  Review: Current GraphemeCell is 32 bytes on this 64-bit target. Narrowing offsets
  introduces representable-size constraints; no current benefit justifies that choice by
  itself.

- **Optional:** Some tests assert transaction shape or place behavior coverage in a lane
  that does not match the repository testing rules. (October 24 K4, K5, K8)
  Review: The public BufferDelta replay test is also a real consumer-equivalence
  contract, not simply an internal transaction-shape test. The source-side regex
  replacement test is a workflow under the repository lane policy; review individual
  coverage/placement without mass relocation.

- **Optional:** Additional duplicated logic was reported in code-line builders,
  command-modifier declarations, settings metadata, line-edit builders and invalidation
  sequences. Assess each concrete duplication without presuming a new abstraction.
  (October 24 S4, S6-S8, K6, R8)
  Review: Similar builders/switches remain, but no independent failure was established
  for the broad cleanup group. Specific shortcut, invalidation and metadata defects
  retain their own entries.

- **Optional:** Physical-display test clipboard sentinel owners are not cleaned up.
  (October 24 H7)
  Review: Physical-display tests intentionally replace selections. The last X11 owner
  must stay alive to serve its clipboard; merely killing it would not restore the user
  clipboard. Nested teardown isolates this. Treat desktop restoration/documentation as
  test hygiene, not a demonstrated growing owner leak.

- **Optional:** Microphone sample-format support excludes usable device formats. (C12)
  Review: Unsupported default formats already produce an explicit error. No affected
  device was demonstrated. Additional sample-format support is a portability extension
  until such a device establishes applicability.

- **Optional:** API credentials are checked after trimming but transmitted untrimmed.
  (C20)
  Review: The trim is only an emptiness check, not a promised key normalization step.
  Whitespace-bearing keys fail authentication/header validation safely. Accepting
  trimmed keys is optional boundary polish, not a proven violation of the current
  contract.

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

- **Optional:** Wrap updates duplicate branches whose simplification must preserve the
  unchanged-row case. (23/S1, R1, plausible)
  Review: The branches have different topology/unchanged-row handling and are covered by
  equivalence tests. Sharing them is optional if it reduces complexity without weakening
  the zero-change case.

- **Optional:** Line-terminator handling and syntax line-window calculations duplicate
  logic. (23/R2, R3, plausible)
  Review: Line-ending inconsistency is already a confirmed behavioral issue above. The
  remaining syntax-window code has different responsibilities; duplicated-looking
  expressions do not establish another defect.

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
