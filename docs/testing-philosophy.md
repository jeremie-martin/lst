# Testing Philosophy

Good tests and good production code are the same problem.

A test that is ugly, verbose, or full of fakes is not a test problem — it is a design problem. The test is a mirror. If the mirror shows something ugly, you don't fix the mirror.


## The core idea

Test through the real code path. Exercise as much production code as possible in every test. Fake only at the boundaries where the real world leaks in — clipboard, filesystem, display, clock. Everything between those boundaries should run for real.

This is not a testing technique. It is a design constraint. It means the production code must be structured so that real code paths are exercisable without the real world.

The goal is black-box testing — verify behavior through the public interface without knowledge of internal implementation. But the quality of a black-box test is proportional to how much real production code runs between the setup and the assertion. If the answer is "not much, because everything was faked," the test has a coverage problem disguised as a design problem.


## What makes a good test

A good test does three things:

1. **Sets up a scenario** using the same entry points a real caller would use.
2. **Exercises real production code** — not fakes, not stubs, not reimplementations of logic the test is supposed to verify.
3. **Asserts on observable outcomes** — outputs, state changes, text content. Not on call counts, not on argument lists, not on internal method invocations.

If a test passes but the feature is broken, the test is worthless. The most common cause: the test faked away the code that would have caught the bug.


## Fakes at boundaries

Every fake in a test is a piece of production code that is *not being tested*. Sometimes that tradeoff is necessary — you cannot spawn a real Wayland compositor in CI. But every fake should be a conscious, reluctant decision, not a default strategy.

**Fake at boundaries:**
- System clipboard (subprocess calls to wl-copy/xclip)
- Filesystem (real I/O, path resolution)
- Display and GPU (GPUI rendering, compositor, graphics driver)
- Non-deterministic inputs (wall clock, random)

**Do not fake:**
- Internal state machines, coordination logic, event dispatch
- Pure logic (text manipulation, vim motions, find/replace matching)
- Anything that is "hard to set up" — if it is hard to set up, that is a design problem

A boundary fake should be a minimal trait implementation, not a general-purpose mock framework. The trait carries only the operations the editor actually invokes at that boundary, and the fake is small enough to trust on inspection. The current exit-clipboard path is intentionally not faked in the GPUI app tests anymore: shutdown clipboard persistence is product behavior, so the real X11 lane owns it through `real_x11_smoke.rs`. Keep that standard for other boundaries too. Add a fake only when the real boundary cannot be driven by the X11 harness or a cheaper public model contract.


## The design constraint

Here is the key insight: **if a test requires excessive faking, indirection, or setup to exercise real code, the production code is wrong.**

This is not a test failure. It is a design failure. The production code has made itself untestable by:

- **Tight coupling to external systems.** If a function directly calls `wl-copy` in the middle of a text operation, every test must either fake the clipboard or skip that code path. Fix: accept the capability as a trait object.
- **Mixed concerns.** If one module manages state, handles input, dispatches events, and owns the view, testing any one concern requires setting up all of them. Fix: split into focused modules.
- **Untestable framework state.** If line operations mutate a GUI widget's internal state directly, and that widget has no public constructor, tests cannot create the state they need. Fix: extract the pure logic into functions that operate on plain data.
- **Hidden coupling.** If two components communicate through shared mutable state rather than explicit interfaces, tests must carefully orchestrate that state. Fix: make dependencies and data flow explicit.

The right response to "this is hard to test" is never "write a more clever test." It is "restructure the production code so the obvious test works."


## Lean non-X11 contracts

A non-X11 test should have a narrow public contract owned by the module under
test. It should not instantiate the GPUI app, inspect snapshots, or poke private
state to approximate a user workflow. That kind of test is usually a stale X11
test trying to survive in source form.

Good remaining examples are parser contracts, filesystem result shapes,
UTF-16/character boundary conversion, syntax catalog registration, trace JSONL
reading, and small representation invariants. These tests stay useful because
they protect a boundary or representation directly, not because they are cheaper
duplicates of product behavior.

If setup starts to resemble a product workflow, move the behavior to X11 and
delete the source-side test. If the behavior cannot be driven through the real
app yet, put it on the watch list rather than treating the unit test as the
preferred long-term home.


## Tests as bug detectors

A test does not just verify that the code works — it makes the code's behavior visible. When the behavior you observe is surprising, that is a signal. Do not write the assertion and move on.

When 241 vim tests were written against the real motion and operator logic, four bugs surfaced immediately:

- Backward inclusive motions (dF/dT) were including the cursor character
- Forward motions at boundaries (dl/de/d$ on last char) were silently doing nothing instead of deleting
- `dw` on the last word of a file was leaving the final character
- `cw` on whitespace was incorrectly remapping to `ce`

None of these were hypothetical. They were real bugs in production code, found because the tests exercised the real code path — not a mock, not a stub, not a reimplementation. The tests were the mirror. The bugs were in the code.

**When writing a test, treat every assertion as a claim about how the system *should* behave, not just how it *does* behave.** If you cannot confidently defend the assertion — if it feels like you are just documenting an accident — raise it.


## What we don't test

Not testing something is a valid choice when it is a principled boundary, not a gap. We don't test:

- **GPUI's rendering pipeline, layout engine, and graphics backend** — these are framework internals. We trust them the same way we trust the standard library.
- **Compositor, clipboard-owner, and filesystem tooling internals** — drive them through X11 when they are part of editor behavior; otherwise trust the platform/tool and keep the editor boundary narrow.
- **Visual correctness** — no headless renderer available. Pixel-level assertions would be brittle even if they were possible.

The line is: test everything we own, trust everything we don't. If we find ourselves wanting to test framework behavior, that is a sign we are relying on undocumented behavior and should reconsider the design.


## Behavior gate

The canonical behavior gate is the real-display X11 suite:

```sh
DISPLAY=:0 cargo nextest run --profile x11 -p lst-gpui --tests --run-ignored only
```

This is the closest thing the repository has to a true black-box refactor gate:
it launches the production GPUI app, sends real keyboard and mouse input, and
asserts on observable user-facing state. When a behavior can be driven through
that path, X11 is the preferred specification.

`cargo test` is still useful, but it is no longer the primary behavior contract.
Treat it as fast compile/domain feedback. It should stay lean enough to run
often, and it should not grow into a second implementation-sensitive behavior
suite beside X11.

"Pure invariant" is an allowed exception, not the preferred shape. If an
assertion describes accepted editor behavior that a user can trigger in the real
app, specify it through X11 even when a source-side unit test would be shorter.
Keep source tests for representation invariants, parser/boundary contracts, and
small algorithms that X11 cannot naturally isolate without brittle test-only
plumbing.

### What lives where

- **X11 tests (`apps/lst-gpui/tests/real_x11_*.rs`)** — accepted product behavior: editor commands, mouse/keyboard input, Vim flows, clipboard-visible results, state trace, autosave/save workflows, cursor/selection geometry, and multi-cursor behavior.
- **Editor core unit tests (`#[cfg(test)] mod tests` in `crates/lst-editor/src`)** — rare pure-algorithm or invariant checks only: Unicode boundary handling, transaction validation, wrapping calculations, and small state containers. Do not rebuild `crates/lst-editor/tests` as a public-model behavior suite when X11 can cover the same user-visible path.
- **App-private unit tests** — boundary, parser, syntax, harness, and widget geometry contracts that are not accepted product behavior by themselves. Keep these lean; user-visible editing behavior belongs in X11.
- **Optional invariant suites** — private coordination checks behind explicit package/feature selections when they protect important internals without pretending to be product behavior.

Do not expose private functions just to preserve old unit tests. If a test mostly
documents implementation mechanics, delete it or reduce it to the smallest
invariant that still earns its keep. If it protects user-visible behavior, cover
that behavior through X11 whenever possible.

Before adding or keeping a non-X11 test, ask two questions:

1. Can a user drive this behavior through the real app today?
2. Would deleting this source test leave an internal representation or boundary
   contract meaningfully less protected?

If the answer to the first question is yes and the second is no, write or keep
the X11 test and prune the source test.

### Test-only escape hatches in production code

A few `#[cfg(test)]` items remain in production code. Each is justified or it should be removed:

- `process::exit(0)` vs `cx.defer(|app| app.quit())` in `finish_quit` — unavoidable platform difference. Tests cannot terminate the host process. The only `#[cfg(test)]` left in `finish_quit` is the exit step itself.


## The testability feedback loop

Testability is a leading indicator of code quality. When you notice:

| Symptom in tests | Root cause in production code |
|---|---|
| Many fakes needed to instantiate one struct | Struct has too many responsibilities |
| Tests break when internals change | Struct lacks a clean public interface |
| Same setup boilerplate in every test | Missing shared harness helper or narrow constructor |
| Hard to assert on outcomes | Side effects are hidden or state is inaccessible |
| Test requires complex orchestration | Components are implicitly coupled through shared mutable state |
| "Works in tests, breaks in production" | Fakes diverged from real behavior — too many fakes |

Each of these symptoms points to a production code change, not a test change.


## Summary

The quality of a test suite is determined by the quality of the production code it tests. Design for testability means writing production code with clear boundaries, explicit dependencies, focused modules, and minimal indirection — code where the obvious test is also the correct test.

If the tests are bad, fix the code.
