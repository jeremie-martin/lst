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
2. **Fastest monitor refresh rate** (`src/platform/linux/x11/client.rs`).
   The refresh timer took the mode of the first CRTC, which on a
   multi-monitor host can be a slower secondary display (60 Hz here while
   the editor sits on a 144 Hz monitor). It now uses the fastest active
   CRTC. Animations such as smooth scrolling tick at that rate.
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

The `Cargo.toml` here also drops the crate's example and test targets whose
sources are not vendored.
