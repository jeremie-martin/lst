# CLAUDE.md

## Active Work

The active editor is the GPUI implementation in `apps/lst-gpui`.
The repository root is a workspace-only manifest; it is not an application crate.

The previous iced implementation has been removed from the repository. Treat
the GPUI app and framework-neutral editor crate as the only active implementation.

## Quick Commands

- Build active editor: `cargo build --release -p lst-gpui`
- Run active editor: `cargo run -p lst-gpui -- path/to/file.rs`
- Test active workspace: `cargo test`
- Test editor behavior with Vim internals: `cargo test -p lst-editor --features internal-invariants`
- Format active workspace: `cargo fmt --all`

## Architecture

- `apps/lst-gpui`: GPUI application, rendering, input adapter, file dialogs, clipboard, and runtime effects.
- `crates/lst-editor`: framework-neutral editor model, document primitives, effects, snapshots, and Vim state machine.
- `apps/lst-gpui/src/ui`: app-private GPUI widgets for tabs, inputs, and shell UI.

Behavior should move toward `lst-editor` when it can be tested through direct
model APIs, effects, snapshots, or document-level contracts. GPUI should mostly
adapt desktop events to those contracts and render observable state.
