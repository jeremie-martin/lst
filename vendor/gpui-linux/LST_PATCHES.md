# lst patches to gpui-pre-linux 0.3.10

Published source: `gpui-pre-linux` 0.3.10, Zed revision
`08cbb88a152c3751a43e853425a70e9f631a5918`. The workspace selects this directory
through `[patch.crates-io]`. `lst.patch` is the exact source diff against that
published archive. Every change is marked `lst patch`.

These preserve the relevant X11 protections from our GPUI 0.2.2 patch set:

- **Frame demand and immediate input drawing.** Use upstream `frame_waker` and
  `schedule_frame` hooks to coalesce requests in one per-window demand owner.
  Its revision distinguishes new input demand from an animation already waiting
  for the refresh clock. Flush new visible demand after an X11 event batch,
  coalescing any Expose request for the same window. The timer handles async invalidation,
  next-frame callbacks and recovery; clean idle windows avoid entering GPUI.
  Consume pending demand before drawing so requests raised during a frame survive.
  Keep the input presentation tail and upstream recovery/forced-render path.
  No GPUI-core frame API changes are needed. Global modifier routing uses the same
  visibility predicate as rendering, without suppressing keyboard-layout updates.
- **Newest-frame presentation.** Request mailbox presentation on X11, matching
  Blade's former `DisplaySync::Recent` preference. Upstream's capability check
  falls back to FIFO when mailbox is unavailable. The application does not own
  swapchain state or implement an alternate rendering path.
- **Checked refresh periods.** Select the fastest valid active RandR mode, falling
  back to 60 Hz when the extension, query or timing data is unusable. Convert
  timings directly without a rounded-zero divisor. After 1.2 seconds without an
  event batch, cap timer wakeups at 60 Hz. Upstream owns hide/show timer removal.
- **Window titles and cursor updates.** One title writer emits UTF-8 EWMH and
  Latin-1 legacy properties, substituting `?` for unrepresentable legacy characters.
  Use it at creation and update; title and paint-time cursor writes do not wait
  for server round trips.
- **Unchanged geometry.** Notify resize/move only for actual changes. Continue
  querying drawable size and acknowledging XSync counters, including repeated
  ConfigureNotify events and queued intermediate sizes.
- **Clipboard readiness.** Wait on the selection reader's private socket after
  draining queued events. Preserve transfer deadlines, retry EINTR and propagate
  other I/O errors rather than sleeping 1 ms per INCR chunk.

Remove each patch when upstream supplies equivalent behavior, verified through
lst's production X11 suite and controlled performance comparisons. Migration
verification and measurements are tracked in `docs/findings.md`; source inspection
alone is not performance evidence.

## Disposition of the old core and Blade patches

GPUI core is now an unmodified published dependency. Texture-based sprite sorting
is upstream. The old glyph-tile cache is not copied: wgpu can clear its atlas on
recovery, so its never-evicted-tile assumption no longer holds. Measure the new
painting path before adding a replacement cache. System font scanning remains
upstream. Blade's ray-tracing flag and GPU initialization thread do not apply to
the new renderer; startup must be evaluated end to end. Path-target allocation
is handled by the much smaller patch in `vendor/gpui-wgpu/LST_PATCHES.md`.
