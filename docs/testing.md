# Testing

Tests are organized by the boundary they protect. The real-X11 suite is the
acceptance contract for user-visible behavior; source-side tests protect smaller
model, representation, adapter, and tooling contracts.

## Principles

- Drive the same entry points a user or production caller uses.
- Exercise real production code between setup and assertion.
- Assert observable outcomes: text, selections, clipboard contents, visible
  state, geometry, or boundary results.
- Fake only a boundary that the test cannot drive directly.
- Treat difficult setup as a design signal. Prefer a narrower production
  boundary over a more elaborate mock.

For user-visible behavior, prefer the X11 lane even when a unit test would be
shorter. Do not expose private functions or inspect transactions merely to keep
a source-side behavior test.

## Test lanes

### Real-X11 acceptance

`apps/lst-gpui/tests/real_x11_*.rs` launches the production `lst` binary and
sends real XTEST keyboard and mouse input. Tests assert saved files, CLIPBOARD
and PRIMARY, cursor and selection state, panels, status text, and viewport
geometry. Shared setup and behavior helpers live in
`apps/lst-gpui/tests/support/mod.rs`.

`crates/lst-x11-harness` owns X11 discovery, input synthesis, clipboard access,
screenshots, and the JSONL state-trace reader. The trace is a test-only
observation channel for user-visible state, not a model transaction log.

`apps/lst-gpui/tests/real_x11_visual.rs` is the physical-display pixel lane.
The other `real_x11_*` binaries form the normal off-screen behavior lane.

### Model and source-side tests

`crates/lst-editor/tests` drives the public `EditorModel` surface. It contains
the exhaustive Vim parity suite and oracle replay, plus a small number of
editor-model workflows that are impractical to repeat exhaustively through a
display.

Inline tests under `crates/lst-editor/src` are for small algorithms and
representation invariants. Inline app tests are for settings, parsing,
tree-sitter adapters, incremental-cache equivalence, and runtime boundary
contracts. Harness and benchmark self-tests protect test tooling rather than
editor behavior.

Keep these tests narrow. If setup becomes a user workflow, move the contract to
X11. Do not maintain a prose inventory of individual tests; the test files are
the current inventory.

## Fast checks

The root workspace selects the app and editor as default members:

```sh
cargo test
cargo test --all-features
cargo test -p lst-editor --features internal-invariants
cargo test -p lst-x11-harness
```

`cargo test` is fast feedback, not the final gate for desktop behavior.

## Running the X11 suite

The normal gate creates an off-screen Xephyr server and runs the `x11-nested`
nextest profile:

```sh
./scripts/run_x11_nested.py
```

This needs a host X11 session, Cargo nextest, Python Xlib, and these commands on
`PATH`: `Xephyr`, `lwm`, `wmctrl`, `xclip`, `xprop`, `xwininfo`, `xdpyinfo`, and
`setxkbmap`. The required and recommended nextest versions are recorded in
`.config/nextest.toml`.

Useful forms are:

```sh
./scripts/run_x11_nested.py --probe
./scripts/run_x11_nested.py --visible --keep-session
./scripts/run_x11_nested.py -- \
  cargo nextest run --profile x11-nested -p lst-gpui \
  --test real_x11_find --run-ignored only
./scripts/run_x11_nested.py -- \
  cargo nextest run --profile x11-nested -p lst-gpui \
  --tests --run-ignored only --stress-count 3
```

Use a physical display for visual baselines or to investigate a nested-only
difference:

```sh
DISPLAY=:0 cargo nextest run --profile x11 -p lst-gpui --tests --run-ignored only
```

Both profiles run serially because tests take keyboard focus and move the X11
pointer. `x11-nested` excludes `real_x11_visual`; `x11` includes it. All test
functions are ignored under ordinary Cargo runs and selected with
`--run-ignored only`.

## Writing an X11 test

Name the test after observable behavior and use `support::run_x11_test`:

```rust
mod support;

use support::{EditorTestExt, TestResult};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn duplicate_line_changes_the_saved_document() -> TestResult {
    support::run_x11_test("duplicate-line", |session| {
        let path = session.seed_file("document.txt", "alpha\n")?;
        let mut editor = session.open_file("duplicate-line", &path)?;

        editor.keys("<C-S-d>")?;
        editor.save_then_expect_file(&path, "alpha\nalpha\n")?;
        Ok(())
    })
}
```

Prefer the highest-level observable:

- Use `save_then_expect_file` for text transformations.
- Use clipboard helpers for CLIPBOARD or PRIMARY behavior.
- Use behavior-named trace helpers such as `expect_cursor_heads` and
  `expect_find_state` for state that is not cleanly observable through a file.
- Use text-coordinate mouse helpers such as `click_at_text`,
  `alt_click_at_text`, and `drag_text`; fixed pixels are reserved for chrome
  whose bounds are themselves the subject of the test.

The key DSL accepts printable ASCII, names such as `<enter>`, `<esc>`, `<bs>`,
`<delete>`, `<home>`, and `<pageup>`, modifier forms such as `<C-x>` and
`<C-A-S-x>`, and held spans such as `<C-{k d}>`. It does not synthesize
non-ASCII printable text.

Every X11 action is asynchronous. `open`, `open_file`, `keys`, `wait_quiet`,
`wait_state`, file waits, and exit waits provide synchronization. Do not add
fixed sleeps between keys: they hide frame races and can break multi-key Vim
commands. The harness's pointer-settle delay is the intentional exception.

## Diagnostics and artifacts

On failure, `run_x11_test` preserves the scratch directory, editor output, and
a best-effort window capture. Visual failures also write expected, actual, and
diff images. Nextest JUnit reports live under `target/nextest/<profile>/`.

Environment controls:

- `LST_X11_WINDOW_TIMEOUT_MS`: window discovery timeout; default 30000
- `LST_X11_KEEP_TEMP=1`: preserve per-test temporary files on success
- `LST_GPUI_BIN=/path/to/lst`: test a specific editor binary
- `LST_X11_HARNESS_LAYOUT`, `LST_X11_HARNESS_VARIANT`,
  `LST_X11_HARNESS_OPTIONS`: override the pinned XKB layout
- `LST_VISUAL_SCENARIO=<name>`: run one visual scenario
- `LST_VISUAL_BASELINE_DIR=/path/to/reference-images`: use an alternate image
  directory for same-environment comparisons of preserved binaries; defaults
  to the committed `tests/visual_baselines` directory
- `LST_UPDATE_VISUAL_BASELINES=1`: replace the selected expected image

If display geometry differs from the committed images, capture references from
the preserved **old** binary into a temporary directory, then compare the new
binary against that directory. Keep the display configuration fixed and do not
update references from the new binary to hide a mismatch.

At large desktop sizes, run visual scenarios individually with
`LST_VISUAL_SCENARIO` if the grouped fresh-launch checks exceed the test
timeout. Keep the same repeatability and exact-pixel assertions.

Inspect every updated baseline image. A repeatable capture of another window is
not a valid result.

## Qualifying the nested environment

Run this after changing nested-display setup, harness synchronization, profile
partitioning, or assumptions about the display:

```sh
./scripts/qualify_x11_nested.py
```

Qualification runs the same baseline and deliberately faulty binaries on the
physical and nested displays and requires equivalent behavioral results. Pixel
identity is not expected because compositor, GPU, DPI, and font rasterization
differ. Reports, binaries, JUnit files, and logs are written under
`target/x11-qualification/`.
