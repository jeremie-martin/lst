//! Real-display visual snapshot probes for chrome/UI refactors. These tests
//! intentionally live in the X11 harness layer: product code should not know
//! about screenshots.
//!
//! Run with
//!
//!     cargo nextest run --profile x11 -p lst-gpui --test real_x11_visual --run-ignored only

mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use lst_x11_harness::{Editor, Screenshot, ScreenshotDiff};

use support::{secs, EditorTestExt, ScratchpadSession, SupportResult, TestResult};

const CAPTURE_QUIET: Duration = Duration::from_millis(150);
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(5);
const COMPOSITOR_CORNER_MASK_PX: u16 = 8;
const STATE_PRESENTATION_SETTLE: Duration = Duration::from_millis(1_500);
const PRESENTATION_SETTLE: Duration = Duration::from_millis(500);
const FRESH_LAUNCHES: usize = 3;
const VISUAL_FIXTURE_ROOT: &str = "/tmp/lst-x11-visual-fixtures";

#[derive(Clone, Copy)]
struct VisualScenario {
    name: &'static str,
    theme: &'static str,
    capture: fn(&mut ScratchpadSession, usize) -> SupportResult<Screenshot>,
}

const SCENARIOS: &[VisualScenario] = &[
    VisualScenario {
        name: "clean-editor",
        theme: "dark",
        capture: capture_clean_editor,
    },
    VisualScenario {
        name: "clean-editor-light",
        theme: "light",
        capture: capture_clean_editor_light,
    },
    VisualScenario {
        name: "dirty-tab",
        theme: "dark",
        capture: capture_dirty_tab,
    },
    VisualScenario {
        name: "find-panel",
        theme: "dark",
        capture: capture_find_panel,
    },
    VisualScenario {
        name: "identifier-highlights",
        theme: "dark",
        capture: capture_identifier_highlights,
    },
    VisualScenario {
        name: "scrolled-gutter",
        theme: "dark",
        capture: capture_scrolled_gutter,
    },
    VisualScenario {
        name: "recent-files",
        theme: "dark",
        capture: capture_recent_files,
    },
    VisualScenario {
        name: "multi-cursor-status",
        theme: "dark",
        capture: capture_multi_cursor_status,
    },
];

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn visual_scenarios_are_exactly_repeatable() -> TestResult {
    support::run_x11_test("visual-repeatability", |session| {
        let update_baselines = std::env::var_os("LST_UPDATE_VISUAL_BASELINES").is_some();
        for scenario in SCENARIOS {
            session.seed_settings(&format!(
                "version = 1\n[editor]\ncursor_blink = false\n[appearance]\ntheme = '{}'\n",
                scenario.theme
            ))?;
            let expected = (scenario.capture)(session, 0)?;
            expected.write_ppm(session.artifacts().join(format!("{}-expected.ppm", scenario.name)))?;
            if update_baselines {
                expected.write_ppm(baseline_path(scenario.name))?;
            } else {
                let baseline = Screenshot::read_ppm(baseline_path(scenario.name))?;
                assert_screenshot_exact(
                    session.artifacts(),
                    &format!("{}-baseline", scenario.name),
                    &expected,
                    &baseline,
                )?;
            }

            for run in 1..FRESH_LAUNCHES {
                let actual = (scenario.capture)(session, run)?;
                assert_screenshot_exact(
                    session.artifacts(),
                    &format!("{}-fresh-launch-{run}", scenario.name),
                    &actual,
                    &expected,
                )?;
            }
        }
        Ok(())
    })
}

fn baseline_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("visual_baselines")
        .join(format!("{name}.ppm"))
}

fn capture_clean_editor(session: &mut ScratchpadSession, run: usize) -> SupportResult<Screenshot> {
    let dir = reset_fixture_dir("clean-editor")?;
    let path = write_fixture(
        &dir,
        "main.rs",
        "fn main() {\n    println!(\"visual snapshot\");\n}\n\nfn helper() -> usize {\n    42\n}\n",
    )?;
    let path_text = path_text(&path);
    let artifacts = session.artifacts().to_path_buf();
    let mut editor = session.open_file(&format!("visual-clean-editor-{run}"), &path)?;
    prepare_visual_window(&mut editor)?;
    editor.wait_state("visual clean editor ready", secs(5), |record| {
        record.active_tab_path.as_deref() == Some(path_text.as_str())
            && !record.active_tab_modified
            && record.viewport.bounds_size_px.is_some()
    })?;
    settled_screenshot(&mut editor, &artifacts, "clean-editor-same-window")
}

fn capture_clean_editor_light(session: &mut ScratchpadSession, run: usize) -> SupportResult<Screenshot> {
    let dir = reset_fixture_dir("clean-editor-light")?;
    let path = write_fixture(
        &dir,
        "main.rs",
        "fn main() {\n    println!(\"visual snapshot\");\n}\n\nfn helper() -> usize {\n    42\n}\n",
    )?;
    let path_text = path_text(&path);
    let artifacts = session.artifacts().to_path_buf();
    let mut editor = session.open_file(&format!("visual-clean-editor-light-{run}"), &path)?;
    prepare_visual_window(&mut editor)?;
    editor.wait_state("visual light editor ready", secs(5), |record| {
        record.active_tab_path.as_deref() == Some(path_text.as_str())
            && record.theme_name == "Light"
            && record.viewport.bounds_size_px.is_some()
    })?;
    settled_screenshot(&mut editor, &artifacts, "clean-editor-light-same-window")
}

fn capture_dirty_tab(session: &mut ScratchpadSession, run: usize) -> SupportResult<Screenshot> {
    let dir = reset_fixture_dir("dirty-tab")?;
    let path = write_fixture(&dir, "dirty.md", "alpha\nbeta\ngamma\n")?;
    let artifacts = session.artifacts().to_path_buf();
    let mut editor = session.open_file(&format!("visual-dirty-tab-{run}"), &path)?;
    prepare_visual_window(&mut editor)?;
    editor.keys("X")?;
    editor.wait_state("visual dirty tab ready", secs(2), |record| record.active_tab_modified)?;
    settled_screenshot(&mut editor, &artifacts, "dirty-tab-same-window")
}

fn capture_find_panel(session: &mut ScratchpadSession, run: usize) -> SupportResult<Screenshot> {
    let dir = reset_fixture_dir("find-panel")?;
    let path = write_fixture(
        &dir,
        "find.txt",
        "needle in the first line\nsecond line\nanother needle here\n",
    )?;
    let artifacts = session.artifacts().to_path_buf();
    let mut editor = session.open_file(&format!("visual-find-panel-{run}"), &path)?;
    prepare_visual_window(&mut editor)?;
    editor.keys("<C-f>needle")?;
    editor.expect_find_state("needle", 2)?;
    settled_screenshot(&mut editor, &artifacts, "find-panel-same-window")
}

fn capture_identifier_highlights(session: &mut ScratchpadSession, run: usize) -> SupportResult<Screenshot> {
    let dir = reset_fixture_dir("identifier-highlights")?;
    let path = write_fixture(
        &dir,
        "identifiers.rs",
        "let target = source;\nlet copy = target;\nlet target_count = target;\n",
    )?;
    let artifacts = session.artifacts().to_path_buf();
    let mut editor = session.open_file(&format!("visual-identifier-highlights-{run}"), &path)?;
    prepare_visual_window(&mut editor)?;
    editor.place_cursor_at_document_start()?;
    editor.keys("<C-g>1:5<enter>")?;
    editor.wait_state("visual identifier highlights ready", secs(5), |record| {
        record.viewport.occurrence_highlights.len() == 3
    })?;
    settled_screenshot(&mut editor, &artifacts, "identifier-highlights-same-window")
}

fn capture_scrolled_gutter(session: &mut ScratchpadSession, run: usize) -> SupportResult<Screenshot> {
    session.seed_settings(
        "version = 1\n[editor]\ncursor_blink = false\nword_wrap = false\n[appearance]\ntheme = 'dark'\n",
    )?;
    let dir = reset_fixture_dir("scrolled-gutter")?;
    let mut contents = "leading_identifier ".repeat(180);
    contents.push_str("visible_tail");
    let path = write_fixture(&dir, "long-line.txt", &contents)?;
    let artifacts = session.artifacts().to_path_buf();
    let mut editor = session.open_file(&format!("visual-scrolled-gutter-{run}"), &path)?;
    prepare_visual_window(&mut editor)?;
    editor.keys("<C-end>")?;
    editor.wait_state("visual horizontal scroll ready", secs(5), |record| {
        record.viewport.scroll_left_px > record.viewport.char_width_px * 20.0 && record.viewport.gutter_width_px > 0.0
    })?;
    settled_screenshot(&mut editor, &artifacts, "scrolled-gutter-same-window")
}

fn capture_recent_files(session: &mut ScratchpadSession, run: usize) -> SupportResult<Screenshot> {
    let dir = reset_fixture_dir("recent-files")?;
    let active = write_fixture(&dir, "active.md", "active editor tab\n")?;
    let recent = [
        write_fixture(&dir, "alpha-note.md", "alpha preview\nline two\nline three\n")?,
        write_fixture(&dir, "beta-note.md", "beta preview\nline two\nline three\n")?,
        write_fixture(&dir, "gamma-note.md", "gamma preview\nline two\nline three\n")?,
        write_fixture(&dir, "delta-note.md", "delta preview\nline two\nline three\n")?,
    ];
    session.seed_recent_files(&recent)?;
    let first_recent = path_text(&active);
    let artifacts = session.artifacts().to_path_buf();

    let mut editor = session.open_file(&format!("visual-recent-files-{run}"), &active)?;
    prepare_visual_window(&mut editor)?;
    editor.keys("<C-r>")?;
    editor.wait_state("visual recent panel ready", secs(5), |record| {
        record.recent_panel_open
            && record.focused_input == "recent_query"
            && record.recent_panel_selected_path.as_deref() == Some(first_recent.as_str())
            && !record.recent_panel_content_search_pending
    })?;
    settled_screenshot(&mut editor, &artifacts, "recent-files-same-window")
}

fn capture_multi_cursor_status(session: &mut ScratchpadSession, run: usize) -> SupportResult<Screenshot> {
    let dir = reset_fixture_dir("multi-cursor-status")?;
    let path = write_fixture(&dir, "multi.txt", "foo foo foo\nbar baz\n")?;
    let artifacts = session.artifacts().to_path_buf();
    let mut editor = session.open_file(&format!("visual-multi-cursor-{run}"), &path)?;
    prepare_visual_window(&mut editor)?;
    editor.place_cursor_at_document_start()?;
    editor.keys("<C-d><C-d>")?;
    editor.wait_state("visual multi cursor ready", secs(5), |record| {
        record.cursors.len() == 2 && record.status_bar.contains("2 cursors")
    })?;
    settled_screenshot(&mut editor, &artifacts, "multi-cursor-same-window")
}

fn settled_screenshot(editor: &mut Editor<'_>, artifacts: &Path, label: &str) -> SupportResult<Screenshot> {
    let deadline = Instant::now() + CAPTURE_TIMEOUT;
    // A state-trace record proves that app state and layout are ready, but
    // presentation through the physical compositor can lag that record.
    thread::sleep(STATE_PRESENTATION_SETTLE);
    let mut previous = visual_screenshot(editor)?;
    loop {
        thread::sleep(CAPTURE_QUIET);
        let current = visual_screenshot(editor)?;
        if current.diff(&previous)?.is_exact() {
            return Ok(current);
        }
        if Instant::now() >= deadline {
            assert_screenshot_exact(artifacts, label, &current, &previous)?;
            unreachable!("a non-exact screenshot comparison returns an error");
        }
        previous = current;
    }
}

fn visual_screenshot(editor: &mut Editor<'_>) -> SupportResult<Screenshot> {
    editor.raise_and_focus()?;
    // Raising an occluded Vulkan window makes its latest backing image
    // eligible for composition, but the overlay can still contain the last
    // presented frame for a short interval. Sample only after that frame has
    // reached the compositor.
    thread::sleep(PRESENTATION_SETTLE);
    editor.screenshot()?.mask_corner_squares(COMPOSITOR_CORNER_MASK_PX)
}

fn prepare_visual_window(editor: &mut Editor<'_>) -> SupportResult<()> {
    editor.raise_and_focus()?;
    Ok(())
}

fn assert_screenshot_exact(
    artifacts: &Path,
    label: &str,
    actual: &Screenshot,
    expected: &Screenshot,
) -> SupportResult<()> {
    let diff = actual.diff(expected)?;
    if diff.is_exact() {
        return Ok(());
    }

    let expected_path = artifacts.join(format!("{label}-expected.ppm"));
    let actual_path = artifacts.join(format!("{label}-actual.ppm"));
    let diff_path = artifacts.join(format!("{label}-diff.ppm"));
    expected.write_ppm(&expected_path)?;
    actual.write_ppm(&actual_path)?;
    actual.write_diff_ppm(expected, &diff_path)?;

    Err(format!(
        "visual snapshot {label:?} changed: {}\nexpected: {}\nactual: {}\ndiff: {}",
        describe_diff(&diff),
        expected_path.display(),
        actual_path.display(),
        diff_path.display()
    )
    .into())
}

fn describe_diff(diff: &ScreenshotDiff) -> String {
    format!(
        "{} / {} pixels changed ({:.6}%), first={:?}, bounds={:?}, grid={}",
        diff.changed_pixels,
        diff.total_pixels,
        diff.changed_percent(),
        diff.first_diff,
        diff.changed_bounds,
        format_grid(diff.grid_percentages())
    )
}

fn format_grid(values: [f64; 9]) -> String {
    format!(
        "[[{:.4}, {:.4}, {:.4}], [{:.4}, {:.4}, {:.4}], [{:.4}, {:.4}, {:.4}]]",
        values[0], values[1], values[2], values[3], values[4], values[5], values[6], values[7], values[8]
    )
}

fn reset_fixture_dir(name: &str) -> SupportResult<PathBuf> {
    let dir = PathBuf::from(VISUAL_FIXTURE_ROOT).join(name);
    match fs::remove_dir_all(&dir) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn write_fixture(dir: &Path, name: &str, contents: &str) -> SupportResult<PathBuf> {
    let path = dir.join(name);
    fs::write(&path, contents)?;
    Ok(path)
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
