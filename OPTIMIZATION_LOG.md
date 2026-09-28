# Optimization log

Results through 28 September 2026, starting at `8accfc9`.
Detailed experiments, rejected candidates,
individual run counts and earlier sessions are in the
[measurement history](docs/optimization-history.md).

## Measurement contract

- Reference host: i9-14900K, RTX 4090, 3840×2160@144 Hz, scale 2;
  production client 3816×2100 on physical `:0`. Xephyr is for correctness only;
  do not run physical performance/pixel comparisons while it is active.
- Compare preserved production binaries with the same runner, corpus, geometry,
  priming and completion boundary. Editing runs verify exact saved output.
  Raw session outputs/traces are under `/tmp/lst-perf-sep23` and
  `/tmp/lst-gpui-bench-*`; commands are in [Performance](docs/performance.md).
- Fixed throughput gates to wait for the final operation's completed frame.
  Typing and paste now use injection/completed-frame epochs, excluding the
  runner's 10 ms polling wakeup delay. Incomplete trailing records never count.
  Historical numbers are comparable only within their stated paired experiment.
- Complete-root frame tracing replaced the old viewport-only endpoint in
  `65796dc`. Use that preserved build for the final broad frame-cost comparison;
  don't compare old and new frame CPU boundaries as if they were identical.
  Search also reports query injection through the final completed frame;
  its indexing-only metric remains separate.

## Retained gains

These are individual paired experiments, not additive speedups. Full details,
phase measurements and qualifications are preserved in the history.

| Workload | Before → after | Root cause / change |
| --- | --- | --- |
| External 2.98 MB paste, input through paint | 737 → 92 ms | bounded clipboard socket readiness instead of per-chunk sleeps |
| 500k-line no-wrap, 320 backspaces on the widest line | 23,780 → 176 ms (~135×) | direct original-baseline comparison; retain widths and reduce cached numbers |
| Widest-line deletion, final numeric reduction | 205 → 171 ms / 320 keys | eight independent accumulators, preserving pixel ordering |
| 30k-line mixed Unicode/ASCII no-wrap startup through syntax paint | 5.86 → 3.10 s | retain unchanged widths after background parsing; first frame remains ~3.02 s |
| 500k-line wrapped, 320 newlines | 3,496 → 209 ms (~17×) | patch row counts and shift numeric suffixes |
| 500k-line no-wrap, 320 newlines | 3,879 → 93 ms (~42×) | replace changed line windows instead of rebuilding all measurements |
| Growing widest line, 500k-line no-wrap document | 69.708 → 0.382 ms/character | remeasure changed widths; retain unaffected maximum |
| Find with 6,144 matches, query update through final root paint | 44.824 → 17.060 ms | convert only visible matches, located by binary search |
| Plain typing, modifier-state comparison | 0.413 → 0.319 ms/character | use GPUI modifier state, remove an X11 round trip per key |
| Cached tab chrome, navigation process CPU | 3,140 → 2,600 ms | retain unchanged tab elements |
| Cached status chrome, navigation process CPU | 2,610 → 2,230 ms | retain unchanged status elements; publish geometry before root paint |
| GPU memory at reference window size | 208 → 29 MiB | allocate path raster/MSAA targets only when paths are drawn |
| Unwrapped layout, 500k-line document RSS | 247,796 → 243,940 KiB | represent unwrapped rows as an identity mapping |
| 500k-line wrap construction, LF chunk proof | 17.984 → 11.773 ms | prove LF-only chunks once; retain Ropey parsing otherwise |
| Repeated 2.98 MB replacement, wrap patch phase | 14.191 → 3.941 ms | traverse changed rope lines sequentially |

A direct comparison with `8accfc9` plus corrected root tracing confirms plain
typing **0.467 → 0.312 ms/character**, scrolling frames **1.746 → 1.127 ms**,
and edit-navigation frames **1.101 → 0.632 ms** (CPU **3,340 → 2,590 ms**).
Presentation latency is nearly flat at **6.814 → 6.620 ms**.

The final 15-scenario cohort (`65796dc` → `e69a78a`) confirms plain typing
**0.461 → 0.300 ms/character**, large Rust **1.117 → 1.006**, and complete find
query paint **31.766 → 13.419 ms**. Ordinary startup, scrolling, idle and isolated
typing latency are broadly flat. Edit-then-navigation presentation p50 rises
**0.4–0.7 ms** in two comparisons despite flat frame cost and lower CPU; intermediate
build checks do not isolate a cause. Keep this measured regression visible.

The width index costs four bytes per measured logical line (~1.9 MiB at 500k).
Shrinking maxima and row-offset suffix shifts remain linear numeric operations;
unchanged text is neither reshaped nor rescanned. `LineChange` validates old/new
windows, and `WrapLayout` owns row-count invariants. Multiple unmeasured topology
changes conservatively rebuild widths. Full-window replacements rebuild once at
the final gutter/viewport width; a measured duplicate-scan paste regression was
removed before retaining the change. Final paste application cost is unchanged
in its paired check: 29.383 → 29.412 ms.

## Correctness fixes and measurement safeguards

- Removed the `u8` grapheme-length truncation: a 301-codepoint grapheme crashed
  the previous production app. Real-X11 select/copy/replace/save/undo and boundary
  tests cover the fix.
- Find now uses document line boundaries, fixing navigation/replacement after
  lone CR and other Unicode separators. A negative production test demonstrates
  the old failure; the fixed app preserves all separators during Replace All.
- Tree-sitter coordinates count LF rows and UTF-8 byte columns. A boundary test
  demonstrates the old mismatch. A short-lived adapter proves when the document
  index agrees, otherwise scans LF bytes; no persistent second index is added.
- Modifier handling retains GPUI's raw modifier state as well as event modifiers.
  A naive deletion of the X11 query failed held shifted-symbol shortcuts and was
  rejected; the retained implementation passes that real-input regression.
- Borrowed line visitors, incremental widths and wrapped rows match fresh
  indexes across fragmented Unicode edits, topology changes and empty windows.
- The large clipboard runner uses an owner that tolerates competing clipboard
  managers. Final-input gates, full-root timestamps and complete-record parsing
  prevent premature or polling-quantized performance claims.

## Vendor decisions and remaining costs

Retained GPUI patches are documented in [the patch ledger](vendor/gpui/LST_PATCHES.md),
with exact diffs in `lst.patch`. Both vendor reverse-application checks pass.
The GPU worker is restricted to Vulkan; GLES retains its thread-bound context.
Glyph-cache lifetime and sprite ordering were reviewed against atlas ownership
and paint order. Lazy path resources were checked with pixel-identical 1×/4×
path probes across resize and hide/show.

Ordinary startup remains dominated by overlapping GPU/font initialization and first
surface creation. Minimal Vulkan probes reproduce much of the driver cost.
Fourteen matched launches per variant give **186.043 → 187.281 ms**
first-frame medians, with reversed-order runs changing which build is faster.
No substantial new whole-startup gain is established: a deferred-font worker
was slower/no better and was removed; the native GLES experiment was also slower.
No driver installation or environmental change is counted as an editor gain.
Unwrapped Unicode-heavy files still pay for full-document advanced shaping.
The optional upstream shaping cache was rejected: an isolated probe shows wrong
mirrored glyphs across text directions. A future cache needs a complete key and
bounded lifetime.

Large-Rust typing still spends roughly 210 ms per 320 keys in incremental parsing.
Deferring parsing until render did not batch it: GPUI rebuilds a dirty dispatch
tree before the next key. That experiment was removed. Skipping those renders
without a sound input-routing design would risk stale contexts and handlers.
Static viewport caching was not added without a demonstrated worthwhile gain.

## Earlier verification

- Source gates pass: `cargo test --all-features`,
  `cargo test -p lst-editor --features internal-invariants`,
  `cargo clippy --all-targets --all-features`, all 12 benchmark-runner tests,
  formatting and diff checks.
- Final production `e69a78a`: **291/291 nested X11 tests pass** in 1,733.010
  seconds, with no skips. This includes all eight viewport cases, held modifier
  chords, separator-aware find and long-grapheme editing.
- All **ten original visual baselines match exactly**, three captures each.
- All 15 physical benchmark scenarios complete. Direct original-baseline pairs
  and huge-file edit checks verify exact saved output.
- Both vendor patch reverse-application checks pass. Earlier checkpoints and
  complete experiment details remain in the measurement history.

## 25 September 2026 checkpoint

- Lazy external-scanner lookup in tree-sitter `changed_ranges`: hot 0.115 →
  0.050 ms; cold 0.68 → 0.30 ms, with matching ranges across eleven grammars.
  Clean interleaved app pairs: large typing 0.957 → 0.885 ms/character;
  typing key-to-frame-end 4.60 → 4.16 ms. Patch rationale and removal criteria
  remain in `vendor/tree-sitter/LST_PATCHES.md`.
- Fixed first-frame cursor reveal after document-growing edits, first-present
  pairing (exclude mapping/resize damage), and partial trace-line reads.
- Startup remains dominated by overlapping font/device initialization and the
  first NVIDIA swapchain. A minimal Vulkan client takes 116–170 ms to create
  its first swapchain; surface/map reorderings did not reliably improve lst.
- Retained GPUI's one-second re-presentation: removing it saves some CPU but
  worsens presentation latency on this host. Also rejected presenting the
  pre-dispatch frame (FIFO backlog), background parsing with stale highlights,
  and a whitespace-marker cache with negligible projected benefit.
- Cold clocks/caches materially change frame costs. A background transcode made
  the same editor look several times faster; those measurements were separated
  from idle-machine comparisons. Navigation differences are particularly noisy.

Commands, individual comparisons, rejected experiments and environment controls
are in the [25 September history](docs/optimization-history.md#25-september-2026-session).

## 28 September 2026 checkpoint

Baseline `bf7c206`; production changes through `acab9d5`. Same i9-14900K,
physical `:0`, 3816×2100 client and production build profile. Preserved binaries,
profiles and raw logs: `/tmp/lst-perf-sep28`. Commands, intermediate results and
rejected candidates are in the [session history](docs/optimization-history.md#28-september-2026-session).

### Retained changes

| Existing model benchmark, 320 typed characters | Before | Final | Throughput gain |
| --- | ---: | ---: | ---: |
| Small corpus | 1.365 ms | 0.229 ms | 5.95× |
| Medium corpus | 1.381 ms | 0.231 ms | 5.98× |
| Large corpus | 1.429 ms | 0.265 ms | 5.40× |

Final matched original/final executables use identical Criterion settings: 2 s
measurement, 0.2 s warmup and 15 samples. These supersede preliminary typing
figures.

- Profiling put 65% of model typing CPU in repeated prefix grapheme iteration.
  Round from the requested position using Unicode's `GraphemeCursor`, borrow
  contiguous rope lines, and supply complete line context when necessary.
  Reuse an already validated endpoint for ordinary insertions; interior-cluster
  inputs still expand to the complete cluster. No persistent cache or ASCII
  exception. An exhaustive complete-line oracle covers every character offset
  in long combining clusters, emoji joins, flags, Indic conjuncts, separators
  and shifted rope chunk boundaries.
- A paired navigation control caught unnecessary context preparation at column
  zero. Returning that known boundary restores wrapped-page work from 2.33 ms
  to 2.20 ms (old binary 2.23–2.27 ms).
- Binary lookup uses the existing ordered find index for nearest, exact and
  strict next/previous positions. No new index. Existing 512-step Vim search
  benchmarks isolate a 1.6–4% improvement from this lookup change. Exhaustive
  comparison preserves empty-index, exact-hit, strict-direction and wraparound
  behavior. Including the boundary changes, final next-match work falls 29–33%
  versus the original baseline (small/medium/large: 330/455/582 → 234/317/392 µs).
  Vertical Vim motion falls 11–20%; an original/final/original control confirms
  both improvements, including variability in the small vertical case.

These are **model** improvements. No substantial desktop-latency or startup
improvement is established. All 15 final physical benchmark scenarios complete
and editing scenarios verify saved output. Six interleaved samples per binary
(before/final/final/before blocks) give large typing 0.906 → 0.889 ms/character,
typing latency p50 7.234 → 7.333 ms, and 1k-cursor paint 0.516 → 0.509 ms.
These small differences do not establish a substantial desktop gain or regression.

### Rejected work and remaining costs

- Discarded a chunk-feeding Unicode cursor adapter after a differential test
  found a wrong boundary in a long joined-emoji sequence. Complete-line context
  is simpler and passes the oracle.
- Built and measured a cached GPUI child scene for immutable code/marker text,
  preserving glyph identity, geometry and paint order. Navigation mean frame
  0.662 → 0.642 ms and CPU 1810 → 1780 ms; highlighted scrolling worsened
  1.113 → 1.145 ms and CPU 820 → 860 ms. Removed it: scene replay still copies
  and finishes the scene, and comparison/layout adds work on changing frames.
  Complete-root timing and process CPU include the new child's prepaint work.
- Page-navigation profiling puts only ~3% in character iteration, with rope
  indexing/slice metadata dominating. No speculative line-length shortcut.
- Existing vendor patches remain unchanged and all three reverse-application
  checks pass. Their documented startup, transport, geometry, glyph and GPU
  resource-lifetime gains still justify them. Remaining large costs are Vulkan
  initialization/presentation, cold incremental parsing and whole-window frame
  construction. No environment or clock changes count as editor improvements.

### Verification

Source tests, internal-invariant/Vim suites, formatting and Clippy pass.
The earlier grapheme checkpoint passes fifteen of sixteen focused physical
motion/input tests; the held-arrow test
hits the 120-second timeout on both untouched baseline and candidate, with no
assertion or timeout relaxed. The final release passes **271/271 nested X11
tests** in 1828.857 seconds (two expected slow cases, no skipped tests), including
the held-arrow case in 90.578 seconds. The tested binary matches the physical
benchmark binary by SHA-256; before/final benchmark runners are byte-identical.
