# GPUI Paste Lab

This is an isolated GPUI spike for `lst`.

It is intentionally not wired into the shipping `iced` app. The point is to test whether a custom buffer plus GPUI's rendering/input stack looks like a viable foundation for large-file and large-paste work.

It also lives in its own nested Cargo project on purpose. Putting GPUI in the root `lst` package caused dependency-graph conflicts with the current `iced` stack.

## Run

```sh
cd spikes/gpui-paste-lab
cargo run
```

## Status

- `cargo check` passes for this spike crate.
- `cargo build` passes on this host after installing `libxkbcommon-x11-dev`.
- Running under `Xvfb` still does not work here because GPUI surface creation wants a real presentation backend with DRI3 support.
- This spike now exits with a clear error instead of panicking when launched from a headless or fake-display environment.

## Shortcuts

- `Ctrl-R` or `Cmd-R`: reload the premade 20k-line Rust corpus
- `Ctrl-V` or `Cmd-V`: replace the buffer from the clipboard
- `Ctrl-Shift-V` or `Cmd-Shift-V`: append clipboard text to the buffer
- `Ctrl-L` or `Cmd-L`: clear the buffer
- `Ctrl-G` or `Cmd-G`: toggle the line gutter
- `Ctrl-Q` or `Cmd-Q`: quit

Every bulk operation logs timing to stderr in the form `lst_gpui_spike ... apply_ms=...`.

## Scope

The spike currently includes:

- a `Ropey` text buffer
- lazy line rendering via `gpui::uniform_list`
- a simple action bar
- clipboard-driven replace/append operations

It does not include:

- selections or cursor movement
- syntax highlighting
- search
- file saving/loading
- a benchmark harness
