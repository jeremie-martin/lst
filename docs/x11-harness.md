# X11 Harness

In-process driver for end-to-end testing of the lst editor through real
X11 input. Spawn the editor against `DISPLAY`, drive it with synthesized
keyboard/mouse events, assert on autosaved file contents and clipboard
state. Lives in `crates/lst-x11-harness`; test fixture in
`apps/lst-gpui/tests/support/mod.rs`; concrete test suites in
`apps/lst-gpui/tests/real_x11_*.rs`.

This document is the canonical reference for the harness — what it's
good for today, where the foundations are solid, and what remains to be
hardened before it can be the project's load-bearing test
infrastructure.

---

## Canonical test shape

```rust
mod support;

use support::{EditorTestExt, ScratchpadSession, TestResult};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn descriptive_name_for_the_observed_behavior() -> TestResult {
    let mut session = ScratchpadSession::new("label-for-the-temp-dir")?;
    let (mut editor, path) = session.open("scratch")?;

    editor.send_keys("…input sequence…")?;
    editor.save_then_expect_file(&path, "…expected file content…")?;
    Ok(())
}
```

Five lines of body, plus the test attributes. That ratio is the
benchmark: any future scaffolding additions should preserve it.

To run:

```
cargo test -p lst-gpui --tests -- --ignored --test-threads=1 --nocapture
```

`--test-threads=1` is mandatory — every test grabs keyboard focus and
moves the global pointer through XTEST. Running them in parallel would
have them fighting over input.

Useful env knobs: `LST_X11_WINDOW_TIMEOUT_MS=N` (default 30000) tunes
the "how long to wait for the editor's window to become viewable"
budget; `LST_X11_KEEP_TEMP=1` preserves all scratchpad temp dirs so you
can inspect the buffer the editor was last writing to.

---

## Synchronization model (load-bearing)

Every effectful action is fire-and-forget at the X11 layer. Callers
synchronize on **paint events**:

- `Editor::send_keys` waits for the editor's damage to quiesce after
  *each* keystroke before sending the next one. That mirrors what a
  real keyboard guarantees: each keystroke produces a render before
  the next can land.
- `Editor::wait_quiet`, `wait_file_text`, `wait_file_stable`,
  `wait_for_exit` are the explicit synchronization primitives for
  larger units of work.

Critically, the harness **never throttles based on guessed delays**.
Inserting a fixed sleep between events would either be too short
(losing chord events on slow paint) or too long (breaking vim compound
commands like `gg`, `dd`, `ciw`, which have no wall-clock timeout). A
slow-paced test (`vim_compound_commands_survive_long_pauses_between_keystrokes`
in `real_x11_vim.rs`) guards this property — do not "fix" failures
there by reintroducing throttle constants.

The single retained internal sleep is `POINTER_SETTLE` (50ms) between
pointer motion and a click, because the X server processes motion
asynchronously and clicking before the motion has been delivered can
route the click to the previous pointer location.

---

## Status: what's solid

- **Principled synchronization.** Paint-sync per keystroke is the
  right primitive. Validated by the slow-paced compound-command test
  and by the multi-cursor `Ctrl+D Ctrl+D Ctrl+D` test.
- **Small, consistent API.** `ScratchpadSession::open` →
  `editor.send_keys(...)` → `editor.save_then_expect_file(...)` is the
  shape across every existing test.
- **Full printable ASCII typing** via level-aware XKB lookup
  (uppercase auto-shifts, digits and punctuation work without per-char
  extensions), plus `Tab` / `Enter` / `Escape` / `Backspace`, plus
  modifier chords (`<C-x>`, `<S-x>`, `<C-S-x>`, `<ctrl-shift-x>`).
- **11 tests across 4 files exercising different flavors:** literal
  type, vim modes (Normal/Insert/Visual-line), vim compound commands
  with arbitrary pauses, multi-cursor `Ctrl+D`, Ctrl+A/Z/Y,
  file-open, scratchpad-open with autosave, middle-click PRIMARY
  paste, clipboard-on-quit. Two consecutive 11/11 runs after the
  paint-sync rewrite.
- **Real bugs already caught by the harness:** `Editor::quit` was
  leaking the child process on timeout; the window-discovery default
  was too tight under focus-stealing window managers (lwm
  specifically).

---

## Known gaps / things to watch

These are **explicit limitations** of the harness as it stands today.
Anyone writing a new test should know about them; anyone hardening the
harness should treat this list as the work-list.

1. **Sample size is small.** 11 tests passing twice tells us the
   harness *can* be reliable; it does not yet tell us it *will be* at
   the 50–100 test scale. Reach the 30–50 test mark across the
   editor-behaviors checklist and watch for new flake patterns
   before declaring "rock solid."

2. **Failure-artifact preservation has a gap.** `Drop` on
   `ScratchpadSession` keeps temp dirs only when the thread is
   panicking *or* `LST_X11_KEEP_TEMP=1` is set. Tests that fail via
   `?` propagation — which is the common case for our `TestResult`
   pattern — silently delete their temp dir on drop. If a CI run
   fails you can't go inspect the buffer afterward. The fix is to
   default to "preserve unless the test calls `session.cleanup()`
   explicitly," and have the test do that on its successful path.

3. **No way to observe non-file state.** Every assertion today goes
   through the autosave file or the X clipboard. That covers most of
   the editor-behaviors checklist but not all of it: vim mode,
   find-panel state, cursor position, selection extents, line-number
   gutter content, status-bar string. As tests grow into those areas
   we'll need an introspection mechanism — likely an out-of-band
   sentinel file the editor writes when in test mode, or a richer
   trace channel similar to `LST_BENCH_TRACE_FILE`.

4. **No tests for modal panels yet.** Find/replace, goto-line, and
   recent-files all change keyboard focus. We have not yet sent a
   `<C-f>foo<enter>` sequence and asserted on the editor's resulting
   state. There may be focus-routing or panel-dismiss races we don't
   know about.

5. **Fixed-pixel mouse coordinates only.** `Editor::middle_click_at(160,
   170)` and friends require the test author to know absolute pixel
   coordinates inside the window. Tests like "double-click the word
   `foo` on line 3" need a text-to-pixel coordinate API we don't
   have. Pure-keyboard tests cover most of the checklist; this only
   matters for explicitly mouse-driven behavior.

6. **`send_keys` throws on unmapped characters.** Anything outside
   ASCII printable (e.g. `é`, `→`, smart quotes) will hit a runtime
   error from the keymap lookup. Documented; fine for English-only
   source code; will need attention if non-ASCII inputs ever land in
   tests.

7. **Window discovery is environment-sensitive.** lwm's focus-stealing
   prevention occasionally takes 10s+ to map a freshly-spawned
   window. Default timeout is 30s, configurable via
   `LST_X11_WINDOW_TIMEOUT_MS`. CI machines with different WMs may
   need their own tuning.

8. **No stress test for `wait_quiet` when no paint occurs.** If a
   keystroke genuinely produces no damage event (e.g., editor is in a
   weird state and ignoring input), `send_keys` waits the full
   `SEND_KEYS_TIMEOUT` (2s) and then errors. Acceptable for now —
   2s × N keystrokes is a reasonable failure budget — but worth
   revisiting if test latency becomes a complaint.

---

## Hardening roadmap

Two concrete rounds before this is the project's load-bearing test
foundation:

1. **Close the failure-preservation gap** (item 2 above). Small
   change, big debug-loop improvement.
2. **Drive the test count up to ~30–50 across the editor-behaviors
   checklist**, picking tests that exercise different feature surfaces:
   find-replace, indent/outdent, undo/redo branches, vim text objects,
   paragraph operations, selection extension, modal panels. Watch for
   new flake patterns at that scale; fix the underlying cause rather
   than papering over individual tests.

After both: the harness is the project's foundation.

---

## Implementation notes

- `crates/lst-x11-harness` is `publish = false` and lives in workspace
  `members` but **not** `default-members`, so default `cargo build` /
  `cargo test` cost is unchanged.
- The harness owns its own X11 connection (`Display::from_env`) and
  enforces "one editor per Display" via `&mut Display` on
  `spawn_editor`. The returned `Editor<'a>` borrows the Display
  immutably for its lifetime; trying to spawn a second concurrent
  editor through the same Display is a borrow-check error, not a
  runtime race.
- The bench example at `apps/lst-gpui/examples/bench_editor_x11.rs`
  has its own copy of the X11 primitives and is unchanged by the
  harness work. Migrating it onto the harness is a separate piece of
  work, deliberately deferred so the bench stays stable.
