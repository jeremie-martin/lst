# Testing Philosophy

Good tests and good production code are the same problem.

A test that is ugly, verbose, or full of mocks is not a test problem — it is a design problem. The test is a mirror. If the mirror shows something ugly, you don't fix the mirror.


## The core idea

Test through the real code path. Exercise as much production code as possible in every test. Mock only at the boundaries where the real world leaks in — hardware, network, filesystem, display, clock. Everything between those boundaries should run for real.

This is not a testing technique. It is a design constraint. It means the production code must be structured so that real code paths are exercisable without the real world.


## What makes a good test

A good test does three things:

1. **Sets up a scenario** using the same entry points a real caller would use.
2. **Exercises real production code** — not mocks, not stubs, not reimplementations of logic the test is supposed to verify.
3. **Asserts on observable outcomes** — outputs, state changes, published events, side effects. Not on call counts, not on argument lists, not on internal method invocations.

If a test passes but the feature is broken, the test is worthless. The most common cause: the test mocked away the code that would have caught the bug.


## Mocks are a smell

Every mock in a test is a piece of production code that is *not being tested*. Sometimes that tradeoff is necessary — you cannot plug in real hardware in CI. But every mock should be a conscious, reluctant decision, not a default strategy.

**Mock at boundaries:**
- External hardware (sensors, devices, displays)
- Network services (LSL streams, external APIs)
- The rendering subsystem (OpenGL, GPU)
- Non-deterministic inputs (wall clock, random)

**Do not mock:**
- Internal classes talking to each other
- State management, coordination logic, event dispatch
- Anything that is "hard to set up" — if it is hard to set up, that is a design problem, not a testing problem

When you need a fake at a boundary, prefer a purpose-built fake over `MagicMock`. A `FakeBackendClient` with real typed fields and predictable behavior is better than a `MagicMock(spec=BackendClient)` that silently returns `MagicMock()` for every attribute. The fake should be simple enough that you trust it without testing it.


## The design constraint

Here is the key insight: **if a test requires excessive mocking, indirection, or setup to exercise real code, the production code is wrong.**

This is not a test failure. It is a design failure. The production code has made itself untestable by:

- **Tight coupling to external systems.** If a class directly imports and calls `pylsl.StreamInlet()` in its constructor, every test must mock `pylsl`. Fix: accept the inlet (or a factory) as a parameter.
- **God classes.** If a 600-line class manages five concerns, testing any one of them requires setting up all five. Fix: split into focused classes.
- **Deep inheritance hierarchies.** If testing a leaf class requires understanding four parent classes, the test will be fragile and unclear. Fix: prefer composition over inheritance, configuration over subclassing.
- **Hidden state and implicit coupling.** If two components communicate through shared mutable state rather than explicit interfaces, tests must carefully orchestrate that state. Fix: make dependencies and data flow explicit.

The right response to "this is hard to test" is never "write a more clever test." It is "restructure the production code so the obvious test works."


## Black-box testing is the goal, not the technique

Black-box testing means: verify behavior through the public interface without knowledge of internal implementation. This is the ideal. But black-box testing of a badly designed system is worse than useless — it creates the illusion of coverage while testing nothing real.

Consider a system where Component A calls Component B through three layers of indirection: a protocol, a registry, and a dispatcher. A "black-box" test of A mocks the protocol boundary, exercises A's logic, and asserts on the mock's call arguments. This test is technically black-box (it does not peek inside A). But it tests almost nothing — the protocol, registry, and dispatcher are all mocked away. If any of them break, the test still passes.

The fix is not a better test. The fix is removing the indirection so that A calls B directly, and the test can exercise A, the call to B, and B itself — all for real.

**The quality of a black-box test is proportional to how much real production code runs between the test's setup and its assertions.** If the answer is "not much, because everything was mocked," the test has a coverage problem disguised as a design problem.


## The testability feedback loop

Testability is a leading indicator of code quality. When you notice:

| Symptom in tests | Root cause in production code |
|---|---|
| Many mocks needed to instantiate one class | Class has too many dependencies (god class) |
| Tests break when internals change | Tests coupled to implementation, but also: the class lacks a clean public interface |
| Same setup boilerplate in every test | Missing factory or builder in production code |
| Hard to assert on outcomes | Side effects are hidden or state is inaccessible |
| Test requires complex orchestration | Components are implicitly coupled through shared mutable state |
| "Works in tests, breaks in production" | Mocks diverged from real behavior — too many mocks |

Each of these symptoms points to a production code change, not a test change.


## Practical guidelines

**Start from the outside.** When writing a new test, ask: "What is the outermost entry point I can use?" For a backend service, that might be sending a real command over ZMQ and subscribing to real state events. For a GUI controller, it might be constructing the real object with faked external boundaries and calling `update()`. The farther out you start, the more real code you exercise.

**One fake per boundary.** A well-designed system has a small number of external boundaries. Each boundary gets one fake implementation (not one per test, not one per class — one per boundary). These fakes are shared across all tests and maintained as test infrastructure.

**Assert on what the user would observe.** Not internal method calls, not intermediate state. What changed in the output? What event was published? What file was written? What state is visible through the public API?

**If a test is long, the design is wrong.** A test that needs 40 lines of setup to reach the interesting assertion is telling you that the code under test is too coupled. A test should be: create the thing, do the action, check the result. If "create the thing" is the hard part, simplify the thing.

**Delete tests that test mocks.** If a test's primary assertion is `mock.assert_called_once_with(...)`, ask: what happens if I delete this test? If the answer is "nothing, because the real behavior is tested elsewhere," delete it. If the answer is "we lose coverage of that code path," the code path needs a real test, not a mock test.


## Tests are a code review of behavior

A test does not just verify that the code works — it makes the code's behavior visible. When you write a black-box test and the behavior you observe is surprising, confusing, or feels wrong, that is a signal. Do not write the assertion and move on.

If you find yourself thinking "this is technically what happens, but it seems weird," stop and challenge it. The test is showing you a design problem, a missing invariant, or a behavior that nobody intended. Examples:

- A filter reset that silently keeps the old value in some edge case. Is that intentional, or a bug hiding behind convention?
- A stream switch that leaves stale state visible for one frame before the reset kicks in. Race condition or acceptable tradeoff?
- A command that succeeds but produces no observable state change. Dead code path, or does the effect happen somewhere the test cannot see?

The purpose of testing behavior is not just to lock it in — it is to scrutinize it. A test that faithfully encodes weird behavior is preserving a bug. A test that flags weird behavior and leads to a production fix is doing its real job.

**When writing a test, treat every assertion as a claim about how the system *should* behave, not just how it *does* behave.** If you cannot confidently defend the assertion — if it feels like you are just documenting an accident — raise it. The test is the cheapest place to catch a bad design decision.


## Summary

The quality of a test suite is determined by the quality of the production code it tests. Design for testability does not mean adding test hooks, dependency injection frameworks, or abstract interfaces. It means writing production code with clear boundaries, explicit dependencies, focused classes, and minimal indirection — code where the obvious test is also the correct test.

If the tests are bad, fix the code.
