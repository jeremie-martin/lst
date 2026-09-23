# lst patches to gpui 0.2.2

This directory is the `gpui` 0.2.2 crates.io release (docs, examples, and
tests removed) with the changes below. The workspace selects it through
`[patch.crates-io]` in the root `Cargo.toml`; delete that section to build
against the unpatched release. `lst.patch` is the exact diff against the
crates.io sources, for re-applying to a newer gpui.

Every change is marked `lst patch` in the source.

1. **Draw right after input** (`src/platform.rs`, `src/window.rs`,
   `src/platform/linux/x11/client.rs`, `src/platform/windows/events.rs`). The
   X11 backend only draws when its periodic refresh timer fires, so a keystroke
   waited up to one refresh period (16.7 ms at 60 Hz) before the frame that
   shows it was even rendered.
   After handling a batch of X11 input events, every window that the input
   left dirty is now drawn and presented immediately; clean windows and
   next-frame callbacks are left to the timer as before.
2. **Fastest monitor refresh rate, only while active**
   (`src/platform/linux/x11/client.rs`, `src/window.rs`). The refresh timer
   took the mode of the first CRTC, which on a multi-monitor host can be a
   slower secondary display (60 Hz here while the editor sat on a 144 Hz
   monitor). It now uses the fastest active CRTC for 1.2 s after any X11
   event batch, which covers animations such as smooth scrolling and GPUI's
   own one-second re-presentation after input, and falls back to a 60 Hz
   tick otherwise so an idle window costs no more wakeups than before. A
   tick with nothing to run, draw, or present also returns before entering
   an app update.
3. **Parallel system font scan — retired.** Restored upstream
   `FontSystem::new()` and removed the custom fontconfig/directory parser and
   its three direct dependencies. On the current host, font loading fits
   within GPU initialization. Eleven whole-app launches per variant gave
   medians ~174 ms parallel versus ~178 ms upstream, with much larger
   within-variant spread; isolated font timing exaggerated the benefit.
   Prefer upstream behavior and portability to maintaining ~170 extra lines
   for that small uncertain end-to-end difference. Reconsider only if a
   whole-launch profile puts font loading on the critical path.

4. **GPU context on a startup thread** (`src/platform/linux/x11/client.rs`).
   Creating the Vulkan instance and device took ~120 ms with the NVIDIA
   driver, serialised after the font database. It now starts on its own
   thread as the first step of the X11 client and is joined where the
   context is first needed, so it overlaps the font scan and the X11 setup.
   First frame on the reference host ~300 -> ~240 ms.
   Restrict the worker to Vulkan: the GLES backend keeps its thread-bound
   EGL context on the platform thread, preserving that backend's build contract.

5. **Sprites sorted by texture, not tile** (`src/scene.rs`). `Scene::finish`
   sorted every glyph sprite by (order, tile id) each frame; draw batches
   are only cut when the texture changes, so the key is now (order, texture
   index). Glyphs painted in text order from one atlas texture are already
   sorted and the stable sort degenerates to a scan (0.18 -> 0.10 ms for
   ~860 sprites). Same-order sprites keep their paint order; the pixel lane
   is identical.

6. **Non-blocking window title** (`src/platform/linux/x11/window.rs`).
   `set_title` waited for the reply of each of its two property writes. On
   the first frame the server is still mapping the window, so the wait
   stalled render by 15-30 ms (the first frame's wall time was 2-3x its
   CPU time); every later title change cost a round trip. Both writes are
   now sent and flushed without waiting, as is the cursor-style attribute
   change in `src/platform/linux/x11/client.rs`, which runs during painting
   whenever the pointer crosses into a region with another cursor.

7. **Per-window glyph tile cache** (`src/window.rs`). `paint_glyph` looked
   up every glyph's raster bounds behind the text system's lock and its
   atlas tile behind the atlas lock, twice per glyph per frame. Glyph tiles
   are never removed from the atlas, so the window keeps one map from glyph
   parameters to (raster bounds, tile) and consults it first. Viewport
   paint 0.34 -> 0.22 ms per frame; scrolling CPU -18%. Pixel-identical.

8. **Raster-only GPU context** (`src/platform/blade/blade_context.rs`). GPUI
   does not use ray tracing. Explicitly disable it using the narrow upstream
   Blade backport documented in `../blade-graphics/LST_PATCHES.md`, avoiding
   unnecessary driver initialization. Keep this flag when upgrading Blade;
   the backport itself can then be removed.

9. **Ignore unchanged X11 geometry** (`src/platform/linux/x11/window.rs`).
   Some WMs repeatedly send identical synthetic ConfigureNotify messages.
   GPUI unconditionally invoked resize and move callbacks, triggering full
   renders even when the drawable and bounds were unchanged. Only invoke
   callbacks for a logical resize, actual drawable resize, or actual move.
   Continue querying drawable geometry (events can describe intermediate
   sizes), and always process pending XSync counters. Reference-host idle
   CPU per two seconds fell from 160 to 40 ms and paints from 80 to four.


10. **Wait for clipboard socket readiness**
    (`src/platform/linux/x11/clipboard.rs`). The synchronous selection reader
    polled events and slept for 1 ms whenever the next event was not ready.
    An INCR transfer with many small chunks paid that delay repeatedly
    (833 ms retrieving 2.98 MB from xsel on the reference host). After draining
    queued events, poll the reader's private socket until readiness or the
    existing deadline. Preserve both initial and per-chunk timeouts, retry
    interrupted waits, and propagate other I/O errors. This belongs in the
    transport, independent of editor text handling. Remove when upstream
    provides an equivalent bounded readiness wait.


11. **Allocate path raster targets on demand**
    (`src/platform/blade/blade_renderer.rs`). Full-window resolved and MSAA
    textures were allocated at window creation and after every resize, even
    for scenes containing no paths. One optional owner now holds the resolved
    texture/view and the optional MSAA texture/view pair; the first path batch
    creates them. Resize and surface-format changes release the old target
    after GPU completion, and a later path batch recreates it. Existing
    descriptors, rasterization, antialiasing, and compositing are unchanged.
    The grouped resource owner also removes duplicated allocation/destruction
    and independently optional MSAA handles. Remove when upstream allocates
    these resources on demand.
    Reference-host production GPU memory falls from 208 to 29 MiB at
    3816×2100 (three paired launches); CPU RSS is unchanged. A path-rendering
    probe is pixel-identical before/after resize and hide/show with 1× and
    4× sampling. Full path resources appear normally when paths are drawn.

The `Cargo.toml` here also drops the crate's example and test targets whose
sources are not vendored. It suppresses upstream dead-code and unreachable-code
warnings that become visible when a registry dependency is built as a path dependency.
