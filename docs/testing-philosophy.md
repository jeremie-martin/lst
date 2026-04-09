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
- Display and GPU (iced's rendering pipeline, wgpu)
- Non-deterministic inputs (wall clock, random)

**Do not fake:**
- Internal state machines, coordination logic, event dispatch
- Pure logic (text manipulation, vim motions, find/replace matching)
- Anything that is "hard to set up" — if it is hard to set up, that is a design problem

A boundary fake should be a minimal trait implementation, not a general-purpose mock framework. A `NullClipboard` with empty methods is better than a dynamic mock that silently returns defaults for every call. The fake should be simple enough that you trust it without testing it.

```rust
pub trait Clipboard {
    fn copy(&self, text: &str);
    fn copy_primary(&self, text: &str);
    fn read_primary(&self) -> Option<String>;
}

pub struct NullClipboard;

impl Clipboard for NullClipboard {
    fn copy(&self, _: &str) {}
    fn copy_primary(&self, _: &str) {}
    fn read_primary(&self) -> Option<String> { None }
}
```

One fake per boundary. Shared across all tests. Maintained as test infrastructure, not duplicated per test file.


## The design constraint

Here is the key insight: **if a test requires excessive faking, indirection, or setup to exercise real code, the production code is wrong.**

This is not a test failure. It is a design failure. The production code has made itself untestable by:

- **Tight coupling to external systems.** If a function directly calls `wl-copy` in the middle of a text operation, every test must either fake the clipboard or skip that code path. Fix: accept the capability as a trait object.
- **Mixed concerns.** If one module manages state, handles input, dispatches events, and owns the view, testing any one concern requires setting up all of them. Fix: split into focused modules.
- **Untestable framework state.** If line operations mutate a GUI widget's internal state directly, and that widget has no public constructor, tests cannot create the state they need. Fix: extract the pure logic into functions that operate on plain data.
- **Hidden coupling.** If two components communicate through shared mutable state rather than explicit interfaces, tests must carefully orchestrate that state. Fix: make dependencies and data flow explicit.

The right response to "this is hard to test" is never "write a more clever test." It is "restructure the production code so the obvious test works."


## The test factory

A well-structured app can offer a single constructor that wires in null boundaries and produces a fully functional instance for testing. The test exercises the real state machine — real event handling, real state transitions, real text manipulation — with only the external world removed.

```rust
let mut app = App::test("foo bar foo");
app.update_inner(Message::FindOpen);
app.update_inner(Message::FindQueryChanged("foo".to_string()));
assert_eq!(app.find.matches.len(), 3);
```

This test exercises the real find logic, the real match computation, the real state update. Nothing is faked except the clipboard and filesystem, which find/replace never touches. The test is short because the code is well-structured, not because the test is clever.


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

- **iced's rendering pipeline, layout engine, wgpu** — these are framework internals. We trust them the same way we trust the standard library.
- **Real clipboard and filesystem in CI** — behind trait boundaries, exercised in production. The traits exist precisely so we can remove these from the test path.
- **Visual correctness** — no headless renderer available. Pixel-level assertions would be brittle even if they were possible.

The line is: test everything we own, trust everything we don't. If we find ourselves wanting to test framework behavior, that is a sign we are relying on undocumented behavior and should reconsider the design.


## The testability feedback loop

Testability is a leading indicator of code quality. When you notice:

| Symptom in tests | Root cause in production code |
|---|---|
| Many fakes needed to instantiate one struct | Struct has too many responsibilities |
| Tests break when internals change | Struct lacks a clean public interface |
| Same setup boilerplate in every test | Missing test factory or builder |
| Hard to assert on outcomes | Side effects are hidden or state is inaccessible |
| Test requires complex orchestration | Components are implicitly coupled through shared mutable state |
| "Works in tests, breaks in production" | Fakes diverged from real behavior — too many fakes |

Each of these symptoms points to a production code change, not a test change.


## Summary

The quality of a test suite is determined by the quality of the production code it tests. Design for testability means writing production code with clear boundaries, explicit dependencies, focused modules, and minimal indirection — code where the obvious test is also the correct test.

If the tests are bad, fix the code.
