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

use support::{EditorTestExt, TestResult};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn descriptive_name_for_the_observed_behavior() -> TestResult {
    support::run_x11_test("label-for-the-temp-dir", |session| {
        let (mut editor, path) = session.open("scratch")?;

        editor.keys("…input sequence…")?;
        editor.save_then_expect_file(&path, "…expected file content…")?;
        Ok(())
    })
}
```

That shape is the benchmark: one wrapper, one session, inputs through
`editor.keys(...)`, and assertions through files or clipboard state.
`run_x11_test` cleans the temp tree only after `Ok(())`; ordinary `?`
failures preserve scratch files and per-editor logs under
`artifacts/`.

To run:

```
cargo test -p lst-gpui --tests -- --ignored --test-threads=1 --nocapture
```

The multi-cursor suite includes TDD expectations for behavior that the
checklist still marks incomplete, so the full ignored run can fail until
those production behaviors land. Use a test filter when you specifically
want only the currently-green real-display subset.

`--test-threads=1` is still the intended mode — every test grabs
keyboard focus and moves the global pointer through XTEST. The harness
also takes a cross-process lock around each `Display`, so accidental
parallel runs serialize instead of racing, but serial test execution is
clearer and avoids wasting threads.

Useful env knobs: `LST_X11_WINDOW_TIMEOUT_MS=N` (default 30000) tunes
the "how long to wait for the editor's window to become viewable"
budget; `LST_X11_KEEP_TEMP=1` preserves all scratchpad temp dirs even
on successful tests.

---

## Synchronization model (load-bearing)

Every effectful action is fire-and-forget at the X11 layer. Callers
synchronize on **paint events**:

- `Editor::send_keys` waits for at least one matching DAMAGE event and
  then for that damage stream to quiesce after *each* keystroke before
  sending the next one. That gives proof that the editor processed and
  painted the key before the next key lands.
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

- **Principled synchronization.** `send_keys` now requires damage after
  each input before waiting for quiet. Validated by the slow-paced
  compound-command test and by the multi-cursor `Ctrl+D Ctrl+D Ctrl+D`
  test.
- **Small, consistent API.** `run_x11_test` →
  `ScratchpadSession::open`/`open_file` → `editor.keys(...)` →
  `editor.save_then_expect_file(...)` is the shape across every
  existing test.
- **Full printable ASCII typing** via level-aware XKB lookup
  (uppercase auto-shifts, digits and punctuation work without per-char
  extensions), plus `Tab` / `Enter` / `Escape` / `Backspace` /
  `Delete` / `Home` / `End` / arrow keys, plus modifier chords
  (`<C-x>`, `<A-x>`, `<S-x>`, `<C-A-S-x>`,
  `<ctrl-alt-shift-x>`).
- **27 tests across 5 files exercising different flavors:** literal
  type, vim modes (Normal/Insert/Visual-line), vim compound commands
  with arbitrary pauses, multi-cursor occurrence creation, adjacent-line
  cursor creation, per-cursor text input, clipboard distribution and
  collection, checklist-gap TDD specs, Ctrl+A/Z/Y, file-open,
  scratchpad-open with autosave, middle-click PRIMARY paste, Ctrl+V
  clipboard paste, goto-line focus return, edit-save-quit-reopen,
  clipboard-on-quit.
- **Failure artifacts are preserved.** `run_x11_test` preserves temp
  dirs on ordinary `?` failures, captures editor stdout/stderr under
  `artifacts/`, and `editor.keys`/`expect_file` attempt an `.xwd`
  window capture when those helpers fail.
- **Normal exits are asserted.** `quit_default` and
  `wait_for_successful_exit` fail if the child exits non-zero.
- **Missing files no longer equal empty files.** `wait_file_text`
  requires the target file to exist even when expected content is `""`,
  which avoids false positives for expected-empty assertions.
- **Real bugs already caught by the harness:** `Editor::quit` was
  leaking the child process on timeout; the window-discovery default
  was too tight under focus-stealing window managers (lwm
  specifically).

---

## Known gaps / things to watch

These are **explicit limitations** of the harness as it stands today.
Anyone writing a new test should know about them; anyone hardening the
harness should treat this list as the work-list.

1. **Sample size is still small.** 14 tests tells us the
   harness *can* be reliable; it does not yet tell us it *will be* at
   the 50–100 test scale. Reach the 30–50 test mark across the
   editor-behaviors checklist and watch for new flake patterns
   before declaring "rock solid."

2. **No way to observe non-file state.** Every assertion today goes
   through the autosave file or the X clipboard. That covers most of
   the editor-behaviors checklist but not all of it: vim mode,
   find-panel state, cursor position, selection extents, line-number
   gutter content, status-bar string. As tests grow into those areas
   we'll need an introspection mechanism — likely an out-of-band
   sentinel file the editor writes when in test mode, or a richer
   trace channel similar to `LST_BENCH_TRACE_FILE`.

3. **Modal-panel coverage is thin.** Goto-line now has one real-display
   workflow test, but find/replace and recent-files still change
   keyboard focus without real X11 coverage. There may be focus-routing
   or panel-dismiss races we don't know about.

4. **Fixed-pixel mouse coordinates only.** `Editor::middle_click_at(160,
   170)` and friends require the test author to know absolute pixel
   coordinates inside the window. Tests like "double-click the word
   `foo` on line 3" need a text-to-pixel coordinate API we don't
   have. Pure-keyboard tests cover most of the checklist; this only
   matters for explicitly mouse-driven behavior.

5. **`send_keys` throws on unmapped characters.** Anything outside
   ASCII printable (e.g. `é`, `→`, smart quotes) will hit a runtime
   error from the keymap lookup. Documented; fine for English-only
   source code; will need attention if non-ASCII inputs ever land in
   tests.

6. **Window discovery is environment-sensitive.** lwm's focus-stealing
   prevention occasionally takes 10s+ to map a freshly-spawned
   window. Default timeout is 30s, configurable via
   `LST_X11_WINDOW_TIMEOUT_MS`. CI machines with different WMs may
   need their own tuning.

7. **`send_keys` assumes a key should paint.** That is intentional for
   end-to-end behavior tests: if a synthesized key does not produce a
   paint, the helper fails instead of silently advancing. Tests for
   deliberate no-op keys should use lower-level `press` plus an
   explicit observable assertion.

---

## Hardening roadmap

Two concrete rounds before this is the project's load-bearing test
foundation:

1. **Drive the test count up to ~30–50 across the editor-behaviors
   checklist**, picking tests that exercise different feature surfaces:
   find-replace, indent/outdent, undo/redo branches, vim text objects,
   paragraph operations, selection extension, modal panels. Watch for
   new flake patterns at that scale; fix the underlying cause rather
   than papering over individual tests.
2. **Add a non-file observation channel** for state files cannot prove:
   vim mode, cursor position, selection ranges, active tab, find panel
   state, status text. Keep it read-only and test-only.

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
- `Display::from_env` holds an exclusive lock file under the system temp
  dir for the lifetime of the display. That protects the global pointer
  and keyboard focus when someone accidentally omits `--test-threads=1`.
- The bench example at `apps/lst-gpui/examples/bench_editor_x11.rs`
  has its own copy of the X11 primitives and is unchanged by the
  harness work. Migrating it onto the harness is a separate piece of
  work, deliberately deferred so the bench stays stable.
