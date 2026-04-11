# CLAUDE.md

## Active Work

The active editor is the GPUI implementation in `apps/lst-gpui`.
The repository root is a workspace-only manifest; it is not an application crate.

Do not use `legacy/iced-lst` for new work unless the user explicitly asks for
legacy iced investigation. That directory is archived and intentionally
excluded from the active workspace.

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
- `crates/lst-ui`: reusable GPUI widgets for tabs, inputs, and shell UI.
- `benchmarks`: active benchmark corpora shared by GPUI examples.

Behavior should move toward `lst-editor` and `lst-core` when it can be tested
through command/effect or document-level contracts. GPUI should mostly adapt
desktop events to those contracts and render observable state.
