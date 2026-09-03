# Repository Guidelines

This is the contributor guide for humans and agents. The active product is the
GPUI application; the repository root is only a Cargo workspace.

## Ownership

- `crates/lst-editor` owns framework-neutral editor behavior and state:
  documents, tabs, selections, transactions, history, find, language behavior,
  viewport intent, effects, and Vim.
- `apps/lst-gpui` adapts desktop input, renders model state, and owns GPUI,
  filesystem, clipboard, dialogs, settings, recent files, tree-sitter, and the
  DeepSeek client.
- `crates/lst-x11-harness` drives the production app with real X11 input. It is
  a workspace member but is excluded from `default-members`.

Put editor-domain behavior in `lst-editor`. Keep operating-system and framework
work at the app boundary. Read [Architecture](docs/architecture.md) before
changing ownership or core state representations.

## Design rules

- Make invalid editor states unrepresentable. Domain types such as `TabSet`,
  `TabOrigin`, `SelectionSet`, and `TextChangeSet` own their invariants.
- Parse and validate at external boundaries, then pass validated values inward.
- Keep mutation behind intention-revealing `EditorModel` operations. Text edits
  flow through `EditRequest` / `TextChangeSet` and one history boundary.
- Give each invariant and derived value one owner. Avoid coordination that
  requires callers to remember a second update.
- Add an abstraction only when it removes an invalid state, duplicated logic,
  or production complexity. Keep the core small.
- Preserve defensive handling for filesystem, clipboard, display, process,
  network, and user-input failures.

## Tests

Accepted user-visible behavior belongs in `apps/lst-gpui/tests/real_x11_*.rs`
when the real app can drive it. Assert observable text, clipboard, UI state, or
geometry—not internal calls or transaction shape. Keep source-side tests for
small algorithms, representation invariants, boundary adapters, and incremental
cache equivalence. The public model suites under `crates/lst-editor/tests` are
the deliberate fast lane for exhaustive Vim parity and a few model workflows.

Read [Testing](docs/testing.md) before adding, moving, or debugging tests. Read
[Vim mode](docs/vim.md) before changing the Vim surface or oracle fixture.

## Commands

During development:

```sh
cargo test
cargo test --all-features
cargo test -p lst-editor --features internal-invariants
cargo fmt --all
cargo clippy --all-targets --all-features
```

Before submitting behavior or architecture changes, run:

```sh
cargo test --all-features
cargo clippy --all-targets --all-features
./scripts/run_x11_nested.py
```

The nested X11 lane requires a host X11 session and its documented system
tools. Run the physical-display profile only for visual baselines or a
nested-only discrepancy:

```sh
DISPLAY=:0 cargo nextest run --profile x11 -p lst-gpui --tests --run-ignored only
```

Read [Performance](docs/performance.md) before changing or evaluating rendering,
search highlighting, paste, typing, scrolling, or startup performance.

## Style and changes

Use standard Rust naming and `cargo fmt --all`. Prefer small modules named for
editor concepts. Name tests after observable behavior.

Keep commits focused with short imperative subjects. Pull requests should state
the behavior or architecture change and list verification commands. Include
screenshots for visible UI changes and benchmark output for performance-sensitive
paths.
