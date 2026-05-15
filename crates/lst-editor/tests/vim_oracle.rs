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
    surround_mappings_detected: bool,
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
        if harness.text() != case.expected.text || harness.cursor() != expected_cursor || harness.model.vim_mode() != expected_mode {
            failures.push(format!("{} ({})\n  keys: {}\n  text: {:?} != {:?}\n  cursor: {:?} != {:?}\n  mode: {:?} != {:?}", case.name, case.area, case.keys, harness.text(), case.expected.text, harness.cursor(), expected_cursor, harness.model.vim_mode(), expected_mode,));
        }
    }

    assert!(failures.is_empty(), "nvim oracle mismatches:\n{}", failures.join("\n\n"));

    if fixture.metadata.surround_mappings_detected {
        assert!(fixture.cases.iter().any(|case| case.area == "surround"), "surround-enabled oracle fixtures should include surround cases");
    }
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
