# X11 Harness

In-process driver for end-to-end testing of `lst` through real X11 input.
Tests spawn the editor against `DISPLAY`, drive it with synthesized keyboard
and mouse events, and assert on user-visible outcomes: saved file contents,
clipboard contents, cursor/selection state, panels, status text, and viewport
geometry.

The harness lives in `crates/lst-x11-harness`; the shared test fixture lives in
`apps/lst-gpui/tests/support/mod.rs`; concrete suites live in
`apps/lst-gpui/tests/real_x11_*.rs`.

This document is the canonical reference for writing and maintaining those
tests.

---

## Black-Box Rule

Real-display tests are black-box behavior tests. They may use the state trace,
but only as a test-only observation channel for user-facing state.

Allowed behavior assertions:

- saved file contents
- clipboard and PRIMARY contents
- cursor heads and selection ranges
- visible Vim mode, pending input, status text, and dirty state
- visible find/goto/recent-panel state and focused input
- viewport rows and text-coordinate geometry needed to click or drag text

Avoid behavior assertions on implementation mechanics:

- model IDs
- revision counters
- transaction shape
- cache state
- normalization internals
- which production function handled an input

State-trace schema details may be tested in harness/state-trace self-tests, but
product behavior tests should go through behavior-named helpers such as
`expect_cursor_heads` and `expect_find_state` whenever practical.

---

## Running Tests

Use the nextest profiles for real-display work. They are the canonical local
and dedicated-machine entry points:

```sh
cargo nextest run --profile x11 -p lst-gpui --tests --run-ignored only
cargo nextest run --profile x11-stress -p lst-gpui --tests --run-ignored only --stress-count 3
cargo nextest run --profile x11-tdd -p lst-gpui --tests --run-ignored only
```

The `x11` profile is the blocking implemented-behavior lane. `x11-stress` runs
that same set repeatedly for flake detection. `x11-tdd` runs accepted
ahead-of-implementation specs as a separate report lane; failures there are
expected until the implementation catches up. Broad multi-cursor specs that are
intentionally ahead of implementation live in
`apps/lst-gpui/tests/real_x11_multi_cursor_spec.rs`, which is included only in
the `x11-tdd` profile.

The profiles run serially. Every test grabs keyboard focus and moves the global
pointer through XTEST. The harness also takes a cross-process display lock, so
accidental parallel runs serialize, but serial execution is clearer and avoids
wasted workers.

Useful environment variables:

- `LST_X11_WINDOW_TIMEOUT_MS=N` sets the window-discovery timeout in
  milliseconds. The default is 30000.
- `LST_X11_KEEP_TEMP=1` preserves scratchpad temp directories even on success.
- `LST_GPUI_BIN=/path/to/lst` runs a specific editor binary.

All three profiles disable retries, continue after failures, and write JUnit
output under `target/nextest/<profile>/junit.xml`. Prefer `--stress-count` over
retries for flake discovery: repeated successes and failures are the signal we
want, while retrying only failures can hide nondeterminism.

---

## Canonical Test Shapes

### Text Outcome

Use saved file contents when the behavior is a text transformation.

```rust
mod support;

use support::{EditorTestExt, TestResult};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn descriptive_text_behavior() -> TestResult {
    support::run_x11_test("label", |session| {
        let (mut editor, path) = session.open("scratch")?;

        editor.keys("input<enter>sequence")?;
        editor.save_then_expect_file(&path, "expected\ntext")?;
        Ok(())
    })
}
```

### State Outcome

Use the trace for user-visible state that cannot be proven cleanly through a
file or clipboard.

```rust
mod support;

use support::{EditorTestExt, TestResult};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn descriptive_cursor_behavior() -> TestResult {
    support::run_x11_test("label", |session| {
        let path = session.seed_file("file.txt", "alpha\nbeta\ngamma")?;
        let mut editor = session.open_file("file", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<S-A-down><S-A-down>")?;
        editor.expect_cursor_heads(&[(0, 0), (1, 0), (2, 0)])?;
        Ok(())
    })
}
```

### Clipboard Outcome

Use `lst_x11_harness::clipboard` helpers for CLIPBOARD and PRIMARY behavior.

### Mouse Outcome

Prefer text-coordinate helpers over fixed pixels:

- `click_at_text(line, col)`
- `shift_click_at_text(line, col)`
- `alt_click_at_text(line, col)`
- `double_click_at_text(line, col)`
- `triple_click_at_text(line, col)`
- `quad_click_at_text(line, col)`
- `middle_click_at_text(line, col)`
- `drag_text((line, col), (line, col), mods)`

These resolve coordinates from the latest state trace and are robust against
font, gutter, and padding changes. `ScratchpadSession::open` and `open_file`
wait for the first painted viewport snapshot, and the text-coordinate helpers
wait for the requested target row/column to be present before synthesizing the
mouse gesture.

### Explicit No-Op

For product behavior, assert that visible state remains unchanged. Do not assert
that no repaint occurred unless the test is specifically about the harness or
rendering contract. `Editor::send_keys_expect_quiet(...)` is a low-level helper
for that narrow case because `Editor::send_keys` expects each key to paint.

---

## Input Model

`editor.keys(...)` accepts Vim-style notation:

- literal printable ASCII characters
- special keys: `<enter>`, `<esc>`, `<tab>`, `<space>`, `<bs>`, `<delete>`,
  `<f2>`, `<home>`, `<end>`, `<left>`, `<right>`, `<up>`, `<down>`, `<pageup>`,
  `<pagedown>`, `<lt>`
- modifier chords: `<C-x>`, `<A-x>`, `<S-x>`, `<C-A-S-x>`, plus verbose
  `<ctrl-x>`, `<alt-x>`, `<shift-x>`
- held-modifier spans: `<C-{k d}>`, used for gestures where the real user keeps
  a modifier held across multiple key taps

Names are case-insensitive. Uppercase literal characters are typed through
Shift automatically. Non-ASCII printable input is not supported by the key
lookup today.

---

## Synchronization Model

Every effectful action is fire-and-forget at the X11 layer. Callers synchronize
on paint events and observable outcomes.

- `ScratchpadSession::open` and `open_file` return only after the editor window
  is focused and a painted text viewport snapshot is available.
- `Editor::send_keys` waits for at least one matching DAMAGE event, then waits
  for that damage stream to quiesce after each key before sending the next key.
- `Editor::wait_quiet`, `wait_file_text`, `wait_file_stable`, and
  `wait_for_exit` are explicit synchronization primitives for larger units of
  work.
- `Editor::wait_state(label, timeout, predicate)` waits on user-visible state
  from the trace.
- `Editor::read_state` returns the latest observed state snapshot, including a
  cached snapshot when no newer record has landed since the previous read.
- `Editor::drain_state_records` is for trace-stream tests that intentionally
  inspect record order or multiple records from one action span.

The harness must not add guessed sleeps between keystrokes. Fixed input delays
either hide slow-frame races or break compound commands that have no wall-clock
timeout, such as Vim `gg`, `dd`, and `ciw`. The slow-paced Vim test exists to
guard this.

The one intentional pointer delay is `POINTER_SETTLE` between pointer motion and
click/release, because the X server processes pointer motion asynchronously.

The state trace is emitted from the paint path after viewport geometry is
available. Product tests should treat each record as an observable UI snapshot,
not as a model transaction log.

---

## Artifacts And Cleanup

Use `support::run_x11_test`. It owns one `ScratchpadSession`, one X11 display
connection, and one temp tree per test.

On success:

- temp directories are removed unless `LST_X11_KEEP_TEMP=1` is set

On failure:

- scratch files are preserved
- editor stdout/stderr are captured under `artifacts/`
- helper failures try to capture an `.xwd` window image
- errors include the tail of captured stderr when available

Normal editor exits are asserted with `quit_default` or
`wait_for_successful_exit`; non-zero exit status is a test failure.

---

## Current Coverage

The real-display suite currently has broad coverage across:

- scratchpad open/close/save/quit workflows
- file edit/save/reopen workflows
- CLIPBOARD and PRIMARY paste/copy behavior
- keyboard modifiers and undo/redo
- Vim mode transitions and compound commands
- find and goto panel state
- mouse click, double-click, triple-click, quad-click, drag selection, middle-click
  paste, and TDD specs for shift-click / Alt-click gaps
- cursor movement and subword motion
- multi-cursor creation, text input, deletion, paste distribution, copy/cut
  collection, Escape collapse, smart Enter, per-cursor motion/selection, line
  operation coalescing, find/occurrence gestures, column drag, and TDD specs for
  remaining gaps
- chord-hold event trains for held-modifier gestures

This is the project's load-bearing end-to-end test path. When adding a new
behavior test, prefer extending this suite unless the behavior is better covered
through the framework-neutral model tests.

---

## Known Gaps / Things To Watch

1. **The raw full ignored suite can fail by design.** Some tests are executable
   specs for behavior not implemented yet. Keep their comments clear enough that
   failures are interpretable. The nextest `x11` profile excludes those current
   TDD specs for a blocking lane; `x11-tdd` runs them as a separate report lane.

2. **Trace discipline matters.** The trace is powerful enough to become an
   implementation inspection tool by accident. Keep behavior tests focused on
   user-visible state and move schema/mechanics assertions into trace self-tests.

3. **Modal-panel coverage is still thinner than core editing coverage.** Find
   has state coverage and goto has workflow coverage, but replace, recent files,
   and other focus-changing panels need more real-display scenarios.

4. **Visual pixel correctness is not covered.** The trace can expose viewport
   geometry and text-coordinate rows; it is not a general screenshot oracle for
   colors, antialiasing, or exact painted pixels.

5. **Non-ASCII input is unsupported in `send_keys`.** Tests that need input such
   as `é`, arrows, or smart quotes need a deliberate harness extension.

6. **Window discovery is environment-sensitive.** Some window managers take a
   long time to map a new editor window. Tune `LST_X11_WINDOW_TIMEOUT_MS` for
   slow CI or local environments.

7. **Text-coordinate mouse helpers require visible rows.** If the target line is
   not in the painted viewport, scroll or move there first.

---

## Hardening Roadmap

- Keep growing checklist coverage with real-display tests, especially modal
  panels, find/replace workflows, multi-cursor movement/editing, column
  selection, and viewport behavior.
- Add trace fields only when they represent user-visible state that cannot be
  asserted cleanly through files, clipboard, or existing trace fields.
- Keep common patterns in `apps/lst-gpui/tests/support/mod.rs` so behavior tests
  remain short and uniform.
- Run the `x11-stress` nextest profile repeatedly on the dedicated X11 machine
  before trusting broad behavior changes.
- Treat flakes as harness or synchronization bugs until proven otherwise. Do not
  paper over them with arbitrary sleeps.

---

## Implementation Notes

- `crates/lst-x11-harness` is `publish = false` and is outside default workspace
  members, so default build/test cost stays low.
- `Display::from_env` resolves the X session, verifies required X extensions and
  `xclip`, pins keyboard layout for deterministic key lookup, and holds a
  cross-process session lock.
- `Display::spawn_editor` takes `&mut Display`; the returned `Editor` borrows
  the display for its lifetime, so one session cannot accidentally drive two
  editors concurrently.
- The benchmark example still has separate X11-driving code. Migrating it onto
  the harness is useful but separate from behavior-test hardening.
