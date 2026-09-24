# Optimization log

Concise record of measurement-driven performance work on the GPUI editor.
Numbers are from the reference host (i9-14900K, RTX 4090, 3840x2160@144Hz
physical display `:0`, scale factor 2.0) unless marked *nested* (off-screen
Xephyr, software presentation, only useful for relative app-side costs).
Commands: see [Performance](docs/performance.md). Earlier sessions are retained
in [the historical log](docs/optimization-history.md).

## Session 4: fresh baseline (23 September evening)

- Start from `8accfc9`, clean worktree, production release built with
  `cargo build --release -p lst-gpui --bin lst --example bench_editor_x11`.
  Preserve both binaries for paired comparisons. Same reference host, physical
  `:0`; run scenarios separately with `--repetitions 3 --priming 1 --keep-temp`
  unless noted. Raw outputs and traces are retained under `/tmp/lst-perf-sep23`
  and `/tmp/lst-gpui-bench-*` during the session.
- Initial medians: small-file spawn to first frame 184.715 ms (five runs),
  large-Rust typing 1.211 ms/character, plain typing 0.489 ms/character,
  highlighted/plain scrolling 1.636/1.614 ms per app frame, search reindex
  0.297 ms, 1k-cursor first-frame paint 0.491 ms, idle 40 ms CPU per two
  seconds. Large paste 11.425 ms in the initial sweep.
- Large-Rust typing spends 214.454 ms in 320 parser updates, versus
  23.473 ms preparing and 13.261 ms painting viewports. Investigate whether
  edits can be combined before each visible frame while keeping syntax current
  for every consumer; delaying highlighting past a frame is not acceptable.
- Two initial scenarios timed out (mixed paste and medium typing); the latter
  trace shows focus loss. Exclude these attempts and reproduce before assigning
  an application cause. Startup discovery sometimes observes pre-tiling window
  sizes; frame traces, not that early geometry, determine comparable dimensions.
- Correct the throughput completion gate: require a completed frame after the
  final input operation, rather than accepting any paint since the burst began.
  The old gate can exclude final rendering work. Twelve benchmark self-tests
  pass; subsequent typing comparisons must use this runner on both binaries.
  Apply the same completion ordering to paste and the first post-paste key;
  rebuild and use that runner for subsequent paste comparisons.
- Vendor review: glyph tiles remain resident; the atlas removal call in the
  current window implementation removes images. No evidence yet to retire the
  glyph cache. Temporary startup phase instrumentation is not a product change.

### Rejected experiment: defer parsing until render

- Composed pending edit batches in original coordinates (borrowed text pieces,
  no repeated copies of large insertions), deferred syntax synchronization to
  render, and synchronized explicitly before structure/layout queries. Model
  invariants, exhaustive small Unicode composition, and randomized disjoint
  edit sequences passed. Preserved the prototype outside the worktree.
- Corrected-runner comparisons, three measured runs after priming on both
  binaries: large typing 1.204 -> 1.205 ms/character; medium 1.048 -> 1.080;
  plain 0.515 -> 0.510. Both large-file variants still parse **320 times**
  (~215 ms) for 320 characters. No demonstrated gain; removed the experiment.
- Root cause: GPUI `Window::dispatch_key_event` draws a dirty window before
  dispatching the next key event to rebuild its dispatch tree. These are real
  CPU renders even when the scene is not presented. Deferring parsing to
  render merely moves it into those draws. Simply skipping them risks stale
  focus, key contexts and handlers; a batching change needs an explicit,
  sound input-routing/invalidation design at the framework boundary.
- Temporary startup instrumentation (removed) puts GPU context creation at
  ~90–104 ms overlapping ~81–99 ms font discovery, surface creation at
  ~41–83 ms, pipelines ~7 ms. Checked X11 request round trips were individually
  below 1 ms; no evidence for another blanket non-blocking setup patch.



### Large external clipboard measurement

- Reproduced mixed-paste hangs twice. System-call traces show Anki's idle mpv
  client requesting the selection first, then leaving its `MPV_CLIPBOARD`
  property in INCR state. xclip serializes that unfinished transfer and stops
  responding to lst (and a separate reader). This is not editor parsing work.
- Use xsel as the benchmark's foreground clipboard owner; it serves concurrent
  requests. Leave other desktop applications untouched. A 3.9 MB transport
  probe and four complete mixed-paste runs passed, including exact saved text.
  xclip remains the reader; benchmark prerequisites now include xsel.
- With the corrected completion gate, baseline 2.98 MB mixed/plain paste is
  866.671 ms to paint: clipboard retrieval 833.248 ms, edit application
  21.308 ms. GPUI's one-millisecond polling sleep per INCR chunk is the next
  measured bottleneck, rather than the editor's text operation.

### Cache unchanged tab chrome

- Split the tab strip into a GPUI child view with one complete, comparable
  set of render inputs. Reuse GPUI's existing layout/paint cache when those
  inputs are unchanged. Changed inputs render immediately; callbacks read
  current application state through a weak owner handle. No vendor patch.
- Paired production measurements, three runs after priming: navigation frame
  wall time **1.054 -> 0.769 ms** and scrolling **1.588 -> 1.239 ms/frame**.
  Navigation CPU fell 3140 -> 2600 ms; scrolling CPU 1250 -> 1070 ms.
  Key-to-screen latency 7.181 -> 7.107 ms is effectively unchanged. Large-file
  typing 1.245 -> 1.284 ms/character shows no gain; parser cost still dominates.
- Default smooth-caret navigation produces ~25.6 app frames per key on this
  host, unchanged by the patch. Reusing unchanged chrome reduces their cost
  without shortening the animation or moving work outside the timing window.
- Verification: release build and 31 focused nested X11 tests passed (tabs,
  chrome, state trace, recent files). All ten physical-display visual scenarios
  match the preserved baseline exactly over three fresh launches each; reviewed
  the reference captures. Run scenarios separately because the five-scenario
  group exceeded the harness timeout. All-targets/all-features Clippy passed
  (existing upstream Blade lifetime warnings). Full checkpoint gates follow.


### Wait for external clipboard data without per-chunk sleeps

- GPUI's private clipboard reader now waits for socket readiness with the
  existing deadline instead of sleeping one millisecond whenever its event
  queue is empty. Preserve timeout, INCR, conversion, and error semantics.
- Paired corrected-runner measurements, three runs after priming: 2.98 MB
  mixed paste **737.461 -> 92.082 ms** through completed paint; clipboard
  retrieval **709.711 -> 60.234 ms**. Exact pasted and subsequently typed
  contents verified. This comparison includes the status-bar cache candidate;
  the measured clipboard-read reduction is directly inside the transport.
- All 17 workflow tests passed, including a new controlled INCR owner that
  stalls: after the existing timeout, queued typing and saving still work.
  The broader 53-test run passed 51; two prompt-button failures exposed stale
  geometry publication in the separate status-cache candidate, being fixed.
  Vendor patch and patch ledger updated; full checkpoint gates follow.


### Cache unchanged status chrome and publish complete geometry

- The status bar now uses the same GPUI child-view cache as the tab strip,
  keyed by its complete render inputs. Keep its original full-width geometry
  and intrinsic height; changed inputs render immediately.
- Against the tab-strip-only build, paired three-run medians: navigation
  frame wall **0.777 -> 0.568 ms**, scrolling **1.257 -> 0.920 ms/frame**;
  respective total CPU **2610 -> 2230 ms** and **1110 -> 900 ms**. Navigation
  key-to-paint 6.319 -> 6.636 ms does not demonstrate a presentation gain.
- Caching exposed premature test trace records with new selection state and
  old button bounds. Publish once after root prepaint, when all sibling
  geometry is available; install that hook only when tracing is enabled.
- Verification: all 53 focused nested chrome, prompt, trace, voice and workflow
  tests pass, including the initially failing prompt-button cases. Full
  all-features source tests, all-targets/all-features Clippy and 12 benchmark
  self-tests pass. All ten physical visual scenarios match the preserved
  original baseline exactly across three fresh launches each.
- Exclude early status-cache measurements from a version whose inner bar
  lacked full width. Also exclude physical captures made while the nested
  server occupied a tiling slot: client width differed from the reference.


### Allocate GPU path targets only for scenes that use them

- Full-window resolved/MSAA path textures were created at window startup
  and every resize even when the scene had no paths. A single optional owner
  now groups each texture with its view and allocates on the first path batch.
  Resize/format changes release after GPU completion. Rasterization and
  compositing stay unchanged; duplicated cleanup and independent MSAA options
  disappear. This is a narrow renderer responsibility, documented as patch 11.
- Production GPU memory **208 -> 29 MiB** at 3816×2100 across three paired
  launches; CPU RSS remains ~205 MB. A temporary path probe is pixel-identical
  in 12 comparisons: empty, paths, fullscreen, restored, hidden and reshown at
  1×/4× sampling. Drawing paths allocates the same resources as before.
- All ten production screenshot scenarios match the original baseline exactly
  over three launches each. The 53 focused X11 tests, all-features source
  suite and Clippy passed with this renderer. Vendor patch reverse-application
  check passes. The temporary probe is retained outside the repository only.
- Paired production medians: small startup 186.398 -> 190.562 ms (seven runs),
  large first frame 189.393 -> 199.486 ms (five); no startup gain established.
  Scroll 1.010 -> 1.006 ms/frame, plain typing 0.476 -> 0.509 ms/character
  with equal 130 ms process CPU, idle 30 -> 30 ms CPU/two seconds. Keep this
  change for the memory reduction; startup variance merits reverse-order checks.
- Temporary finer Vulkan instrumentation (removed) attributes ~53–61 ms to
  device creation and ~48–51 ms to the first swapchain, versus ~1.5–1.9 ms
  for a later resize. Adapter inspection itself is only ~25–30 microseconds.
  Full nested checkpoint follows; investigate driver-bound startup separately.


### Measure the complete root paint

- Frame accounting ended inside the viewport canvas, excluding later status
  chrome and overlays. A trace-only root element now owns the frame clock and
  ends accounting after delegating the complete root paint. It adds no layout
  node and is absent when tracing is off; remove the app's shared pending clock.
- Historical frame-cost values above use the old viewport boundary. Subsequent
  work uses the complete-root boundary and a fresh comparable baseline; do not
  interpret the larger measured scope as an application regression. These are
  still app-side paint timings, not GPU submission or presentation timings.
- All-features source tests and all-targets/all-features Clippy pass. With
  tracing enabled, clean-editor and find-panel images match the original
  baseline exactly across three launches each. The final-input gate smoke
  passes and verifies exact saved text; its concurrent-build timing is not a
  performance comparison. Full nested checkpoint follows.

- Checkpoint `65796dc`: the complete nested X11 lane passed **287/287**
  tests (27m21s), covering the committed clipboard, chrome, renderer and
  complete-root timing changes. Use its preserved production binary as the
  baseline for the next wrapping experiments.

### Build wrapped-row counts from rope chunks

- Profiling the 84,002-line paste corpus found repeated character, UTF-16 and
  line metadata construction in `Rope::lines()`. Traverse borrowed chunks
  using Ropey's own line-boundary rules; one reusable string assembles only
  lines crossing chunks. Preserve the existing grapheme-aware row counter.
- Warm core probe: **3.3 -> 1.8 ms** at 100/220 columns. At 80 columns,
  actual word wrapping dominates: 13.6 -> 11.8 ms. These isolate row-index
  construction, not input-to-screen latency.
- Paired production `open-large --corpus huge-plain-500k`, three runs after
  priming: first wrap construction **28.2 -> 17.0 ms**; spawn to first frame
  213.734 -> 201.752 ms and process CPU 390 -> 350 ms. The WM resize causes
  a second construction. Whole-launch timing remains noisier than this phase.
- Mixed-paste totals show no gain in this pass: 61.462 -> 92.775 ms while
  clipboard read varies 40.502 -> 64.786 ms. Do not attribute transport
  variance to row counting. Navigation and scrolling are effectively unchanged.
- Existing wrap tests plus fragmented-Unicode equivalence against logical
  rope lines pass, including long lines, CRLF and Unicode separators. The
  all-features source suite and Clippy pass; focused X11 validation follows.

### Avoid Unicode property lookup for ASCII grapheme cells

- At 80 columns, about 85% of wrapping CPU was in grapheme-cell construction
  and counting. ASCII has one scalar per cluster except CRLF; construct that
  same metadata directly in the shared editor helper, retaining Unicode
  segmentation for other text. Reserve the known ASCII capacity once.
- With chunk traversal already present, the 84,002-line core probe improves
  **11.4 -> 4.1 ms** at 80 columns. At 100/220 columns, short lines already
  bypass segmentation and remain ~1.8 ms.
- Production plain typing, three paired runs: 0.413 -> 0.382 ms/character,
  CPU 110 -> 90 ms. Five reverse-order runs: 0.478 -> 0.413, CPU 130 ->
  110 ms. Compared with the earlier complete-root baseline, the reverse run
  has equal 0.413 ms/character; whole-burst timing is variable. Large-Rust
  typing remains parser-bound, and navigation shows no latency improvement.
- Every adjacent ASCII byte pair (16,384 cases), long CRLF/control sequences,
  model/Vim suites, internal-invariant tests, all-features tests and Clippy
  pass. Additional X11 validation follows with the wrapping changes.

### Rejected experiment: native OpenGL backend

- Isolated copies of GPUI/Blade exercised the existing GLES backend. Native
  EGL requires main-thread context ownership and matching X11/EGL visuals;
  this host needed a 24-bit visual rather than GPUI's usual 32-bit visual.
  These experimental changes remain outside the repository.
- Quiet physical-display `open-small`, three runs after priming: GLES
  **217.621 ms**, Vulkan **188.801 ms**; RSS 265.188 versus 201.855 MB.
  An earlier unprimed smoke was slower still, but ran beside the nested lane
  and is not a comparative latency result. No demonstrated benefit justifies
  the backend/thread-affinity/visual-selection maintenance cost.

### Remove the grapheme-length truncation

- Review exposed an existing `u8` length for grapheme clusters. Opening a
  valid cluster with 300 combining marks crashed the preserved `65796dc`
  production binary before its window appeared (wrapped cursor-row indexing
  used the truncated length). Store the length as `usize`, matching document
  offsets, and remove the narrowing conversion and downstream casts.
- The new X11 regression fails on the preserved baseline and passes with the
  fix: open, select the whole cluster, copy its exact text, replace, save,
  undo and save. Seven viewport/resize/wrap tests also pass. Core tests cover
  the old boundary and a 1,025-character cluster; all-features tests, model/Vim
  suites, internal invariants and Clippy pass.
- The preceding chunk/ASCII changes separately passed all 42 focused X11
  wrapping, word-motion, multi-cursor-editing and viewport cases.

### Traverse changed wrap lines sequentially

- Replacing a document without changing its line count used `Rope::line`
  separately for every invalidated line. Repeated-paste profiling put ~38%
  of process CPU in those rope lookups. Use one `lines_at` iterator across
  the changed range; an unwrapped layout needs no row updates when line
  topology is unchanged. Delay mutable access until a row update is needed.
- Physical-display replay: three runs of 100 actual 2.98 MB replacements
  per variant, using the app's own clipboard and verifying exact saved text.
  Median wrap-patch time **14.191 -> 3.941 ms**; total paste application
  **19.735 -> 12.964 ms**. These are app-operation timings, not external
  clipboard or presentation latency. With the grapheme-length fix included,
  application time is 12.647 ms; no additional regression is established.
- Broadened incremental/full-layout equivalence covers whole-document ranges,
  Unicode, tabs and wrapped/unwrapped modes. All-features tests, Clippy and
  seven real-X11 viewport/resize/wrap tests pass on the combined candidate.
- Reverse-order seven-run startup check for lazy GPU path targets gives
  eager 197.913 versus lazy 187.823 ms, reversing the earlier ordering.
  Treat startup as unchanged within variance; retain the proven GPU-memory gain.

- Checkpoint `295b3fb`: the complete nested X11 lane passes **288/288**
  tests (27m48s), including the new long-grapheme regression. This covers
  chunk traversal, ASCII cells and sequential incremental wrap maintenance.

### Preserve GPU context thread affinity

- The GLES experiment exposed a defect in the existing GPUI startup-thread
  patch: an EGL context is not `Send`, so that backend could no longer compile.
  Restrict the worker to Vulkan and construct EGL on the platform thread.
  Native Vulkan initialization is unchanged. Mark/document the existing
  Windows frame-option adaptation as well; regenerate the exact vendor patch.
- The actual vendored sources compile with `RUSTFLAGS='--cfg gles'` (isolated
  target directory), and native all-features tests and Clippy pass. Vendor
  reverse-application check passes. This restores the build contract, not an
  endorsement of the rejected GLES runtime experiment.
- A minimal C/Vulkan probe, without GPUI or optional device features, takes
  ~48–62 ms for its first device and ~43–55 ms for subsequent devices; adding
  only the swapchain extension changes little. Instance creation is ~19–28 ms.
  This independently reproduces much of the measured driver-side startup cost.

### Skip delimiter-free text in plain structural scans

- Reuse one `memchr2`-based delimiter test for full rope chunks and inserted
  text. Delimiter-free chunks use bulk UTF-8 character counting; candidate
  chunks retain the original ordered byte scan. Unicode delimiter handling
  remains character-based. No new dependency or cache.
- Rejected a merged SIMD-token iterator: dense 2.98 MB mixed text regressed
  2.2 -> 3.5 ms. The simpler chunk prefilter measures ~2.3 ms there, while
  29.5 MB plain text improves **9.5 -> 3.2 ms** in the isolated scan.
- Paired production 29.5 MB paste, three runs after priming: application
  **55.431 -> 26.123 ms**, input through paint **222.326 -> 183.513 ms**;
  clipboard read 137.136 -> 130.789 ms. Exact saved text is verified.
  Large-plain startup view construction falls ~10.1 -> 4.2 ms, but whole
  first-frame timing 205.870 -> 205.008 ms shows no established startup gain.
- Fragmented Unicode/full-scan and incremental equivalence tests pass, as do
  all-features tests and Clippy. Combined candidate validation: 42 focused
  X11 find, replace, language, decoration and viewport tests; all ten physical
  visual scenarios match the original baseline across three launches each.

### Keep valid wrapped-row indexes

- A syntax-only refresh no longer invalidates wrap rows for the same text
  revision. A wider viewport reuses existing row starts when every logical
  line already fits one row, while clearing width-dependent visible-line
  caches. Narrowing, text changes and wrap-mode changes retain their normal
  validation/rebuild paths. No new cache fields or derived-state owner.
- Five production large-plain launches after priming consistently need one
  ~17 ms row-index build instead of two (the second previously cost 17–22 ms
  after WM resize). Large Rust goes from three ~0.5–0.8 ms builds to one,
  also avoiding the syntax-completion rebuild. Large-plain process CPU falls
  370 -> 340 ms against the delimiter-prefilter build. Whole-startup timing
  remains dominated by GPU/font variance; do not attribute its full change
  to this small phase reduction.
- Fresh-layout equivalence covers widening/narrowing, Unicode, tabs, long
  words and wrap-mode changes. Same-revision syntax-refresh coverage, source
  suites, Clippy, 42 focused X11 cases and all ten exact visual scenarios pass.

### Update no-wrap width when the longest line grows

- Typing into the current widest line invalidated the cached maximum and
  scanned every document line after each character. Remeasure the changed
  range first: if it still reaches the old maximum, unchanged lines cannot
  exceed it. Retain the full scan when the maximum may have shrunk. Reuse
  the existing cache, with no additional retained state.
- Physical `typing-plain --typing-no-wrap --position end`, candidate then
  preserved baseline, one priming run: 18,000 lines (five measured runs)
  **2.954 -> 0.446 ms/character**, CPU **890 -> 120 ms**. At 500,000 lines
  (three runs), **69.708 -> 0.382 ms/character**, CPU **22,270 -> 130 ms**.
  Both variants finish the complete 320-character edit and verify exact
  saved contents. Initial layout and shrinking the widest line still require
  a full measurement; this gain concerns ordinary line growth.
- The new real-X11 scrollbar test passes on baseline and candidate: grow a
  line past another line's width, continue typing, then shrink it below that
  line and verify the actual horizontal extent and saved text. All eight
  viewport tests, the all-features source suite and Clippy pass. A broader
  checkpoint follows with the search changes.

### Separate model search timing from application synchronization

- `find_reindex_ms` previously timed the entire application update, including
  PRIMARY selection publication and view synchronization. Time the model query
  update/active-result selection directly; expose the containing operation as
  `search_query_update_ms`. Both remain inside the complete query/frame path.
- Rebuilt preserved full-index and windowed-render variants with identical
  instrumentation. Twelve benchmark self-tests, all-features tests and Clippy
  pass. Runtime records confirm the containing operation includes model time.
  The broader wrapper was not the cause of the model-phase variance below.

### Convert only visible find matches for painting

- Every render converted the entire find index from line/column coordinates
  to character ranges. Use binary searches on the existing ordered index and
  request only matches overlapping the freshly prepared viewport rows. Keep
  the complete index for counts, navigation and replacement. No new cache.
- Corrected-instrumentation, reverse-order production comparison on
  `search-large --corpus huge-rust-50k`, five runs after priming, 6,144 final
  matches: first query update through the final completed root paint
  **44.824 -> 17.060 ms**. Total root-frame CPU **89.132 -> 16.753 ms**
  over the same nine frames; process CPU **220 -> 150 ms**. Query spans are
  derived from the existing input/frame epoch trace, excluding X delivery.
- Isolated final model-query time rises **0.687 -> 2.192 ms** in these runs,
  although its algorithm is unchanged. The first query character costs ~4.1 ms
  in both variants; subsequent phases differ. Pinning both processes to CPU 12
  preserves that ordering (0.696 -> 2.282 ms), so migration is not an adequate
  explanation. The cause is not established; do not claim an indexing gain.
  Retain the clear improvement in complete-query time and CPU instead.
- Exhaustive character-window comparisons against the full index cover empty
  and reversed windows, literals, regex, Unicode clusters and line separators.
  All-features tests, Clippy and 42 focused X11 find/replace/language/decoration/
  viewport cases pass. The find-panel image matches the original baseline
  across three launches. A full combined checkpoint follows.

### Use the framework's modifier state

- Remove the app's process-wide X11 connection and synchronous pointer query
  on every key. GPUI already owns the raw window modifier state; combine it
  with the event and existing chord history. This also removes the normal
  app dependency on x11rb (the benchmark/test dependency remains).
- Simply deleting the query is incorrect: GPUI normalizes Shift out of
  symbol keystrokes. A new held Ctrl+Shift+backslash regression passes the
  baseline, fails that naive deletion, and passes the window-state version.
  All 77 focused modifier/chord/text/multi-cursor/Vim X11 tests pass, as do
  all-features tests and Clippy.
- Physical-display paired production runs, one prime: plain typing seven-run
  median **0.413 -> 0.319 ms/character**, CPU **110 -> 100 ms**; large Rust
  typing three-run median **1.144 -> 1.047 ms/character**, CPU 350 -> 340 ms.
  Saved output and the final input's completed frame are verified.
- Five-run single-key medians show no established presentation gain:
  typing 8.173 -> 8.267 ms; navigation 6.881 -> 6.711 ms. Typing delivery
  falls 0.469 -> 0.391 ms, while navigation delivery is nearly unchanged
  (0.387 -> 0.381 ms). Keep the throughput gain and simpler ownership;
  do not claim a general screen-latency improvement.

### Share borrowed-line traversal with full width measurement

- Extract the wrapping traversal into `for_each_rope_line`, preserving Ropey's
  exact lines, terminators, and final empty line. Full no-wrap width scans now
  borrow chunk text instead of constructing metadata-bearing slices for every
  line. Cross-chunk lines use one reusable buffer; local edits keep their
  existing slice-based measurement. Reuse one ASCII/monospace predicate.
- A 500,000-line scan probe falls ~70 -> 17 ms. Alternating-order production
  runs deleting 320 characters from the widest line of a 29.5 MB document:
  **23,561 -> 5,720 ms through the final paint**, median of three launches
  each; app cost **73.622 -> 17.855 ms/backspace**. Exact saved text verified.
  A 30 MB single-line scan probe is also faster (~9.0 -> 7.6 ms), though
  cross-chunk materialization requires temporary storage for that line.
- Line traversal matches Rope::lines after 1,024 fragmented Unicode edits,
  all supported line separators, empty text and long cross-chunk lines.
  All-features tests, Clippy and all eight viewport X11 cases pass. Full
  scans remain necessary when the scalar maximum can no longer prove the
  extent; retaining measured widths is the next experiment.

### Retain measured widths when the widest line shrinks

- Replace the scalar no-wrap maximum with one owner for per-line pixel widths
  and their maximum. Remeasure only invalidated lines; when a widest line
  shrinks, reduce the stored numbers instead of re-reading or reshaping text.
  Growth still needs no full reduction. Font and line-topology changes rebuild.
- Alternating-order production comparison against the borrowed-scan version,
  three launches each, the same 500,000-line/320-backspace workload:
  **5,704 -> 199 ms through the final paint**; app cost **17.800 -> 0.597
  ms/backspace**. Every run verifies the exact 29.5 MB saved file. Relative
  to the original slice-based scan, the complete operation falls ~23.6 s
  -> 0.20 s. Growing-line typing stays in the overlapping 0.25–0.29 ms/char
  range; both variants use 110 ms CPU in the three-run comparison.
- Tradeoff: four bytes per logical line when no-wrap measurement is needed
  (~1.9 MiB for 500,000 lines). Sampled whole-process RSS is ~242 MiB for
  both variants; do not interpret allocator reuse/noise as zero storage cost.
  Shrink reduction is still linear in line count, but reads only this array.
- Incremental widths/maxima match fresh reductions across 1,024 updates,
  including ties, growth, shrink, empty windows and clearing every width.
  All-features tests and Clippy pass; the full combined X11 checkpoint follows.

### Use completion timestamps instead of polling wakeups

- Short typing runs landed in ~0.032 ms/character steps: a 10 ms polling
  interval divided by 320 characters. Keep the final-input completion gate,
  but calculate typing and paste-to-paint durations from injection time to
  the first corresponding completed-frame epoch. Reject incomplete trailing
  timestamp records, and never substitute a later blink frame.
- `typing_completion_observed_ms` preserves the runner's observation latency
  as a diagnostic. Large paste now uses `paste_input_to_paint_ms` as its
  primary; `paste_complete_ms` remains the polling-based application-phase
  diagnostic. Update the performance guide and benchmark contract tests.
- Twelve benchmark self-tests, all-features tests and Clippy pass. Earlier
  typing/paste numbers in this log include polling delay; compare new numbers
  only with baselines run through the same corrected runner. Startup and
  single-key latency already used epoch timestamps and are unaffected.

### Combined input, find, and width checkpoint

- The production `cb45390` build passes the full nested lane: **290/290**
  tests in 1,651.808 seconds. This covers visible-only find conversion,
  structural scan filtering, wrap reuse, framework modifier state, and the
  measured-width cache together, including clipboard/process timeouts,
  held keys, geometry, filesystem conflicts, Vim and multi-cursor editing.
- The later compact-layout and deferred-font candidates are being checked
  separately; this checkpoint does not claim coverage for those experiments.

### Rejected experiment: defer the upstream font loader

- Tested a small GPUI change that ran unchanged `FontSystem::new()` on a
  worker, with a single lazy owner joining before the first font operation
  and synchronous fallback if thread creation failed. This let X11 setup
  proceed while fonts loaded; it did not skip fonts or postpone required
  first-frame work beyond the timer.
- Alternating production launches on the restored physical display, one
  prime per variant: small-file first frame **174.727 -> 175.256 ms**
  (11 measured launches each); large-file **170.886 -> 181.164 ms** (seven
  each). No whole-startup gain, despite occasional shorter initialization
  phases. Reverted the entire font change; no extra vendor patch retained.
- Both candidates passed source tests/Clippy and the compact-layout/font
  combination passed eight viewport X11 cases. A read-only adapter probe
  found only NVIDIA exposed by the installed Vulkan ICD; the extra hardware
  render node is not an available alternative Vulkan adapter on this setup.

