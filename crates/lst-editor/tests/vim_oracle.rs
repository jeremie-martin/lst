mod support;

use lst_editor::{vim, Position};
use serde::Deserialize;

use support::VimHarness;

const FIXTURE: &str = include_str!("fixtures/vim_oracle.json");

#[derive(Debug, Deserialize)]
struct Fixture {
    metadata: Metadata,
    cases: Vec<OracleCase>,
}

#[derive(Debug, Deserialize)]
struct Metadata {
    oracle_profile: String,
    nvim_version: String,
}

#[derive(Debug, Deserialize)]
struct OracleCase {
    name: String,
    area: String,
    initial_text: String,
    cursor: FixturePosition,
    keys: String,
    expected: ExpectedState,
}

#[derive(Debug, Deserialize)]
struct ExpectedState {
    text: String,
    cursor: FixturePosition,
    mode: String,
    #[serde(default)]
    register: Option<ExpectedRegister>,
    #[serde(default)]
    search_query: Option<String>,
    #[serde(default)]
    selection: Option<String>,
    #[serde(default)]
    visual_state: Option<ExpectedVisualState>,
}

#[derive(Debug, Deserialize)]
struct ExpectedRegister {
    kind: String,
    text: String,
}

#[derive(Debug, Deserialize)]
struct ExpectedVisualState {
    anchor: FixturePosition,
    head: FixturePosition,
}

#[derive(Debug, Deserialize)]
struct FixturePosition {
    line: usize,
    column: usize,
}

#[test]
fn nvim_oracle_fixtures_match_editor_model() {
    let fixture: Fixture = serde_json::from_str(FIXTURE).expect("valid vim oracle fixture");

    assert_eq!(fixture.metadata.oracle_profile, "user_config");
    assert!(fixture.metadata.nvim_version.starts_with("NVIM v"), "fixture should record the nvim version that generated it");

    let mut failures = Vec::new();
    for case in &fixture.cases {
        let mut harness = VimHarness::normal_at(&case.initial_text, case.cursor.line, case.cursor.column);
        harness.keys(&case.keys);

        let expected_cursor = Position { line: case.expected.cursor.line, column: case.expected.cursor.column };
        let expected_mode = parse_mode(&case.expected.mode);
        let cursor_matches = case.expected.selection.is_some() || harness.cursor() == expected_cursor;
        let register_matches = case.expected.register.as_ref().is_none_or(|expected| register_matches(harness.model.vim_register(), expected));
        let search_matches = case.expected.search_query.as_ref().is_none_or(|expected| harness.model.find().query == *expected);
        let selection_matches = case.expected.selection.as_ref().is_none_or(|expected| harness.selected_text().as_deref() == Some(expected.as_str()));
        let visual_state_matches = case.expected.visual_state.as_ref().is_none_or(|expected| visual_state_matches(harness.model.vim_visual_state(), expected));
        if harness.text() != case.expected.text || !cursor_matches || harness.model.vim_mode() != expected_mode || !register_matches || !search_matches || !selection_matches || !visual_state_matches {
            failures.push(format!(
                "{} ({})\n  keys: {}\n  text: {:?} != {:?}\n  cursor: {:?} != {:?}\n  mode: {:?} != {:?}\n  register: {:?} != {:?}\n  search: {:?} != {:?}\n  selection: {:?} != {:?}\n  visual_state: {:?} != {:?}",
                case.name,
                case.area,
                case.keys,
                harness.text(),
                case.expected.text,
                harness.cursor(),
                expected_cursor,
                harness.model.vim_mode(),
                expected_mode,
                actual_register(harness.model.vim_register()),
                case.expected.register.as_ref().map(|expected| (expected.kind.as_str(), expected.text.as_str())),
                harness.model.find().query,
                case.expected.search_query,
                harness.selected_text(),
                case.expected.selection,
                harness.model.vim_visual_state(),
                case.expected.visual_state.as_ref().map(visual_state_tuple),
            ));
        }
    }

    assert!(failures.is_empty(), "nvim oracle mismatches:\n{}", failures.join("\n\n"));
}

fn actual_register(register: &vim::Register) -> (&'static str, &str) {
    match register {
        vim::Register::Empty => ("char", ""),
        vim::Register::Char(text) => ("char", text.as_str()),
        vim::Register::Line(text) => ("line", text.as_str()),
    }
}

fn register_matches(register: &vim::Register, expected: &ExpectedRegister) -> bool {
    actual_register(register) == (expected.kind.as_str(), expected.text.as_str())
}

fn visual_state_tuple(expected: &ExpectedVisualState) -> (Position, Position) {
    (Position { line: expected.anchor.line, column: expected.anchor.column }, Position { line: expected.head.line, column: expected.head.column })
}

fn visual_state_matches(actual: Option<vim::VisualState>, expected: &ExpectedVisualState) -> bool {
    actual.map(|state| (state.anchor, state.head)) == Some(visual_state_tuple(expected))
}

fn parse_mode(mode: &str) -> vim::Mode {
    match mode {
        "Normal" => vim::Mode::Normal,
        "Insert" => vim::Mode::Insert,
        "Visual" => vim::Mode::Visual,
        "VisualLine" => vim::Mode::VisualLine,
        other => panic!("unsupported vim fixture mode: {other}"),
    }
}
