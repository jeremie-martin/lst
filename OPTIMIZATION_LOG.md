# Optimization log

Current results from 23–24 September 2026, starting at `8accfc9`.
Production changes through `82b5b24`; detailed experiments, rejected candidates,
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

## Retained gains

These are individual paired experiments, not additive speedups. Full details,
phase measurements and qualifications are preserved in the history.

| Workload | Before → after | Root cause / change |
| --- | --- | --- |
| External 2.98 MB paste, input through paint | 737 → 92 ms | bounded clipboard socket readiness instead of per-chunk sleeps |
| 500k-line no-wrap, 320 backspaces on the widest line | 23,561 → 199 ms | borrow full-scan lines, then retain per-line widths and reduce cached numbers |
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

Startup remains dominated by overlapping GPU/font initialization and first
surface creation. Minimal Vulkan probes reproduce much of the driver cost.
No substantial new whole-startup gain is established: a deferred-font worker
was slower/no better and was removed; the native GLES experiment was also slower.
No driver installation or environmental change is counted as an editor gain.

Large-Rust typing still spends roughly 210 ms per 320 keys in incremental parsing.
Deferring parsing until render did not batch it: GPUI rebuilds a dirty dispatch
tree before the next key. That experiment was removed. Skipping those renders
without a sound input-routing design would risk stale contexts and handlers.
Static viewport caching was not added without a demonstrated worthwhile gain.

## Verification

- Final source gates pass: `cargo test --all-features`,
  `cargo test -p lst-editor --features internal-invariants`,
  `cargo clippy --all-targets --all-features`, formatting and diff checks.
- Earlier combined production checkpoint: **290/290** nested X11 tests pass.
- Eight focused viewport tests pass, including widest-line growth, shrink,
  split/join extents, wrapped cursor visibility and responsive geometry.
- The current **291-test full X11 checkpoint is running**. Final physical-display
  benchmark and ten-scenario pixel comparisons follow it.
