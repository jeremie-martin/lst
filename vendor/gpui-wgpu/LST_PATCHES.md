# lst patches to gpui-pre-wgpu 0.3.10

Published source: `gpui-pre-wgpu` 0.3.10, Zed revision
`08cbb88a152c3751a43e853425a70e9f631a5918`. The workspace selects this directory
through `[patch.crates-io]`. `lst.patch` records the exact source diff against the
published archive; the change is marked `lst patch`.

**Allocate path targets only for scenes containing paths.** Upstream already owns
lazy creation, resize invalidation and recovery of intermediate textures, but
calls their allocator for every frame. Guard that call with `scene.paths`.
Ordinary text scenes then avoid full-window resolved and MSAA path textures.
Path rasterization, sampling and compositing remain upstream. This replaces the
larger Blade resource-owner patch without adding a second resource lifecycle.

Remove when upstream guards allocation equivalently. Validate text-only memory
usage and path rendering across resize/recovery, alongside real-app acceptance
and visual comparisons. Migration evidence is tracked in `docs/findings.md`.

The source-side GPU regression covers text-only frames, later path rendering,
resize and target invalidation. The published manifest omits `naga`, required by
its existing shader-validation tests; the dev dependency is restored here.
Running the upstream tests also needs their three pinned Zed font fixtures under
`assets/fonts` two levels above this crate. This test setup does not affect lst's
production dependency features.
