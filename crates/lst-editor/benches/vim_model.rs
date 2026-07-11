use std::{hint::black_box, path::PathBuf, time::Duration};

use criterion::{criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput};
use lst_editor::{
    vim::{self, Key, NamedKey},
    EditorCommand, EditorEffect, EditorModel, EditorTab, FocusTarget, InputMode, Position, TabId,
};
use serde::Deserialize;

const WRAP_COLUMNS: usize = 80;
const VERTICAL_STEPS: usize = 512;
const WORD_STEPS: usize = 128;
const DELETE_UNDO_CYCLES: usize = 64;
const CHANGE_UNDO_CYCLES: usize = 32;
const VISUAL_YANK_CYCLES: usize = 64;
const COPY_PASTE_CYCLES: usize = 16;
const SEARCH_NEXT_STEPS: usize = 512;
const SEARCH_WORD_CYCLES: usize = 32;
const DELETE_ALL_UNDO_CYCLES: usize = 8;
const EDIT_AFTER_MUTATION_LINES: usize = 65_536;
const EDIT_AFTER_MUTATION_CYCLES: usize = 16;
const ORACLE_FIXTURE: &str = include_str!("../tests/fixtures/vim_oracle.json");

fn bench_vim_motion(c: &mut Criterion) {
    let mut group = c.benchmark_group("vim_motion");

    for size in DocumentSize::all() {
        let text = generated_vim_corpus(size.lines());

        group.throughput(Throughput::Elements((VERTICAL_STEPS * 2) as u64));
        group.bench_with_input(BenchmarkId::new("vertical", size.label()), &size, |b, size| {
            b.iter_batched(
                || VimDriver::normal_at(&text, size.start_line(), 0),
                |mut driver| {
                    for _ in 0..VERTICAL_STEPS {
                        driver.keys("j");
                    }
                    for _ in 0..VERTICAL_STEPS {
                        driver.keys("k");
                    }
                    black_box(driver.cursor());
                },
                BatchSize::LargeInput,
            );
        });

        group.throughput(Throughput::Elements((WORD_STEPS * 2) as u64));
        group.bench_with_input(BenchmarkId::new("word", size.label()), &size, |b, size| {
            b.iter_batched(
                || VimDriver::normal_at(&text, size.start_line(), 0),
                |mut driver| {
                    for _ in 0..WORD_STEPS {
                        driver.keys("w");
                    }
                    for _ in 0..WORD_STEPS {
                        driver.keys("b");
                    }
                    black_box(driver.cursor());
                },
                BatchSize::LargeInput,
            );
        });
    }

    group.finish();
}

fn bench_vim_edit(c: &mut Criterion) {
    let mut group = c.benchmark_group("vim_edit");

    for size in DocumentSize::all() {
        let text = generated_vim_corpus(size.lines());
        let line = size.start_line();

        group.throughput(Throughput::Elements(DELETE_UNDO_CYCLES as u64));
        group.bench_with_input(BenchmarkId::new("delete_word_undo", size.label()), &size, |b, _| {
            b.iter_batched(
                || VimDriver::normal_at(&text, line, 0),
                |mut driver| {
                    for _ in 0..DELETE_UNDO_CYCLES {
                        driver.set_cursor(line, 0);
                        driver.keys("dwu");
                    }
                    black_box(driver.text_len());
                },
                BatchSize::LargeInput,
            );
        });

        group.throughput(Throughput::Elements(CHANGE_UNDO_CYCLES as u64));
        group.bench_with_input(
            BenchmarkId::new("change_inner_word_escape_undo", size.label()),
            &size,
            |b, _| {
                b.iter_batched(
                    || VimDriver::normal_at(&text, line, 2),
                    |mut driver| {
                        for _ in 0..CHANGE_UNDO_CYCLES {
                            driver.set_cursor(line, 2);
                            driver.keys("ciw");
                            driver.insert_text("replacement");
                            driver.escape();
                            driver.keys("u");
                        }
                        black_box(driver.text_len());
                    },
                    BatchSize::LargeInput,
                );
            },
        );
    }

    group.finish();
}

fn bench_vim_edit_after_mutation(c: &mut Criterion) {
    let mut group = c.benchmark_group("vim_edit_after_mutation");
    let text = generated_vim_corpus(EDIT_AFTER_MUTATION_LINES);
    let line = EDIT_AFTER_MUTATION_LINES / 2;

    group.throughput(Throughput::Bytes((text.len() * EDIT_AFTER_MUTATION_CYCLES) as u64));
    group.bench_function("delete_then_next_key_huge", |b| {
        b.iter_batched(
            || VimDriver::normal_at(&text, line, 0),
            |mut driver| {
                for _ in 0..EDIT_AFTER_MUTATION_CYCLES {
                    driver.set_cursor(line, 0);
                    driver.keys("dwju");
                }
                black_box(driver.text_len());
            },
            BatchSize::LargeInput,
        );
    });

    group.finish();
}

fn bench_vim_visual(c: &mut Criterion) {
    let mut group = c.benchmark_group("vim_visual");

    for size in DocumentSize::all() {
        let text = generated_vim_corpus(size.lines());
        let line = size.start_line();

        group.throughput(Throughput::Elements(VISUAL_YANK_CYCLES as u64));
        group.bench_with_input(BenchmarkId::new("visual_yank_word", size.label()), &size, |b, _| {
            b.iter_batched(
                || VimDriver::normal_at(&text, line, 0),
                |mut driver| {
                    for _ in 0..VISUAL_YANK_CYCLES {
                        driver.set_cursor(line, 0);
                        driver.keys("viwy");
                    }
                    black_box(driver.cursor());
                },
                BatchSize::LargeInput,
            );
        });
    }

    group.finish();
}

fn bench_vim_copy_paste(c: &mut Criterion) {
    let mut group = c.benchmark_group("vim_copy_paste");

    for size in DocumentSize::all() {
        let text = generated_vim_corpus(size.lines());
        let copied_bytes = size.copy_paste_lines() * size.approx_line_bytes();
        let sequence = format!("{}yypu", size.copy_paste_lines());

        group.throughput(Throughput::Bytes((copied_bytes * COPY_PASTE_CYCLES) as u64));
        group.bench_with_input(
            BenchmarkId::new("yank_paste_undo_lines", size.label()),
            &size,
            |b, _| {
                b.iter_batched(
                    || VimDriver::normal_at(&text, 0, 0),
                    |mut driver| {
                        for _ in 0..COPY_PASTE_CYCLES {
                            driver.set_cursor(0, 0);
                            driver.keys(&sequence);
                        }
                        black_box(driver.text_len());
                    },
                    BatchSize::LargeInput,
                );
            },
        );
    }

    group.finish();
}

fn bench_vim_search(c: &mut Criterion) {
    let mut group = c.benchmark_group("vim_search");

    for size in DocumentSize::all() {
        let text = generated_search_corpus(size.lines());

        group.throughput(Throughput::Bytes(text.len() as u64));
        group.bench_with_input(BenchmarkId::new("submit_query", size.label()), &size, |b, _| {
            b.iter_batched(
                || VimDriver::normal_at(&text, 0, 0),
                |mut driver| {
                    driver.keys("/needle<Enter>");
                    black_box(driver.cursor());
                },
                BatchSize::LargeInput,
            );
        });

        group.throughput(Throughput::Elements(SEARCH_NEXT_STEPS as u64));
        group.bench_with_input(BenchmarkId::new("next_matches", size.label()), &size, |b, _| {
            b.iter_batched(
                || {
                    let mut driver = VimDriver::normal_at(&text, 0, 0);
                    driver.keys("/needle<Enter>");
                    driver
                },
                |mut driver| {
                    for _ in 0..SEARCH_NEXT_STEPS {
                        driver.keys("n");
                    }
                    black_box(driver.cursor());
                },
                BatchSize::LargeInput,
            );
        });

        group.throughput(Throughput::Bytes((text.len() * SEARCH_WORD_CYCLES) as u64));
        group.bench_with_input(BenchmarkId::new("word_under_cursor", size.label()), &size, |b, _| {
            b.iter_batched(
                || VimDriver::normal_at(&text, 0, 0),
                |mut driver| {
                    for _ in 0..SEARCH_WORD_CYCLES {
                        driver.set_cursor(0, 0);
                        driver.keys("*");
                    }
                    black_box(driver.cursor());
                },
                BatchSize::LargeInput,
            );
        });
    }

    group.finish();
}

fn bench_vim_whole_document(c: &mut Criterion) {
    let mut group = c.benchmark_group("vim_whole_document");

    for size in DocumentSize::all() {
        let text = generated_vim_corpus(size.lines());

        group.throughput(Throughput::Bytes((text.len() * DELETE_ALL_UNDO_CYCLES) as u64));
        group.bench_with_input(BenchmarkId::new("dgg_undo_from_end", size.label()), &size, |b, _| {
            b.iter_batched(
                || VimDriver::normal_at(&text, 0, 0),
                |mut driver| {
                    for _ in 0..DELETE_ALL_UNDO_CYCLES {
                        driver.keys("Gdggu");
                    }
                    black_box(driver.text_len());
                },
                BatchSize::LargeInput,
            );
        });
    }

    group.finish();
}

fn bench_vim_oracle(c: &mut Criterion) {
    let specs = oracle_specs();
    let event_count = specs.iter().map(|case| parse_keys(&case.keys).len() as u64).sum();
    let mut group = c.benchmark_group("vim_oracle");
    group.throughput(Throughput::Elements(event_count));

    group.bench_function("replay_fixture", |b| {
        b.iter_batched(
            || oracle_batch(&specs),
            |mut cases| {
                let mut checksum = 0usize;
                for case in cases.iter_mut() {
                    case.driver.keys(&case.keys);
                    let cursor = case.driver.cursor();
                    checksum = checksum
                        .wrapping_add(cursor.line)
                        .wrapping_add(cursor.column)
                        .wrapping_add(case.driver.text_len());
                }
                black_box(checksum);
            },
            BatchSize::LargeInput,
        );
    });

    group.finish();
}

#[derive(Clone, Copy)]
enum DocumentSize {
    Small,
    Medium,
    Large,
}

impl DocumentSize {
    fn all() -> [Self; 3] {
        [Self::Small, Self::Medium, Self::Large]
    }

    fn label(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Medium => "medium",
            Self::Large => "large",
        }
    }

    fn lines(self) -> usize {
        match self {
            Self::Small => 128,
            Self::Medium => 1_024,
            Self::Large => 4_096,
        }
    }

    fn start_line(self) -> usize {
        self.lines() / 2
    }

    fn copy_paste_lines(self) -> usize {
        match self {
            Self::Small => 16,
            Self::Medium => 128,
            Self::Large => 512,
        }
    }

    fn approx_line_bytes(self) -> usize {
        generated_vim_line(0).len() + 1
    }
}

struct VimDriver {
    model: EditorModel,
    focus: FocusTarget,
    deferred_find_query: Option<String>,
}

impl VimDriver {
    fn new(text: &str) -> Self {
        let tab = EditorTab::from_path_with_stamp(TabId::from_raw(1), PathBuf::from("vim-bench.md"), text, None);
        let mut model = EditorModel::from_tabs(tab, Vec::new(), "Ready.".to_string());
        model.set_input_mode(InputMode::Vim);
        let mut driver = Self {
            model,
            focus: FocusTarget::Editor,
            deferred_find_query: None,
        };
        driver.sync_effects();
        driver
    }

    fn normal_at(text: &str, line: usize, column: usize) -> Self {
        let mut driver = Self::new(text);
        driver.escape();
        driver.set_cursor(line, column);
        driver.clear_effects();
        driver
    }

    fn keys(&mut self, sequence: &str) {
        for (key, modifiers) in parse_keys(sequence) {
            self.key(key, modifiers);
        }
    }

    fn key(&mut self, key: Key, modifiers: vim::Modifiers) {
        match self.focus {
            FocusTarget::Editor => self.editor_key(key, modifiers),
            FocusTarget::FindQuery => self.find_query_key(key),
            FocusTarget::FindReplace | FocusTarget::GotoLine => {}
        }
        self.sync_effects();
    }

    fn editor_key(&mut self, key: Key, modifiers: vim::Modifiers) {
        if key == Key::Named(NamedKey::Enter) && self.model.vim_mode() == vim::Mode::Insert {
            self.model.execute(EditorCommand::InsertNewline);
            return;
        }
        if key == Key::Named(NamedKey::Tab) && self.model.vim_mode() == vim::Mode::Insert {
            self.model.execute(EditorCommand::InsertTab);
            return;
        }
        if key == Key::Named(NamedKey::Backspace) && self.model.vim_mode() == vim::Mode::Insert {
            self.model.execute(EditorCommand::Backspace);
            return;
        }
        if key == Key::Named(NamedKey::Delete) && self.model.vim_mode() == vim::Mode::Insert {
            self.model.execute(EditorCommand::DeleteForward);
            return;
        }
        if key == Key::Named(NamedKey::ArrowLeft) && self.model.vim_mode() == vim::Mode::Insert {
            self.model.execute(EditorCommand::MoveHorizontal(-1, false));
            return;
        }
        if key == Key::Named(NamedKey::ArrowRight) && self.model.vim_mode() == vim::Mode::Insert {
            self.model.execute(EditorCommand::MoveHorizontal(1, false));
            return;
        }
        if key == Key::Character("\u{1b}".to_string()) {
            self.model.handle_vim_escape();
            return;
        }
        if self.model.vim_mode() == vim::Mode::Insert && !modifiers.command && !modifiers.control {
            if let Key::Character(text) = key {
                self.model.replace_text_from_input(None, text);
                return;
            }
        }

        self.model.handle_vim_key(key, modifiers, WRAP_COLUMNS);
    }

    fn find_query_key(&mut self, key: Key) {
        match key {
            Key::Character(text) if text == "\u{1b}" => {
                self.deferred_find_query = None;
                self.model.close_find_panel();
            }
            Key::Character(text) => {
                self.deferred_find_query
                    .get_or_insert_with(|| self.model.find().query.clone())
                    .push_str(&text);
            }
            Key::Named(NamedKey::Backspace) => {
                let mut query = self
                    .deferred_find_query
                    .take()
                    .unwrap_or_else(|| self.model.find().query.clone());
                query.pop();
                self.deferred_find_query = Some(query);
            }
            Key::Named(NamedKey::Enter) => {
                let query = self
                    .deferred_find_query
                    .take()
                    .unwrap_or_else(|| self.model.find().query.clone());
                let was_visual = matches!(self.model.vim_mode(), vim::Mode::Visual | vim::Mode::VisualLine);
                self.model.update_find_query_and_activate(query);
                if was_visual {
                    self.model.close_find_panel();
                } else {
                    self.model.submit_find_query();
                }
            }
            _ => {}
        }
    }

    fn insert_text(&mut self, text: &str) {
        self.model.replace_text_from_input(None, text.to_string());
        self.sync_effects();
    }

    fn escape(&mut self) {
        self.model.handle_vim_escape();
        self.sync_effects();
    }

    fn set_cursor(&mut self, line: usize, column: usize) {
        self.model.set_active_cursor_position(line, column);
        self.sync_effects();
    }

    fn sync_effects(&mut self) {
        for effect in self.model.drain_effects() {
            if let EditorEffect::Focus(target) = effect {
                self.focus = target;
                if target == FocusTarget::FindQuery {
                    self.deferred_find_query = Some(self.model.find().query.clone());
                }
            }
        }
    }

    fn clear_effects(&mut self) {
        self.model.drain_effects();
    }

    fn cursor(&self) -> Position {
        self.model.active_tab().cursor_position()
    }

    fn text_len(&self) -> usize {
        self.model.active_tab().len_chars()
    }
}

#[derive(Clone, Deserialize)]
struct Fixture {
    cases: Vec<OracleCaseSpec>,
}

#[derive(Clone, Deserialize)]
struct OracleCaseSpec {
    initial_text: String,
    cursor: FixturePosition,
    keys: String,
}

#[derive(Clone, Copy, Deserialize)]
struct FixturePosition {
    line: usize,
    column: usize,
}

struct OracleReplayCase {
    driver: VimDriver,
    keys: String,
}

fn oracle_specs() -> Vec<OracleCaseSpec> {
    serde_json::from_str::<Fixture>(ORACLE_FIXTURE)
        .expect("valid vim oracle fixture")
        .cases
}

fn oracle_batch(specs: &[OracleCaseSpec]) -> Vec<OracleReplayCase> {
    specs
        .iter()
        .map(|case| OracleReplayCase {
            driver: VimDriver::normal_at(&case.initial_text, case.cursor.line, case.cursor.column),
            keys: case.keys.clone(),
        })
        .collect()
}

fn generated_vim_corpus(lines: usize) -> String {
    let mut text = String::new();
    for line in 0..lines {
        text.push_str(&generated_vim_line(line));
        text.push('\n');
    }
    text
}

fn generated_vim_line(line: usize) -> String {
    format!("alpha{line:05} beta gamma_delta word{line:05} punctuation,brackets(foo[bar]) tail end")
}

fn generated_search_corpus(lines: usize) -> String {
    let mut text = String::new();
    for line in 0..lines {
        if line % 8 == 0 {
            text.push_str(&format!(
                "needle alpha{line:05} haystack repeated search target payload tail\n"
            ));
        } else {
            text.push_str(&format!(
                "plain alpha{line:05} haystack repeated search target payload tail\n"
            ));
        }
    }
    text
}

pub fn parse_keys(sequence: &str) -> Vec<(Key, vim::Modifiers)> {
    let mut out = Vec::new();
    let chars: Vec<char> = sequence.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '<' {
            if let Some(end) = chars[index + 1..].iter().position(|ch| *ch == '>') {
                let token: String = chars[index + 1..index + 1 + end].iter().collect();
                out.push(parse_token(&token));
                index += end + 2;
                continue;
            }
        }
        out.push((Key::Character(chars[index].to_string()), vim::Modifiers::default()));
        index += 1;
    }
    out
}

fn parse_token(token: &str) -> (Key, vim::Modifiers) {
    let mut rest = token.to_ascii_lowercase();
    let mut modifiers = vim::Modifiers::default();
    loop {
        if let Some(stripped) = rest.strip_prefix("c-").or_else(|| rest.strip_prefix("ctrl-")) {
            modifiers.control = true;
            rest = stripped.to_string();
        } else if let Some(stripped) = rest
            .strip_prefix("cmd-")
            .or_else(|| rest.strip_prefix("command-"))
            .or_else(|| rest.strip_prefix("super-"))
        {
            modifiers.command = true;
            rest = stripped.to_string();
        } else {
            break;
        }
    }

    let key = match rest.as_str() {
        "esc" | "escape" => Key::Character("\u{1b}".to_string()),
        "enter" | "return" => Key::Named(NamedKey::Enter),
        "tab" => Key::Named(NamedKey::Tab),
        "bs" | "backspace" => Key::Named(NamedKey::Backspace),
        "del" | "delete" => Key::Named(NamedKey::Delete),
        "left" => Key::Named(NamedKey::ArrowLeft),
        "right" => Key::Named(NamedKey::ArrowRight),
        "up" => Key::Named(NamedKey::ArrowUp),
        "down" => Key::Named(NamedKey::ArrowDown),
        "home" => Key::Named(NamedKey::Home),
        "end" => Key::Named(NamedKey::End),
        "pageup" | "pgup" => Key::Named(NamedKey::PageUp),
        "pagedown" | "pgdn" => Key::Named(NamedKey::PageDown),
        "lt" => Key::Character("<".to_string()),
        other if other.chars().count() == 1 => Key::Character(other.to_string()),
        other => panic!("unsupported key token: {other}"),
    };
    (key, modifiers)
}

fn criterion_config() -> Criterion {
    Criterion::default()
        .warm_up_time(Duration::from_millis(500))
        .measurement_time(Duration::from_secs(5))
        .sample_size(20)
}

criterion_group! {
    name = benches;
    config = criterion_config();
    targets =
        bench_vim_motion,
        bench_vim_edit,
        bench_vim_edit_after_mutation,
        bench_vim_visual,
        bench_vim_copy_paste,
        bench_vim_search,
        bench_vim_whole_document,
        bench_vim_oracle
}
criterion_main!(benches);
