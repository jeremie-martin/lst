# CLAUDE.md

## Active Work

The active editor is the GPUI implementation in `apps/lst-gpui`.
The repository root is a workspace-only manifest; it is not an application crate.

The previous iced implementation has been removed from the repository. Treat
the GPUI app and framework-neutral crates as the only active implementation.

## Quick Commands

- Build active editor: `cargo build --release -p lst-gpui`
- Run active editor: `cargo run -p lst-gpui -- path/to/file.rs`
- Test active workspace: `cargo test`
- Test editor behavior with Vim internals: `cargo test -p lst-editor --features internal-invariants`
- Format active workspace: `cargo fmt --all`

## Architecture

- `apps/lst-gpui`: GPUI application, rendering, input adapter, file dialogs, clipboard, and runtime effects.
- `crates/lst-core`: framework-neutral document, selection, find, wrap, and line-edit primitives.
- `crates/lst-editor`: framework-neutral editor commands, effects, snapshots, and Vim state machine.
- `apps/lst-gpui/src/ui`: app-private GPUI widgets for tabs, inputs, and shell UI.

Behavior should move toward `lst-editor` and `lst-core` when it can be tested
through command/effect or document-level contracts. GPUI should mostly adapt
desktop events to those contracts and render observable state.
