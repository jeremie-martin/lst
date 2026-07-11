# Repository Guidelines

This is the canonical contributor guide for the `lst` editor. It applies to humans and agents alike. `CLAUDE.md` intentionally just points here.

## Active Work

The active editor is the GPUI implementation in `apps/lst-gpui`. The repository root is a workspace-only manifest, not an application crate. The previous iced implementation has been removed — treat the GPUI app and the framework-neutral editor crate as the only active implementation.

## Project Structure

- `crates/lst-editor`: framework-neutral editor model, document primitives, effects, snapshots, language detection, bookmarks, and Vim state machine. Keep it lean: behavior belongs here when it is editor-domain logic, but accepted product behavior should be specified through X11 whenever it can be driven through the real app.
- `apps/lst-gpui`: GPUI desktop app, rendering, input adaptation, runtime file/clipboard/display effects, tree-sitter syntax highlighting under `src/syntax`, the DeepSeek text-cleanup client under `src/llm.rs`, and app-private UI widgets under `src/ui`. Should mostly adapt desktop events to `lst-editor` contracts and render observable state.
- `crates/lst-x11-harness`: in-process X11 driver for spawning the editor binary and synthesizing real keyboard/mouse input under `DISPLAY`. This is the canonical behavior-spec harness for accepted editor behavior. Outside `default-members` because it has no purpose without an X server.
- `apps/lst-gpui/examples/bench_editor_x11.rs`: real-display X11 benchmark runner.
- Inline tests in `crates/lst-editor/src`: small pure-algorithm and invariant checks only, and only when X11 cannot naturally cover the same user-visible path.
- `crates/lst-editor/tests`: fast model-level black-box suites that drive the public `EditorModel`/Vim input surface and assert observable outcomes — `vim_behavior.rs` (the exhaustive Vim parity lane), `vim_oracle.rs` (a generated Neovim oracle corpus from `tests/fixtures/vim_oracle.json`, regenerated with `scripts/generate_vim_oracle_fixtures.py`), and `editor_model_workflows.rs`. These complement X11 where exhaustive command coverage through a real display would be too slow; they are not a place to re-spec behavior that X11 already gates. See `docs/vim-feature-inventory.md`.
- `apps/lst-gpui/tests`: app real-display suites (`real_x11_*.rs`) on top of `lst-x11-harness`. Shared fixture lives in `apps/lst-gpui/tests/support/mod.rs` (`ScratchpadSession`, `EditorTestExt::save_then_expect_file`, etc.) — new accepted editor behavior should normally be added here. See `docs/x11-harness.md` for the canonical test shape, the synchronization model, and the current list of harness gaps to be aware of when writing new tests.
- `docs`: testing philosophy, behavior checklist, roadmap, performance workflow.

## Build, Test, and Development Commands

- `cargo build --release -p lst-gpui` — build the active editor binary.
- `cargo run -p lst-gpui -- path/to/file.rs` — run the editor locally.
- `cargo test` — fast compile/domain sanity check.
- `cargo test --all-features` — full non-X11 sanity suite.
- `cargo test -p lst-editor --features internal-invariants` — optional deep private invariant checks.
- `cargo clippy --all-targets --all-features` — lint all targets.
- `cargo fmt --all` — format the workspace.
- `cargo build --release -p lst-gpui --bin lst --example bench_editor_x11` — build the benchmark runner with the release app.
- `./scripts/run_x11_nested.py` — run the blocking X11 behavior lane in an off-screen Xephyr server. It drives the production app with real XTEST keyboard/mouse input while keeping the host desktop usable. It requires a host X11 display, `Xephyr`, `lwm`, `wmctrl`, `xclip`, and Python Xlib. Pass `--probe` for two focused lifecycle checks or `-- <command>` to run a custom command on the nested display. The runner excludes `real_x11_visual`, whose pixel baselines remain real-display-only.
- `DISPLAY=:0 cargo nextest run --profile x11 -p lst-gpui --tests --run-ignored only` — run the suite directly on the physical X11 display when explicitly validating visual output or investigating a nested-only discrepancy. Use `./scripts/qualify_x11_nested.py` to prove real/nested behavioral parity with the full baseline and six disposable source mutants. Real-display profiles run serially because every test grabs keyboard focus and moves the global pointer through XTEST. The `lst-x11-harness` crate waits up to 30s for each editor window to be mapped (override with `LST_X11_WINDOW_TIMEOUT_MS=N`); set `LST_X11_KEEP_TEMP=1` to preserve scratchpad contents on disk for debugging.

Run `cargo test --all-features`, `cargo clippy --all-targets --all-features`, and `./scripts/run_x11_nested.py` before submitting behavior or architecture changes.

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
- **X11 is the behavior gate.** `cargo test` is useful fast feedback, but accepted editor behavior is specified through the real app under `apps/lst-gpui/tests/real_x11_*.rs`. A pure invariant is not a free pass; when behavior is user-visible and X11-drivable, rely on X11 and prune the source-side test. Keep source-side tests only when they are boundary/tooling contracts or when there is no practical display-level route yet.

Name tests after observable behavior, e.g. `save_preserves_explicit_language_override` or `search_matches_for_row_slices_to_visible_char_range`. Any user-visible logic change should include or update X11 coverage unless the behavior cannot be driven through the app.

## Coding Style & Naming

Standard Rust formatting via `cargo fmt --all`. Prefer small, behavior-owned modules and explicit domain types over loose primitive data. Use `snake_case` for functions and modules, `PascalCase` for types, and concise names that match editor concepts (`TabSet`, `TabOrigin`, `Selection`, `ViewportPaintInput`).

## Commits & Pull Requests

Recent commits use short imperative subjects, e.g. `Fix review regressions` and `Remove legacy compatibility paths`. Keep commits focused and avoid bundling unrelated refactors.

Pull requests should describe the behavior or architecture change, list verification commands, and call out performance-sensitive paths. Include screenshots only for visible UI changes; include benchmark output when changing rendering, search highlighting, paste, typing, or startup paths.
