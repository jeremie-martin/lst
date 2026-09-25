# Optimization log

Current results from 23–24 September 2026, starting at `8accfc9`.
Production changes through `e69a78a`; detailed experiments, rejected candidates,
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

## Verification

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

## 25 September 2026 session

Baseline `87f85a7` on `:0` (five runs, one priming): typing latency p50
7.705 ms (delivery 0.345, key to frame end 4.568, frame end to damage 2.532),
navigation 6.178 ms, edit-navigation 6.362 ms; typing 0.273 / 0.781 / 0.956
ms per character (plain / medium / large Rust); mixed paste 71.1 ms; scroll
frames ~1.04 ms; idle CPU 20 ms per two seconds.

### Startup: what the first visible frame waits for

`open_to_first_frame_ms` stops at the app's own first frame, which precedes
the window manager mapping the window and GPUI's first present by 20–55 ms.
The runner now also records `open_to_first_present_ms` (first damage on the
editor window, attached from `CreateNotify` so it cannot miss the frame) and
uses it as the `open-small` primary. Baseline median 172 ms (163–228), app
first frame 155 ms.

Temporary GPUI/Blade instrumentation (not retained) gives the critical path on
this host: Vulkan instance/device ~80 ms on the startup thread, overlapped by
the ~70 ms system font scan on the main thread; then window and surface
creation ~45 ms, of which the first `vkCreateSwapchainKHR` alone is 31–60 ms
(later reconfigurations take 1.4–2 ms); app construction and first frame
~10–30 ms; then the window manager's map. A plain X11 window (including an
ARGB visual and GPUI's WM protocols) maps in 2–10 ms here, but while the
NVIDIA swapchain is being created other clients stall, so the editor's map
completes 40–60 ms after its request.

Rejected reorderings, each measured on `:0`:
- Mapping before creating the surface on the main thread: the window manager
  stalls behind the swapchain creation; first draw unchanged (170–196 ms).
- Creating the surface at the first draw: mapping completes in ~1 ms, but the
  45 ms surface creation then follows it serially (168–189 ms).
- Creating it on a worker thread from map time: the window manager resizes the
  window after mapping (2720×1720 → 3816×2100), and the follow-up
  reconfiguration from the main thread takes ~40 ms instead of 2 ms, i.e. the
  driver's first-use cost appears per thread; no reliable gain.

Roughly 110–140 ms of startup is therefore NVIDIA driver time (device plus
first swapchain), which the editor cannot remove without changing driver or
backend; the font scan is fully hidden behind it.

### GPUI's one-second re-presentation after input: kept

GPUI keeps presenting for one second after any input (a macOS-motivated
workaround). Limiting it to macOS cut CPU by ~6%, `open_to_quiet` from 1242 to
277 ms and damage events per run from 141 to 3, but worsened latency on this
NVIDIA host: frame end to damage for navigation 2.72 → 3.68 ms and for typing
2.28 → 2.65 ms, and typing p50 7.47 → 7.94 ms. Continuous presenting keeps the
GPU clocked up between keys. Rejected; GPUI unchanged.

### Startup floor measured without lst

A 60-line C program (instance, device on a second thread, XCB window,
surface, FIFO_RELAXED swapchain) needs 116–170 ms to get its first swapchain
on this host: instance 19–76 ms (bimodal), device 45–51 ms, first swapchain
36–61 ms even for a 64×64 window (recreations take 0.5 ms). Surface capability
queries made while the device is created do not pre-pay the swapchain cost.
lst's first present (median 172 ms) is 10–30 ms above that floor, which is
app construction plus the first frame; the rest is driver work that any
Vulkan client pays.

### Keystroke parse cost: where it goes

`latency-typing` spends ~2.3 ms of its ~4.4 ms key-to-frame-end in
`TabSyntaxState::update`, against 0.33 ms for the same edits back to back.
A standalone tree-sitter probe reproduces both numbers: with a 70 ms pause
before each edit, pinned to a P-core, the parse is 7× slower, i.e. caches
emptied by deep C-states while idle, not app code. A background parse was
rejected in an earlier session (a frame with stale highlights), and I agree.

Instead, less memory touched per edit. `Tree::changed_ranges` costs as much as
the incremental parse (0.115 ms hot, 0.68 ms cold, on the 660 KB corpus) and
nearly all of it is `ts_subtree_last_external_token`, run eagerly for every
subtree the diff skips. `vendor/tree-sitter` resolves it only when a
comparison needs it (exact; differentially fuzzed on nine grammars):
0.115 → 0.050 ms hot, 0.68 → 0.30 ms cold. tree-sitter 0.27.0 is no faster.

Interleaved A/B on `:0` (9 samples per variant, base `87f85a7`):
typing key-to-frame-end 4.43 → 3.94 ms, key-to-paint p50 7.38 → 7.04 ms;
typing-large 0.962 → 0.879 ms/char; typing-medium 0.787 → 0.759 ms/char;
edit-navigation unchanged.

Other checks this session, no change made:
- Mixed paste (71 ms): 55 ms is reading 3 MB from xclip, which offers it in
  4000-byte INCR chunks (746 lock-step round trips, 15–61 ms depending on
  core wake-ups); GPUI's reader already waits on its socket.
- Patch ledger: each GPUI/Blade patch still has its measured reason; the
  ray-tracing opt-out saves ~20 ms of device creation on the critical path.

### Presenting the pre-dispatch frame: rejected

GPUI draws a dirty window, without presenting, before dispatching a key event
to it. The benchmark's press and release arrive in one batch, so each key's
first frame is drawn before the release, the post-input refresh finds the
window clean, and the frame is re-rendered and presented at the next timer
tick. Presenting it at the end of the batch cut key-to-frame-end by 9–14%
but made key-to-damage worse in all three latency scenarios (typing p50
7.09 → 7.61 ms, navigation 6.29 → 6.90, p95 +13–16%; 9 samples each). With
GPUI re-presenting every tick for a second after input, an extra present
between ticks leaves one more image in the FIFO queue for every later
present to wait behind. Reverted.

### Runner fixes

- `open_to_first_present_ms` took the first damage on the editor window,
  which can come from mapping or a window-manager resize before any present
  (seen as a "present" before the app's first frame). It now takes the first
  damage at or after the app's first-frame stamp, and clears each damage so
  that later ones are reported. Frame to present is 1–30 ms on this host.
- Trace reads parsed a trailing partial line. The app writes each line in
  several `write` calls, so a wait or a frame pairing could see a truncated
  epoch. Trace readers now parse complete lines only.

### Large paste: first frame at the old scroll position (bug fix)

A paste that grows the document past the viewport rendered one frame at the
old scroll position, then scrolled to the cursor on the next frame. The
inline reveal clamped against the scroll handle's extent, which GPUI only
updates during layout, so it gave up and retried a frame later. The reveal
now clamps against the extent this frame lays out (content height and width
are computed just before it). `large_paste_reveals_the_cursor_in_its_first_frame`
fails on the old binary (first frame at `scroll_top 0`) and passes now; the
paste costs one frame fewer.

### The cold-core penalty, measured by accident

At 03:20 a Jellyfin `ffmpeg` transcode (one core, nice 10, GPU at P0) started
during a full benchmark pass of `6e8ed46`. With cores kept out of deep idle
states, and against the idle-machine baseline: scroll frames 1.04 → 0.33 ms,
multi-cursor paint 0.51 → 0.13 ms, typing latency p50 7.7 → 5.4 ms,
navigation 6.2 → 4.2 ms, latency-run CPU time −65%, with the same frames per
key. So, like the parse, lst's per-frame cost on an idle desktop is mostly
cold caches and clocks, not instructions. Holding a CPU latency QoS request
would need root (`/dev/cpu_dma_latency`) and costs power; not pursued.
Numbers from such a run are not comparable with idle baselines.

### Session result

Kept: `vendor/tree-sitter` (lazy external-scanner state in
`changed_ranges`), the first-frame cursor reveal after a document-growing
edit, and two runner fixes (first-present pairing, complete-line trace
reads). Final interleaved A/B of `cabc293` against `87f85a7` (six samples
each, taken while the Jellyfin transcode kept cores warm, which understates
the cold-core gain measured earlier on an idle machine): typing-large
0.961 → 0.890 ms/char, typing-medium 0.777 → 0.762, latency-typing
key-to-paint p50 6.32 → 5.55 ms, large paste 3.91 → 3.52 ms; navigation
key-to-frame-end unchanged (0.478 vs 0.477 ms).

Measured and rejected or left alone: a background parse (stale-highlight
frame), dropping the one-second re-presentation, earlier presenting of the
pre-dispatch frame (FIFO backlog), a lazy reuse token in the parser (no
gain), surface/map reorderings at startup (driver-bound), and caching the
whitespace-marker scan (1.9% of frame CPU).

What remains is structural: GPUI rebuilds and repaints the whole window for
each of the ~21 caret-animation frames per keystroke (~0.6 ms each), and
tree-sitter's cold-cache cost per edit. The first needs the text viewport in
a cached view with the caret painted outside it. That is worth a design of its
own, checked in the pixel lane, rather than a late-night patch.
