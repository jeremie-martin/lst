# Repository Guidelines

This is the canonical contributor guide for the `lst` editor. It applies to humans and agents alike. `CLAUDE.md` intentionally just points here.

## Active Work

The active editor is the GPUI implementation in `apps/lst-gpui`. The repository root is a workspace-only manifest, not an application crate. The previous iced implementation has been removed — treat the GPUI app and the framework-neutral editor crate as the only active implementation.

## Project Structure

- `crates/lst-editor`: framework-neutral editor model, document primitives, effects, snapshots, language behavior, and Vim state machine. Behavior should move here whenever it can be tested through model APIs, effects, snapshots, or document-level contracts.
- `apps/lst-gpui`: GPUI desktop app, rendering, input adaptation, runtime file/clipboard/display effects, and app-private UI widgets under `src/ui`. Should mostly adapt desktop events to `lst-editor` contracts and render observable state.
- `crates/lst-x11-harness`: in-process X11 driver for spawning the editor binary and synthesizing real keyboard/mouse input under `DISPLAY`. Used by smoke tests today; the `bench_editor_x11` example will migrate onto it. Outside `default-members` because it has no purpose without an X server.
- `apps/lst-gpui/examples/bench_editor_x11.rs`: real-display X11 benchmark runner.
- `crates/lst-editor/tests`: editor behavior integration tests.
- `apps/lst-gpui/src/tests.rs` and `apps/lst-gpui/tests`: app tests plus the real-display test suites (`real_x11_*.rs`) on top of `lst-x11-harness`. Shared fixture lives in `apps/lst-gpui/tests/support/mod.rs` (`ScratchpadSession`, `EditorTestExt::save_then_expect_file`, etc.) — new real-display tests should reuse it. See `docs/x11-harness.md` for the canonical test shape, the synchronization model, and the current list of harness gaps to be aware of when writing new tests.
- `docs`: testing philosophy, behavior checklist, roadmap, performance workflow.

## Build, Test, and Development Commands

- `cargo build --release -p lst-gpui` — build the active editor binary.
- `cargo run -p lst-gpui -- path/to/file.rs` — run the editor locally.
- `cargo test` — default workspace refactor gate (behavioral contracts only).
- `cargo test --all-features` — full feature-enabled suite.
- `cargo test -p lst-editor --features internal-invariants` — deeper Vim/editor invariant checks.
- `cargo clippy --all-targets --all-features` — lint all targets.
- `cargo fmt --all` — format the workspace.
- `cargo build --release -p lst-gpui --bin lst --example bench_editor_x11` — build the benchmark runner with the release app.
- `cargo test -p lst-gpui --tests -- --ignored --test-threads=1 --nocapture` — run all real-display tests (requires `DISPLAY`, an X11 server, and `xclip` on `PATH`). `--test-threads=1` is required because every test grabs keyboard focus and moves the global pointer through XTEST; running them in parallel would have them fighting over input. The `lst-x11-harness` crate waits up to 30s for each editor window to be mapped (override with `LST_X11_WINDOW_TIMEOUT_MS=N`); set `LST_X11_KEEP_TEMP=1` to preserve scratchpad contents on disk for debugging.

Run `cargo test --all-features` and `cargo clippy --all-targets --all-features` before submitting behavior or architecture changes.

## Correctness By Construction

Prefer designs where invalid internal states are unrepresentable. Do not rely on scattered defensive checks to preserve core editor invariants. The goal is a smaller, clearer core, not more layers around the same behavior.

- Model domain concepts directly: use types such as `TabSet`, `TabOrigin`, `Selection`, and explicit state objects instead of loose primitive data plus comments about how it should be used.
- Parse and validate at boundaries, then pass stronger internal representations through the core. Filesystem, clipboard, display, process, and user input boundaries may need defensive handling; editor-domain code should not.
- Keep invariants owned by one module. Other code should not need to remember follow-up calls like "after changing X, also refresh Y" unless that sequence is encoded in a single API.
- Make mutation paths narrow and intention-revealing. Prefer explicit `EditorModel` APIs and state transitions over exposing containers for direct mutation.
- Treat repeated null checks, fallback branches, compatibility adapters, and broad "just in case" code inside the core as design smells. First look for a stronger representation or ownership boundary.
- Stronger representation should earn its keep. Add a type, wrapper, helper, or abstraction only when it removes an invalid state, eliminates repeated validation, narrows mutation paths, or makes behavior easier to test through public contracts.
- Treat line count as a real design pressure. New code should pay for itself by removing duplicated logic, clarifying ownership, or encoding an invariant. Do not add forwarding facades, generic coordinators, or broad adapter layers that merely move complexity around.
- Do not remove necessary boundary error handling. The goal is not optimistic code; it is a core model whose invariants are enforced by construction, with defensive code limited to the real world outside the model.

## Testing Philosophy

Full writeup in `docs/testing-philosophy.md`. The short version:

- **Test through the real code path.** Exercise as much production code as possible in every test. Fake only at boundaries where the real world leaks in — clipboard, filesystem, display, clock. Everything between those boundaries should run for real.
- **Assert on observable outcomes** (outputs, state changes, text content), not on call counts or internal method invocations.
- **If a test requires excessive faking or setup, the production code is wrong.** Restructure the code so the obvious test works. "Hard to test" is a design signal, not a reason to write a cleverer test.
- **One minimal fake per boundary**, shared across tests. Prefer a `NullX` trait implementation over a dynamic mock framework.
- **The default `cargo test` suite is a blind refactor gate** biased toward behavioral contracts. Implementation-sensitive checks live behind `--features internal-invariants` or explicit package selection so they don't block healthy internal rewrites.

Name tests after observable behavior, e.g. `save_preserves_explicit_language_override` or `search_matches_for_row_slices_to_visible_char_range`. Any logic change must include tests.

## Coding Style & Naming

Standard Rust formatting via `cargo fmt --all`. Prefer small, behavior-owned modules and explicit domain types over loose primitive data. Use `snake_case` for functions and modules, `PascalCase` for types, and concise names that match editor concepts (`TabSet`, `TabOrigin`, `Selection`, `ViewportPaintInput`).

## Commits & Pull Requests

Recent commits use short imperative subjects, e.g. `Fix review regressions` and `Remove legacy compatibility paths`. Keep commits focused and avoid bundling unrelated refactors.

Pull requests should describe the behavior or architecture change, list verification commands, and call out performance-sensitive paths. Include screenshots only for visible UI changes; include benchmark output when changing rendering, search highlighting, paste, typing, or startup paths.
