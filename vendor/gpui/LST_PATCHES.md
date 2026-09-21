# lst patches to gpui 0.2.2

This directory is the `gpui` 0.2.2 crates.io release (docs, examples, and
tests removed) with the changes below. The workspace selects it through
`[patch.crates-io]` in the root `Cargo.toml`; delete that section to build
against the unpatched release. `lst.patch` is the exact diff against the
crates.io sources, for re-applying to a newer gpui.

Every change is marked `lst patch` in the source.

1. **Draw right after input** (`src/platform.rs`, `src/window.rs`,
   `src/platform/linux/x11/client.rs`). The X11 backend only draws when its
   periodic refresh timer fires, so a keystroke waited up to one refresh
   period (16.7 ms at 60 Hz) before the frame that shows it was even rendered.
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
3. **Parallel system font scan** (`src/platform/linux/text_system.rs`,
   `Cargo.toml`). `FontSystem::new()` walks every fontconfig directory and
   parses every font file on the main thread before anything else can
   happen; with 12k font files that is ~200 ms of a ~300 ms startup, nearly
   all of it per-file syscalls. The scan now follows the same fontconfig
   configuration, aliases, and cosmic-text default families, but parses the
   files on up to eight threads and reads directory entry types instead of
   calling `stat` twice per entry. The resulting database is identical in
   content; only its construction differs.

4. **GPU context on a startup thread** (`src/platform/linux/x11/client.rs`).
   Creating the Vulkan instance and device took ~120 ms with the NVIDIA
   driver, serialised after the font database. It now starts on its own
   thread as the first step of the X11 client and is joined where the
   context is first needed, so it overlaps the font scan and the X11 setup.
   First frame on the reference host ~300 -> ~240 ms.

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
   now sent and flushed without waiting.

The `Cargo.toml` here also drops the crate's example and test targets whose
sources are not vendored.
