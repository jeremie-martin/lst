# lst patches to blade-graphics 0.7.1

This is the crates.io 0.7.1 source, selected through the workspace patch table.
The upstream revision is `88b6c64cd7a32dd933acc4e5266808a07143ce84`.
Documentation images and the unused `etc` directory are omitted. LICENSE is
from that revision's repository root. `lst.patch` contains every source change.

## Explicit Vulkan ray-tracing opt-in

Backport the `ContextDesc::ray_tracing` flag and Vulkan capability gate from
[upstream c7edf7b6](https://github.com/kvark/blade/commit/c7edf7b6f036f6242e5970bbf5aca3ea17cb7dfd).
GPUI explicitly sets it to false. Its shaders use no acceleration structures,
ray queries, binding arrays, or buffer device addresses. The existing Vulkan
capability gate already controls the corresponding extensions, feature chain,
buffer usage, allocator configuration, and shader capabilities together.

The other hunks in that upstream commit add independent buffer-device-address
support for newer Blade clients; they are not needed by 0.7.1 or GPUI and are
not backported. As in the upstream change, the new flag gates Vulkan; the Metal
and GLES implementations are unchanged.

This avoids loading/initializing unused NVIDIA ray-tracing support. A single
isolated context probe alternating the flag measured ~90 ms enabled versus
~70 ms disabled on the reference host. Whole-editor measurements and validation
are recorded in `OPTIMIZATION_LOG.md`.

Remove this vendor copy when GPUI can use a released Blade with the upstream
flag. This deliberately avoids taking the unrelated API and renderer changes
on Blade main just to configure one optional device capability.
