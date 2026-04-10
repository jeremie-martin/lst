# GPUI Paste Lab

This is an isolated GPUI spike for `lst`.

It is intentionally not wired into the shipping `iced` app. The point is to test whether a custom buffer plus GPUI's rendering/input stack looks like a viable foundation for large-file and large-paste work.

It also lives in its own nested Cargo project on purpose. Putting GPUI in the root `lst` package caused dependency-graph conflicts with the current `iced` stack.

## Run

```sh
cd spikes/gpui-paste-lab
cargo run
DISPLAY=:1 cargo run -- --bench-replace-corpus
DISPLAY=:1 cargo run -- --bench-append-corpus
```

## Status

- `cargo check` passes for this spike crate.
- `cargo build` and `cargo build --release` pass on this host after installing `libxkbcommon-x11-dev`.
- Running under `Xvfb` still does not work here because GPUI surface creation wants a real presentation backend with DRI3 support.
- This spike now exits with a clear error instead of panicking when launched from a headless or fake-display environment.
- The viewport no longer uses `uniform_list`; it now uses a scroll spacer plus a viewport-sized custom-painted canvas with a small shaped-line cache.

Current real-display measurements on `DISPLAY=:1` with `target/release/lst-gpui-paste-lab`:

- `--bench-replace-corpus`: `apply_ms=0.993`, `action_to_next_frame_ms=55.532`
- `--bench-append-corpus`: `apply_ms=0.867`, `action_to_next_frame_ms=44.353`

## Shortcuts

- `Ctrl-R` or `Cmd-R`: reload the premade 20k-line Rust corpus
- `Ctrl-V` or `Cmd-V`: replace the buffer from the clipboard
- `Ctrl-Shift-V` or `Cmd-Shift-V`: append clipboard text to the buffer
- `Ctrl-L` or `Cmd-L`: clear the buffer
- `Ctrl-G` or `Cmd-G`: toggle the line gutter
- `Ctrl-Q` or `Cmd-Q`: quit

Every bulk operation logs timing to stderr in the form `lst_gpui_spike ... apply_ms=...`.

Auto-bench mode runs one bulk replace or append after the first rendered frame, waits for the next frame, prints a `lst_gpui_spike bench ...` line, and exits.

## Scope

The spike currently includes:

- a `Ropey` text buffer
- a custom-painted viewport driven by a scroll spacer and shaped-line cache
- a simple action bar
- clipboard-driven replace/append operations
- an auto-bench mode for bulk replace and append from the 20k corpus or any file

It does not include:

- selections or cursor movement
- syntax highlighting
- search
- file saving/loading
