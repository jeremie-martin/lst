# Optimization log

Concise record of measurement-driven performance work on the GPUI editor.
Numbers are from the reference host (i9-14900K, RTX 4090, 3840x2160@144Hz
physical display `:0`, scale factor 2.0) unless marked *nested* (off-screen
Xephyr, software presentation, only useful for relative app-side costs).
Commands: see `docs/performance.md`.

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

## Session 3: baseline and measurement reliability

- Display interruption: the user reported an accidental monitor power-off
  during the broad comparison. Discarded that sweep and re-ran both preserved
  production binaries after restoration, including targeted comparisons.
  Results below use those reruns. Correctness tests were unaffected.
- Baseline: `7748109`, production release, physical `:0`, same reference
  CPU/GPU, NVIDIA 615.71.09, restored desktop with a 3816x2100 client window
  at scale 2. Keep comparisons within this session; older startup timings
  are not comparable. Commands use `--repetitions 3 --priming 1` unless
  specified otherwise; final production code is `212650d`.
- `typing-plain --corpus huge-plain-500k`: 0.887 ms/character;
  112 ms of 118 ms applying 320
  characters is wrap-layout maintenance. The incremental update traverses
  all later line offsets even when their row delta is zero.
- The initial one-run `all` sweep failed at mixed paste: pointer input used
  discovery-time coordinates after the tiling WM moved the window. Resolve
  the live origin and size before pointing, as the acceptance harness does.
- `open-large --corpus huge-plain-500k` exposed a separate benchmark race:
  a mapped window can remain undamaged for the quiet interval before its
  first frame exists. Wait for the existing first-frame stamp before quiet;
  the spawn-to-frame measurement itself is unchanged. The WM resize can
  cause a second wrap layout. No app or GPUI changes in this measurement
  correction.

### Incremental wrap maintenance and borrowed row counting

- Skip suffix offset updates when the row delta is zero. Count rows directly
  in borrowed contiguous rope text, allocating only for a line crossing rope
  chunks; this replaces repeated character lookups and rope subslicing and
  also avoids copying contiguous complex lines. No new cache or vendor patch.
- Final restored-display comparison, three measured runs after one priming
  run: huge plain typing 0.887 -> 0.461 ms/character; input application total
  118.155 -> 7.062 ms; wrap patch total 112.331 -> 2.256 ms for 320 characters.
  With the startup changes below, huge plain first frame is 312.465 ->
  210.341 ms. The quiet metric changes with window activation/re-presentation
  and is not evidence for or against this improvement.
- Verification: grapheme row-count equivalence across rope chunks, Unicode,
  tabs and line endings; incremental layout equivalence for positive, zero
  and negative row changes with wrapping on/off; 14 nested X11 viewport and
  motion acceptance tests passed. Benchmark self-tests passed and the fixed
  mixed-paste scenario completed with verified saved output.

### Startup preparation and optional GPU capabilities

- File reads and rope construction now overlap GPUI initialization in a
  dedicated app-owned startup loader. It hands stamped tabs to the existing
  model boundary; scratchpad creation remains after the window opens. Thread
  creation failure falls back to synchronous loading, and file errors retain
  the existing status/fallback behavior.
- A symbolized startup profile showed NVIDIA ray-tracing library work.
  Blade 0.7.1 automatically enables Vulkan ray queries even though GPUI does
  not use them. Backport upstream `c7edf7b6`'s explicit opt-in instead of
  upgrading to an incompatible renderer API or disabling driver features via
  environment variables. The patch adds one descriptor field and gates the
  existing coherent capability path; GPUI explicitly selects raster-only.
- Final restored-display `open-small --repetitions 5 --priming 1`:
  first frame 220.998 -> 171.540 ms, peak RSS 298.871 -> 200.590 MiB.
  This is RSS, including driver/library mappings, not a claim of 98 MiB
  less editor-owned heap. File preparation remains inside spawn-to-first-frame
  timing even though it overlaps platform initialization.
- All-feature tests and Clippy pass (the newly vendored upstream Blade emits
  seven pre-existing lifetime-syntax warnings). All 23 nested startup,
  clipboard and file workflow tests passed. Both vendor patch files pass
  reverse-apply checks against their source trees.
- The sweep also exposed ~60 paints per two idle seconds. X11 event capture
  confirms a stream of identical synthetic ConfigureNotify messages from the
  WM. GPUI's unconditional resize/move callbacks turn these into full redraws.
  Compare actual drawable size and window origin while retaining drawable
  queries and XSync acknowledgements. A separate three-run physical-display
  comparison gives idle CPU 160 -> 40 ms per two seconds (10 ms sampling
  resolution), and paints 80 -> four. This fixes redundant redraws rather than
  suppressing legitimate resize, movement, or caret-blink work.
- Retire the parallel font-scan patch and its three direct dependencies.
  Eleven launches each (five-run batches followed by six alternating pairs)
  give first-frame medians 173.605 ms custom versus 177.894 ms upstream,
  with 143–208 ms within-variant spread. Isolated font loading benefits from
  parallel scanning, but overlaps GPU initialization and competes with it.
  The small uncertain whole-launch difference does not earn ~170 patch lines
  of custom fontconfig parsing and threading. Restore upstream byte-for-byte;
  keep the independently useful GPU startup thread.
- Final three-run comparisons: idle CPU 170 -> 40 ms per two seconds,
  paints 71 -> four. Typing key-to-damage p50 9.528 -> 7.173 ms, p95
  14.131 -> 9.842 ms; frames/key 4.300 -> 1.150. Navigation p50 7.815 ->
  5.596 ms; edit-then-navigation p50 5.703 -> 5.295 ms. Navigation app work
  per frame remains ~1.6 ms; fewer redundant frames are the improvement.

### Local word and subword boundaries

- Ordinary Ctrl/Alt word motion and word deletion built grapheme cells for
  the entire rope on every call. The existing grapheme transitions now run
  on the current line and walk neighbouring lines only across skipped
  whitespace. No document cache, alternate tokenization, or changed word rules.
- Isolated release probe, same corpus and cursor positions, 20 calls per
  operation on 660 KB Rust: ~6.7–7.7 ms -> ~1.5–2.1 microseconds. Two calls
  each on 500k-line plain text: ~351–357 ms -> ~1.4–3.9 microseconds. All four
  result checksums match. These are boundary-operation costs, not claims of
  equivalent speedups in display latency or ordinary single-character motion.
- Exhaustive comparison against whole-document graphemes covers every
  character offset (including mid-cluster inputs and EOF), Unicode line
  breaks, CRLF, combining marks, emoji, underscores, and blank lines across
  rope chunks. Model internal-invariant suites and 17 focused X11 chrome,
  motion and text-input tests passed. The real-app CRLF/subword regression
  also passed in the 266-test full nested checkpoint.

### Batch-edit cursor mapping

- A CPU profile of deleting 10,000 selected occurrences attributes ~96% of
  samples to repeatedly scanning changes/counting replacement characters for
  each cursor. The old mapping is quadratic in selections and edits.
- Build a temporary prefix index from the immutable `TextChangeSet`, then
  binary-search each position. No persistent cache or invalidation protocol.
  Use it for multi-selection deletion, linewise paste, and multi-cursor line
  deletion; the latter no longer constructs and edits a throwaway rope just
  to map offsets. Keep the original linear algorithm only as the test oracle.
- Exhaustive small-coordinate equivalence includes touching replacements,
  repeated insertions, Unicode growth, deletions, and out-of-order queries.
  Added 1k/10k deletion cases to the existing model benchmark and a real-app
  1k-selection deletion/undo regression; the latter passed with 35 other
  affected X11 tests.
- Isolated optimized model probe after all behavior tests, three alternating
  before/after batches of 30 deletions each, verifying every resulting text:
  median batch means 0.526 -> 0.106 ms at 1k selections and 36.668 ->
  1.223 ms at 10k. These are model edit costs, not total display latency.

### Validation and measurement boundaries

- Full nested X11 checkpoint: 266/266 passed. After the cursor-mapping change,
  all-feature tests, internal-invariant model tests, Clippy, and 36 affected
  X11 tests passed, including the new 1k-selection deletion/undo regression.
- Committed visual references use a different window width (1901 versus
  3816 pixels). Add an alternate reference-directory option so the preserved
  baseline can supply same-environment screenshots without overwriting the
  committed images. All ten final scenarios are pixel-identical to those
  references across three fresh launches each; inspected all reference images.
  Grouped runs exceeded the fixed 120-second budget at this desktop size,
  so validation ran one scenario per invocation with assertions unchanged.
- Final all-feature tests, Clippy, formatting, 11 benchmark self-tests, and
  both vendor reverse-patch checks pass. The new 1k/10k model benchmark cases
  also execute successfully in Criterion's test mode. Blade's seven upstream
  lifetime-syntax warnings remain documented rather than patched incidentally.
- The 1k-cursor paint benchmark previously selected the last quiet frame,
  sometimes measuring hidden blinking carets. Select the first completed
  frame after the command instead, and test exclusion of later blink frames
  and incomplete traces. Compare both binaries with the corrected runner;
  do not attribute the earlier apparent paint improvement to production code.
- Corrected 1k-cursor first-frame paint (three runs): 0.494 -> 0.449 ms;
  preparation 0.621 -> 0.649 ms. No claimed step-change in paint itself.
  Highlighted scrolling: mean frame 1.526 -> 1.590 ms, but CPU per run
  1250 -> 1110 ms and app frames/s 188 -> 156. Removing redundant frames
  changes the mixture of cheap unchanged frames and newly scrolled frames;
  app frames above the 144 Hz refresh rate are not extra visible refreshes.
- Plain scrolling likewise stays ~1.54–1.56 ms/frame while CPU falls
  1250 -> 1100 ms. Repeated search reindexing is ~0.3 ms (baseline median
  0.323, final 0.273, with substantial baseline spread); no claimed algorithmic
  search improvement. Both production binaries completed all 15 default
  scenarios with one priming and one measured run. That broad cross-check
  gives large paste 12.360 -> 11.812 ms, mixed paste 44.254 -> 41.916 ms,
  and medium/large/plain typing 1.236/1.325/0.640 -> 1.078/1.229/0.429
  ms/character. Use the repeated targeted results for stronger conclusions.

### Remaining costs

- Single-frame navigation remains ~1.6 ms. Most work is GPUI chrome element
  construction/layout plus glyph painting; reusing whole chrome views would
  require a coherent component/invalidation design, not another loosely
  coordinated display-state cache. Existing texture-order and glyph-tile
  patches still have sound ordering/lifetime assumptions and earn their keep.
- Large-Rust typing still spends ~200 ms per 320-character burst in
  tree-sitter's synchronous incremental parser, particularly error recovery
  near the file start. Moving it after the measured interval would delay
  highlighting/structure availability and is not an equivalent optimization.
- Startup still includes real Vulkan device/surface work and system font
  loading. Lazy font discovery needs upstream-quality fallback semantics;
  dropping fonts or hiding first-frame work would not preserve behavior.
  Startup comparisons are primed launches, not cold-boot disk-cache claims.
- Final symbolized startup profiles (`perf record -F 997 --call-graph dwarf`,
  three one-second launches of the same small-Rust fixture) confirm NVIDIA,
  font discovery, query compilation, shader translation and initial glyph
  rasterization as the CPU work. Existing grammar/font warmup finishes at
  ~99–114 ms, before model construction at ~130–153 ms: it already overlaps
  window creation and is not an additional serial startup phase. Profiled
  phase stamps are explanatory, not extra production benchmark samples.
- Word motion is bounded by visited lines rather than document size, but a
  single enormous logical line still requires line-sized grapheme work.

## Measurement additions

- `open-small`: process spawn to first completed frame (`open_to_first_frame_ms`),
  with app-side startup marks (`startup_app_init_ms`, `startup_model_ready_ms`,
  `startup_window_open_ms`, `startup_first_frame_ms`).
- `idle`: CPU, repaints, and RSS over two focused idle seconds.
- `latency-typing`, `latency-navigation`: one key at a time, key press to the
  first damaged frame (`key_to_paint_ms_p50/p95/max`), frames per key, and
  per-frame wall/CPU cost (`frame_wall_ms_*`, `frame_cpu_ms_*`).
- Trace labels `notify=<reason>` name every app-side redraw request so frame
  counts can be attributed without guessing.
- The summary no longer aborts when a secondary trace metric is absent from a
  run (for example a paste that takes the background syntax path).
- `scroll-*`: the primary metric is now `scroll_frame_wall_ms_mean`, the
  mean app-side frame time during the scheduled wheel input, with
  `scroll_frames_per_second` and `frame_wall_ms_max`; `scroll_overrun_ms`
  stays as a secondary because GPUI's one-second re-presentation after
  input bounds it regardless of editor work.

## Baseline (before any change)

| Metric | Value |
| --- | --- |
| `open-small` open_to_first_frame_ms | 313 (app_init 203, model 54, first frame 26) |
| `latency-typing` key_to_paint_ms_p50 / p95 | 11.9 / 15.3, 4.05 frames per key |
| `latency-navigation` key_to_paint_ms_p50 / p95 | 9.3 / 12.3, 4.03 frames per key |
| `idle` idle_cpu_ms per 2 s | 20, 4 repaints (caret blink) |
| `typing-medium` typing_ms_per_char | 1.32 (syntax reparse 0.49 per char at line 0) |
| `scroll-plain` scroll_overrun_ms | 1065 (see note on presentation below) |

Where the time goes (perf, physical display):

- Every keystroke rendered four frames: model update, then a next-frame caret
  reveal, then the reveal's own notify, plus caret blink frames.
- GPUI keeps presenting the last scene at the display rate for one second
  after any input (`gpui::window` request-frame policy) and the NVIDIA driver
  reports XDamage for a frame only once the following frame presents. Both
  inflate `*_to_quiet` and `scroll_overrun` metrics on this host; they are
  framework/driver behaviour, not editor work.
- GPUI's `Application::new` loads the full system font database synchronously
  (12.8k font files here): ~200 ms of the 313 ms startup. Not fixable in the
  app; GPUI carries a `todo(linux) make font loading non-blocking`.
- Text shaping: cosmic-text 0.14 creates a rustybuzz shape plan per word, so a
  freshly visible code line costs ~0.2 ms to shape; scrolling is dominated by
  it (26% of app CPU nested).
- tree-sitter incremental reparse per keystroke: 0.16 ms mid-file, 0.5-2.5 ms
  when the edit sits at the top of a large file (error recovery).

## Changes

1. **Inline caret reveal** (`editor_view.rs`, `shell.rs`): the queued reveal is
   applied during `render` against the previous frame's geometry instead of a
   next-frame callback plus notify. Frames per key 4.05 -> 1.13 (the remainder
   is caret blink); *nested* key_to_paint p50 34 -> 9.8 ms.

2. **Cell-painted code lines** (`code_line.rs`): plain monospace ASCII
   segments are painted from a per-token glyph cache at column positions
   instead of being shaped by cosmic-text (one rustybuzz shape plan per word).
   Tokens are shaped once through `layout_line`, so ligatures inside
   punctuation runs are kept; a token whose advances are not uniform cells
   (proportional font, missing glyph) makes the segment fall back to GPUI
   shaping, as do tabs and non-ASCII text. Each line paints inside its own
   layer, matching GPUI's line painting, so primitives take one draw order
   instead of one bounds-tree insertion per glyph.
   Physical display: `latency-navigation` p50 9.3 -> 7.2 ms (p95 12.3 -> 10.1),
   mean frame 3.7 -> 2.4 ms; `scroll-plain` prepare max 10.1 -> 3.8 ms;
   `typing-medium` 1.32 -> 1.24 ms/char. The visual lane (baselines
   regenerated with the previous binary on this display) is pixel-identical
   for every scenario except the 3,400-character horizontally scrolled line,
   where glyphs previously drifted sub-pixel from the caret's column grid
   through accumulated float advances; they now sit exactly on it.

3. **First-frame warmup** (`main.rs`, `syntax/mod.rs`): the first frame spent
   13 ms compiling the tree-sitter highlight query (`Query::new` runs pattern
   analysis) and 6 ms resolving the editor font through GPUI's font database.
   Both are now done on a background thread started right after settings load,
   overlapping GPUI's ~57 ms window and Vulkan setup on the main thread.
   Startup phase marks (`startup_settings_loaded_ms`, `startup_app_new_ms`,
   `startup_inputs_ready_ms`, `startup_model_loaded_ms`,
   `startup_recent_loaded_ms`, `startup_views_ready_ms`) attribute the rest.
   First frame 25 -> 7 ms; `open-small` open_to_first_frame_ms 313 -> ~275.
   What remains is GPUI: ~200 ms loading the system font database and ~57 ms
   creating the window.

4. **Local row walk for vertical motion** (`lst-editor` `wrap.rs`, `motion.rs`,
   `lib.rs`): the model cached a full-document wrap layout keyed by revision,
   so the first arrow key after every edit rebuilt it (O(lines)), a second
   owner of the layout the app already patches incrementally. Display-row
   targets are now found by walking neighbouring lines, O(rows moved), and
   the model-side cache is gone. New `latency-edit-navigation` scenario
   (type, then time the arrow key), physical display, p50:
   17k-line Rust 15.1 -> 10.2 ms; 500k-line plain 87 -> 11.3 ms (max 102 -> 17).

## Latency anatomy

`latency-*` now split key-to-paint with app wall-clock stamps. Physical
display, `latency-navigation` middle of the 17k-line Rust corpus: X delivery
0.3 ms, app work through paint 2.6 ms, presentation 3.5 ms (7.2 ms p50).
Presentation grows to ~8.6 ms right after an edit because GPUI keeps
presenting the previous scene at the refresh timer for one second after
input, and that timer runs at 60 Hz here: `gpui::platform::linux::x11::client`
takes the first CRTC's mode (the secondary 1080p60 monitor) rather than the
monitor the window is on (3840x2160@144). Smooth scrolling is capped at 60 fps
for the same reason. Both are framework behaviour outside the app.

5. **No guessed-width wrap layout before the first paint** (`shell.rs`): the
   first render built a full wrap layout for an assumed viewport width, and
   the first paint immediately rebuilt it for the real width. The scroll
   extent now uses the logical line count for that one frame. 50k-line Rust
   first frame 18 -> 11.5 ms; 500k-line plain first frame 150 -> 77 ms.

6. **Byte scan for plain bracket structure** (`syntax/highlight.rs`): the
   plain structural snapshot (bracket pairs for files without a tree-sitter
   grammar, and the interim structure while a large file parses in the
   background) iterated every character through closures over the pair
   list. Pairs are ASCII, so a byte scan with a 256-entry class table finds
   them while continuation bytes are skipped; non-ASCII pair sets keep the
   character path, and a test checks both agree on multibyte text.
   500k-line plain open: view setup 74 -> 10 ms.

7. **Window title only when it changes** (`shell.rs`): every render set the
   window title, which GPUI implements as two X property changes with
   round trips. The title is now sent only when it differs from the last
   one sent.

## Results (physical display, 3 measured runs, medians)

| Scenario | Metric | Before | After |
| --- | --- | --- | --- |
| `open-small` | open_to_first_frame_ms | 313 | 288 (GPUI init ~255 of it) |
| `open-large` 50k-line Rust | open_to_first_frame_ms | 334 | 297 |
| `latency-typing` | key_to_paint_ms p50 / p95 | 11.9 / 15.3 | 7.7 / 15.8 |
| `latency-navigation` | key_to_paint_ms p50 / p95 | 9.3 / 12.3 | 6.9 / 9.7 |
| `latency-edit-navigation` 17k lines | key_to_paint_ms p50 | 15.1 | 9.8 |
| `latency-edit-navigation` 500k lines | key_to_paint_ms p50 / max | 87 / 102 | 11.3 / 17 |
| frames per keystroke | count | 4.05 | 1.1 (rest is caret blink) |
| `scroll-plain` | viewport_prepare_ms max | 10.1 | 3.2 |
| `typing-medium` | typing_ms_per_char | 1.32 | 1.27 (tree-sitter reparse bound) |
| `idle` | idle_cpu_ms per 2 s | 20 | 30 (caret blink; unchanged) |
| first frame, 500k-line plain file | frame_wall_ms | 150 | 77 |

Per-key app work is now ~2.5 ms of a ~7 ms key-to-paint; the remainder is
X delivery (~0.3 ms) and presentation (~3.5 ms, up to ~8 ms right after an
edit) inside GPUI and the driver.

## Not done, and why

- GPUI loads the system font database (~200 ms here) and creates the
  window (~57 ms) before any app code runs; GPUI carries a
  `todo(linux) make font loading non-blocking`.
- GPUI's X11 refresh timer uses the first CRTC's mode (60 Hz on this host)
  rather than the window's monitor (144 Hz), capping smooth scrolling at
  60 fps; it also re-presents the last scene for one second after input.
  Both belong in GPUI (`platform/linux/x11/client.rs`).
- tree-sitter's incremental reparse per keystroke (0.16 ms mid-file, up to
  2.5 ms with error recovery at the top of a 660 KB file) is left
  synchronous: coalescing needs composed edit batches and moving it off the
  input path would paint one frame with stale highlights.
- Caret blink re-renders the whole window twice a second (~15 ms CPU/s).

## Session 2: measurement corrections and framework patches

The first session's numbers above were taken with two measurement faults,
found by profiling the production binary:

- **The benchmarked binary was not the production build.** Building the app
  and the runner in one `cargo build` unified the runner's
  `gpui` dev-dependency features (`test-support`, `leak-detection`) into the
  app. With `test-support`, GPUI draws inside `flush_effects`, i.e. right
  after input, whereas the production build only draws when the X11 refresh
  timer fires (`gpui::platform::linux::x11::client::start_refresh_loop`,
  60 Hz here). The dev-dependency is removed; nothing used it.
- **Key injection was phase-locked to that timer.** The runner injected each
  key a fixed delay after the previous paint, so every sample landed at the
  same timer phase and up to one period of latency was invisible. Keys are
  now injected at evenly spread delays. Damage reports also had to be paired
  with the app's frame stamp: GPUI presents the unchanged scene at the
  refresh rate for one second after any input, and every present raises
  XDamage (145 reports per keystroke measured with a probe), so "first damage
  after the key" often preceded the key's frame.

Corrected baseline, production binary at 119a528, physical display, medians
of 3 runs:

| Scenario | Metric | Corrected baseline |
| --- | --- | --- |
| `latency-navigation` | key_to_paint_ms p50 / p95 / max | 9.9 / 16.6 / 17.6 |
| `latency-typing` | key_to_paint_ms p50 / p95 / max | 10.4 / 17.3 / 17.8 |
| `open-small` | open_to_first_frame_ms | 297 |

Where the time goes (production binary): app work through paint ~2.5 ms
(navigation) / ~4.9 ms (typing); then a uniform 0-16.7 ms wait for the
60 Hz timer tick that draws; then ~1.5 ms to the present. Startup: font
database ~70-90 ms, Vulkan instance and device ~120 ms (NVIDIA), both
serialised before the window; window creation ~55 ms (surface and swapchain
~45, pipelines ~10); app setup and first frame ~35 ms. Memory: of ~300 MB
RSS on an empty file, ~200 MB is the NVIDIA driver (file-backed libraries
and `/dev/nvidiactl` mappings) and ~70 MB heap, mostly driver allocations.

### GPUI patches (`vendor/gpui`, see `vendor/gpui/LST_PATCHES.md`)

gpui 0.2.2 is the newest release and upstream `main` still has both
behaviours, so the fixes are carried as a vendored copy selected through
`[patch.crates-io]`; the exact diff is `vendor/gpui/lst.patch`.

8. **Draw right after X11 input.** Windows left dirty by an input batch are
   drawn and presented immediately instead of at the next refresh tick.
9. **Fastest monitor's refresh rate.** The refresh timer used the first
   CRTC's mode (the 60 Hz secondary display); it now uses the fastest active
   CRTC (144 Hz), which is also the smooth-scroll animation rate.
10. **Parallel system font scan.** The fontconfig directory walk parses font
    files on up to eight threads and avoids two `stat` calls per entry
    (~70 -> ~32 ms in isolation; mmap contention limits scaling past two
    threads).
11. **Vulkan context on a startup thread.** Instance and device creation
    (~120 ms) overlaps the font scan and X11 setup.

| Scenario | Metric | Corrected baseline | With patches |
| --- | --- | --- | --- |
| `latency-navigation` | key_to_paint_ms p50 / p95 / max | 9.9 / 16.6 / 17.6 | 6.1 / 8.4 / 12.2 |
| `latency-typing` | key_to_paint_ms p50 / p95 / max | 10.4 / 17.3 / 17.8 | 7.6 / 10.2 / 13.9 |
| `open-small` | open_to_first_frame_ms | 297 | ~240 (app_init 190 -> 135) |

Per key the remaining time is the app's own work (2.5 ms navigation, 4.9 ms
typing, of which tree-sitter's reparse is the largest piece) plus ~1.5-2.6 ms
from paint end to the damage report.

12. **Whitespace markers walk the slice** (`viewport.rs`): the marker scan
    did two rope character lookups per space to find runs; it now tracks
    neighbours while iterating the painted slice.

### Full suite with the patches (0e90458, physical display, 3 runs, medians)

The "previous" column is the first session's final table, which was measured
on the `test-support` binary with phase-locked injection, so the latency rows
are not comparable; the corrected production baseline is in the table above.

| Scenario | Metric | Previous | Now |
| --- | --- | --- | --- |
| `open-small` | open_to_first_frame_ms | 310 | 227 |
| `open-large` 17k-line Rust | open_to_quiet_ms | 1452 | 1332 |
| `latency-typing` | key_to_paint_ms p50 | (10.4 corrected) | 7.9 |
| `latency-navigation` | key_to_paint_ms p50 | (9.9 corrected) | 6.4 |
| `latency-edit-navigation` | key_to_paint_ms p50 | 9.5 (biased) | 5.9 |
| `typing-medium` / `-large` / `-plain` | typing_ms_per_char | 1.17 / 1.35 / 0.79 | 1.17 / 1.40 / 0.72 |
| `idle` | idle_cpu_ms per 2 s | 30 | 20 |
| `multi-cursor-1k` | viewport_paint_ms | 5.8 | 5.8 |
| `search-large` | search_reindex_ms | 0.27 | 0.28 |
| `large-paste` | paste_complete_ms | 10.2 | 11.0 |
| `mixed-paste` | paste_input_to_paint_ms | 30 | 30-61 (see below) |
| `scroll-plain` / `-highlighted` | scroll_overrun_ms | 1111 / 1118 | 1070 / 1077 |

`mixed-paste` times one paste per run and swings between 30 and 61 ms for
either binary across runs (clipboard hand-off with `xclip`, and a
full-document plain bracket scan on paste); it needs more repetitions before
it can rank a change. The scroll overrun metrics remain bounded by GPUI's
one-second re-presentation after input.

### Per-frame preparation (6a4bfa6, and the marker scan commit after it)

A perf profile of smooth scrolling (468 frames in 3 s at 144 Hz) put the
app's prepare step at 0.62 ms per frame: rope line-to-char lookups per row,
identifier-window expansion for occurrence highlights even without a query,
cosmic-text shaping of every newly visible gutter number, a full-window
whitespace/control-character scan through rope slices (control characters
render by default), and cache trimming over four maps every frame.

13. Gutter numbers are painted from per-digit glyph cells (ten shaped
    glyphs shared by every line number) instead of shaping each label.
14. Line starts accumulate across the visible rows; display-line lengths are
    cached; per-line caches are trimmed only when the retained window moves.
15. Occurrence scan windows are built only when a query exists.
16. Painted windows carry their logical line, and the marker scan walks the
    cached display text (bytes on ASCII lines) instead of rope slices.

| Metric (scroll-plain, per frame) | Before | After |
| --- | --- | --- |
| viewport_prepare_ms | 0.62 | 0.24 |
| structure_decorations_ms | 0.21 | 0.05 |
| viewport_visible_highlights_ms | 0.04 | 0.00 |
| CPU per 3 s scroll run | 1710 ms | 1540 ms |

`latency-navigation` key_to_frame_end p50 2.37 -> 2.02 ms. Paint stays at
~0.68 ms per frame, all of it GPUI's per-glyph work (raster-bounds and atlas
hash lookups, primitive insertion); the element tree, layout, and scene
finish add ~1.2 ms per frame outside the viewport.

### Frame anatomy after the preparation work

Timing GPUI's draw phases directly (temporary instrumentation in the
vendored crate, steady-state caret-blink frames, 1,360x860 logical window
at scale 2):

| Phase | ms | Notes |
| --- | --- | --- |
| request_layout | 0.32 | app `render` plus GPUI element construction, 42 taffy nodes |
| taffy compute | 0.46 | 13 measured leaves, 83 measure calls per frame |
| prepaint | 0.27 | 0.23 of it the viewport prepare step |
| paint | 0.61 | 0.53 of it the viewport: ~2,000 `paint_glyph` calls |
| scene finish | 0.18 | GPUI sorts every glyph sprite by (order, tile) each frame |

Rendering without the tab strip and status bar (an experiment, not a
change) cut frame CPU from 1.98 to 1.04 ms and key-to-frame-end p50 from
2.0 to 1.1 ms: the ~35 chrome elements cost ~27 us each across GPUI's
layout, prepaint, paint, and hit-testing work. Marking the never-wrapping
chrome text `whitespace_nowrap` (so GPUI reuses its measured size across
taffy passes) made no measurable difference and was not kept. Reducing the
chrome to fewer, shallower elements is the remaining app-side lever for
frame cost; every option changes element structure and therefore needs the
pixel lane, so it is left for a dedicated change.

### Idle wakeups and the final numbers

The 144 Hz timer raised `idle` from 30 to 40-50 ms per 2 s: the extra
wakeups are kernel time (timerfd, epoll, socket poll), not app work. The
timer now runs at the monitor rate only for 1.2 s after an X11 event batch
(animations start from input, and GPUI re-presents for one second after it)
and at the previous 60 Hz cadence otherwise; an idle tick with nothing to
draw, run, or present returns before entering an app update. `idle` is back
to 20-30 ms per 2 s while scrolling still renders at 144 fps.

Environment note: after a monitor power cycle in the middle of the session
(both outputs are 4K afterwards), Vulkan device creation got slower for
every binary, so app_init measured ~185 ms for a build that measured
~135 ms before. The final table therefore compares the unpatched production
binary and the final build back to back in the same environment.

| Scenario | Metric | Unpatched (119a528, production build) | Final (2bd8423) |
| --- | --- | --- | --- |
| `open-small` | open_to_first_frame_ms | 330 | 259-275 |
| `latency-typing` | key_to_paint_ms p50 / p95 | 11.2 / 17.9 | 6.8-8.0 / 9.6-11.4 |
| `latency-navigation` | key_to_paint_ms p50 / p95 | 10.6 / 17.8 | 4.9 / 8.6 |
| `latency-edit-navigation` | key_to_paint_ms p50 / p95 | 8.4 / 17.4 | 4.8 / 8.1 |
| key to frame end (app work) | navigation / typing p50 | 2.5 / 4.9 | 1.6 / 3.8 |
| `idle` | idle_cpu_ms per 2 s | 30-40 | 20-30 |
| `typing-medium` / `-large` / `-plain` | typing_ms_per_char | 1.17 / 1.35 / 0.79 | 0.92 / 1.18 / 0.46 |
| `scroll-plain` / `-highlighted` | scroll_frame_wall_ms_mean (worst) | 2.98 (6.4) / 2.81 (5.9) | 1.55 (3.1) / 1.51 (3.4) |
| `scroll-plain` | frames per second, CPU per 3 s scroll | 66 fps, 900 ms | 156 fps, 1110 ms |
| `multi-cursor-1k` | viewport_paint_ms | 5.7 | 0.34 |
| `search-large` | search_reindex_ms | 0.27 | 0.30 |
| `open-large` | open_to_quiet_ms | 1452 | 1359 |
| `large-paste` | paste_complete_ms | 10.2 | 11.0 |

The unpatched column was measured after the monitor power cycle; the final
column is the complete suite on the final commit in the same environment,
with `open-small` and `idle` re-measured back to back against the unpatched
binary. Per frame the app now spends ~0.23 ms preparing and ~0.22 ms
painting the viewport; the rest of a ~1.5 ms frame is GPUI's element tree.

Verification on the final commit (2bd8423): `cargo test --all-features`,
the lst-editor internal-invariants tests, `cargo clippy --all-targets
--all-features`, the full nested X11 lane (265 passed), and the physical
visual lane with baselines regenerated per scenario from the unpatched
binary: all ten scenarios are pixel-identical.

## Session 2 remaining work (historical)

- GPUI's per-frame element work for the ~35 chrome elements (~0.95 ms) and
  its per-glyph paint path (~0.5 ms for a full 4K viewport) are the
  remaining frame cost; both need element restructuring or deeper GPUI
  changes and the pixel lane.
- Vulkan instance and device creation (~120 ms, NVIDIA driver, no layers)
  and surface/swapchain creation (~45 ms) bound startup; the former is
  overlapped with the font scan, the latter needs the window.
- tree-sitter's incremental reparse stays synchronous: 0.36 ms per
  keystroke mid-file. `latency-typing` types at the top of a 660 KB Rust
  file, where each keystroke costs ~2.4 ms in `TabSyntaxState::update`: a
  profile puts 37% in tree-sitter's `ts_subtree_last_external_token`
  (external-scanner state checks while re-using nodes under the growing
  ERROR node) and 23% in `ts_tree_get_changed_ranges`; the app's own share
  of that update is under 5%.
- Memory is driver-dominated (~200 MB of the ~300 MB RSS on an empty file).

### Painting in passes (73f479f) and the sprite sort

17. **Pass-based viewport paint** (`viewport.rs`, `code_line.rs`): rows never
    overlap, so backgrounds, text, carets, and the gutter are painted as
    passes. All code lines share one GPUI layer instead of opening one each,
    and the gutter occlusion is one quad instead of one per row; ~200
    bounds-tree insertions per frame are gone. Paint 0.50 -> 0.34 ms per
    frame, scroll CPU -12%, `latency-navigation` key_to_frame_end p50
    1.93 -> 1.78 ms. Pixel-identical on the four scenarios that exercise
    gutter, carets, and highlights.
18. **Sprite sort by texture** (vendored gpui, patch 5): GPUI sorted every
    glyph sprite by (order, tile) each frame; grouping by texture is what
    batching needs, and it leaves text-order sprites already sorted
    (finish 0.18 -> 0.10 ms at ~860 sprites; more at 4K). Pixel-identical.
19. **Fewer chrome elements** (`shell.rs`, `ui/tab.rs`, `ui/icon_button.rs`):
    the four tab-strip buttons no longer each sit in a wrapper element whose
    only job was capturing bounds (the two button groups capture their
    children's bounds instead, which are the same rectangles), and a tab's
    indicator and label are direct children of its content row. 42 -> 37
    layout nodes per frame, frame CPU -3-6%. Pixel-identical on clean,
    dirty-tab, recent-files, and find-panel scenarios; chrome, tabs,
    recent-files, and state-trace behaviour tests pass.

### First-frame stall (vendored gpui patch 6)

Splitting the first render with trace marks (`render_syntax_ms`,
`window_title_ms`, `render_char_width_ms`, kept as diagnostics) showed the
first frame's wall time at 2-3x its CPU time because `set_window_title`
waited for the X server to acknowledge two property writes while it was
still mapping the window: 14-28 ms per run. The warm-up thread (grammar
queries and font resolution, now also traced as `warm_grammars_ms` and
`warm_fonts_ms`) was not the blocker; it finishes 35-45 ms before the first
render. The vendored X11 `set_title` now sends both writes and flushes
without waiting, and the app no longer re-sets the creation-time title
after opening the window. First frame wall 28-42 -> 13-16 ms (equal to its
CPU time); `open-small` 334 -> ~260 ms in the current environment.

### Glyph tile cache (vendored gpui patch 7)

20. GPUI's `paint_glyph` did two locked hash lookups per glyph per frame
    (raster bounds in the text system, tile in the atlas). Glyph tiles are
    never removed from the atlas, so the window now caches (raster bounds,
    tile) per glyph variant and consults that first. Viewport paint
    0.34 -> 0.22 ms per frame (`latency-navigation`), scroll paint
    233 -> 126 ms per 3 s run and scroll CPU 1370 -> 1130 ms. Pixel-identical.
    The X11 cursor-style attribute change also no longer waits for a reply.
21. **Two fewer layout levels** (`shell.rs`): the tab strip, editor, and
    status bar are direct children of the root column, and the editor's
    focus and key handlers sit on the viewport element itself. Same
    geometry (pixel-identical on clean, find-panel, recent-files, and
    multi-cursor scenarios; 76 focus/key/surface behaviour tests pass);
    frame CPU within noise (-3%), kept as the simpler tree.
22. **Backgrounds and carets in layers** (`viewport.rs`): a quad painted
    outside a layer costs a bounds-tree insertion whose overlap walk grows
    with every quad already painted, so a thousand carets (one highlight
    and one caret quad per visible row) made the paint quadratic. The
    background pass and the outline/caret pass each paint inside one layer
    now, with the same draw-order relations. `multi-cursor-1k` viewport
    paint 3.7 -> 0.34 ms; pixel-identical on multi-cursor, identifier,
    inactive-selection, and find-panel scenarios.
23. **One X11 connection for the modifier query** (`input.rs`): every key
    press queried the pointer's modifier mask through a freshly opened X11
    connection (connection setup plus a round trip, mostly waiting on the
    server). The query keeps its semantics on one process-wide connection.
    `latency-typing` key_delivery p50 0.60 -> 0.45 ms; 113 input,
    modifier, chord, multi-cursor, vim, and find behaviour tests pass.
