use std::{hint::black_box, time::Duration};

use criterion::{criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput};
use lst_editor::EditorCommand;

mod support;

use support::{plain_corpus, position_of, rust_corpus, typing_payload, DocumentSize, ModelDriver, WRAP_COLUMNS};

const TYPING_CHARS: usize = 320;
const PAGE_STEPS: usize = 128;
const FIND_NEXT_STEPS: usize = 512;
const MULTI_CURSOR_PASTE: &str = " // tagged";
const FIND_QUERY: &str = "fn ";
const FIND_REPLACEMENT: &str = "fn";
const OCCURRENCE_QUERY: &str = "selected";
const PASTE_REPETITIONS: usize = 3;

fn bench_editor_open(c: &mut Criterion) {
    let mut group = c.benchmark_group("editor_open");

    for size in DocumentSize::all() {
        let corpus = rust_corpus(size);
        group.throughput(Throughput::Bytes(corpus.bytes()));
        group.bench_with_input(
            BenchmarkId::new("construct_model", size.label()),
            &corpus,
            |b, corpus| {
                b.iter(|| {
                    let driver = ModelDriver::new(corpus.name(), black_box(corpus.text()));
                    black_box((driver.text_len(), driver.model.active_tab().line_count()));
                });
            },
        );
    }

    group.finish();
}

fn bench_editor_typing(c: &mut Criterion) {
    let payload = typing_payload(TYPING_CHARS);
    let mut group = c.benchmark_group("editor_typing");

    for size in DocumentSize::all() {
        let corpus = rust_corpus(size);
        group.throughput(Throughput::Elements(TYPING_CHARS as u64));
        group.bench_with_input(BenchmarkId::new("type_chars", size.label()), &corpus, |b, corpus| {
            b.iter_batched(
                || ModelDriver::new(corpus.name(), corpus.text()),
                |mut driver| {
                    for ch in payload.chars() {
                        driver.insert_text_from_input(ch.encode_utf8(&mut [0; 4]));
                    }
                    black_box(driver.text_len());
                },
                BatchSize::LargeInput,
            );
        });
    }

    group.finish();
}

fn bench_editor_clipboard(c: &mut Criterion) {
    let mut group = c.benchmark_group("editor_clipboard");

    for size in DocumentSize::all() {
        let corpus = rust_corpus(size);

        group.throughput(Throughput::Bytes(corpus.bytes()));
        group.bench_with_input(BenchmarkId::new("select_all", size.label()), &corpus, |b, corpus| {
            b.iter_batched(
                || ModelDriver::new(corpus.name(), corpus.text()),
                |mut driver| {
                    driver.execute(EditorCommand::SelectAll);
                    black_box((driver.selection_count(), driver.primary_len()));
                },
                BatchSize::LargeInput,
            );
        });

        group.throughput(Throughput::Bytes(corpus.bytes()));
        group.bench_with_input(
            BenchmarkId::new("copy_selected_all", size.label()),
            &corpus,
            |b, corpus| {
                b.iter_batched(
                    || {
                        let mut driver = ModelDriver::new(corpus.name(), corpus.text());
                        driver.execute(EditorCommand::SelectAll);
                        driver.clear_transfer_buffers();
                        driver
                    },
                    |mut driver| {
                        driver.execute(EditorCommand::CopySelection);
                        black_box((driver.clipboard_len(), driver.primary_len()));
                    },
                    BatchSize::LargeInput,
                );
            },
        );

        group.throughput(Throughput::Bytes(corpus.bytes()));
        group.bench_with_input(
            BenchmarkId::new("paste_into_empty", size.label()),
            &corpus,
            |b, corpus| {
                b.iter_batched(
                    || {
                        let mut driver = ModelDriver::new(corpus.name(), "");
                        driver.set_clipboard(corpus.text().to_string());
                        driver
                    },
                    |mut driver| {
                        driver.execute(EditorCommand::RequestPaste);
                        black_box(driver.text_len());
                    },
                    BatchSize::LargeInput,
                );
            },
        );

        group.throughput(Throughput::Bytes(corpus.bytes() * PASTE_REPETITIONS as u64));
        group.bench_with_input(
            BenchmarkId::new("paste_three_times_into_empty", size.label()),
            &corpus,
            |b, corpus| {
                b.iter_batched(
                    || {
                        let mut driver = ModelDriver::new(corpus.name(), "");
                        driver.set_clipboard(corpus.text().to_string());
                        driver
                    },
                    |mut driver| {
                        for _ in 0..PASTE_REPETITIONS {
                            driver.execute(EditorCommand::RequestPaste);
                        }
                        black_box(driver.text_len());
                    },
                    BatchSize::LargeInput,
                );
            },
        );

        group.throughput(Throughput::Bytes(corpus.bytes()));
        group.bench_with_input(
            BenchmarkId::new("select_copy_paste_two_tabs", size.label()),
            &corpus,
            |b, corpus| {
                b.iter_batched(
                    || ModelDriver::with_two_tabs(corpus.name(), corpus.text(), "target.rs", ""),
                    |mut driver| {
                        driver.execute(EditorCommand::SelectAll);
                        driver.execute(EditorCommand::CopySelection);
                        driver.execute(EditorCommand::NextTab);
                        driver.execute(EditorCommand::RequestPaste);
                        black_box((driver.text_len(), driver.clipboard_len()));
                    },
                    BatchSize::LargeInput,
                );
            },
        );

        group.throughput(Throughput::Bytes(corpus.bytes() * PASTE_REPETITIONS as u64));
        group.bench_with_input(
            BenchmarkId::new("select_copy_paste_three_times_same_tab", size.label()),
            &corpus,
            |b, corpus| {
                b.iter_batched(
                    || ModelDriver::new(corpus.name(), corpus.text()),
                    |mut driver| {
                        driver.execute(EditorCommand::SelectAll);
                        driver.execute(EditorCommand::CopySelection);
                        for _ in 0..PASTE_REPETITIONS {
                            driver.execute(EditorCommand::RequestPaste);
                        }
                        black_box((driver.text_len(), driver.clipboard_len()));
                    },
                    BatchSize::LargeInput,
                );
            },
        );

        group.throughput(Throughput::Bytes(corpus.bytes() * PASTE_REPETITIONS as u64));
        group.bench_with_input(
            BenchmarkId::new("select_copy_paste_three_times_two_tabs", size.label()),
            &corpus,
            |b, corpus| {
                b.iter_batched(
                    || ModelDriver::with_two_tabs(corpus.name(), corpus.text(), "target.rs", ""),
                    |mut driver| {
                        driver.execute(EditorCommand::SelectAll);
                        driver.execute(EditorCommand::CopySelection);
                        driver.execute(EditorCommand::NextTab);
                        for _ in 0..PASTE_REPETITIONS {
                            driver.execute(EditorCommand::RequestPaste);
                        }
                        black_box((driver.text_len(), driver.clipboard_len()));
                    },
                    BatchSize::LargeInput,
                );
            },
        );
    }

    group.finish();
}

fn bench_editor_navigation(c: &mut Criterion) {
    let mut group = c.benchmark_group("editor_navigation");

    for size in DocumentSize::all() {
        let corpus = rust_corpus(size);
        group.throughput(Throughput::Elements((PAGE_STEPS * 2) as u64));
        group.bench_with_input(BenchmarkId::new("page_wrapped", size.label()), &corpus, |b, corpus| {
            b.iter_batched(
                || {
                    let mut driver = ModelDriver::new(corpus.name(), corpus.text());
                    driver.configure_viewport();
                    driver
                },
                |mut driver| {
                    for _ in 0..PAGE_STEPS {
                        driver.execute(EditorCommand::Page(true, false, WRAP_COLUMNS));
                    }
                    for _ in 0..PAGE_STEPS {
                        driver.execute(EditorCommand::Page(false, false, WRAP_COLUMNS));
                    }
                    black_box(driver.cursor());
                },
                BatchSize::LargeInput,
            );
        });

        let corpus = plain_corpus(size);
        group.throughput(Throughput::Elements((PAGE_STEPS * 2) as u64));
        group.bench_with_input(
            BenchmarkId::new("page_unwrapped", size.label()),
            &corpus,
            |b, corpus| {
                b.iter_batched(
                    || {
                        let mut driver = ModelDriver::new(corpus.name(), corpus.text());
                        driver.configure_viewport();
                        driver.execute(EditorCommand::ToggleWrap);
                        driver
                    },
                    |mut driver| {
                        for _ in 0..PAGE_STEPS {
                            driver.execute(EditorCommand::Page(true, false, WRAP_COLUMNS));
                        }
                        for _ in 0..PAGE_STEPS {
                            driver.execute(EditorCommand::Page(false, false, WRAP_COLUMNS));
                        }
                        black_box(driver.cursor());
                    },
                    BatchSize::LargeInput,
                );
            },
        );
    }

    group.finish();
}

fn bench_editor_find(c: &mut Criterion) {
    let mut group = c.benchmark_group("editor_find");

    for size in DocumentSize::all() {
        let corpus = rust_corpus(size);

        group.throughput(Throughput::Bytes(corpus.bytes()));
        group.bench_with_input(BenchmarkId::new("submit_query", size.label()), &corpus, |b, corpus| {
            b.iter_batched(
                || ModelDriver::new(corpus.name(), corpus.text()),
                |mut driver| {
                    driver.model.update_find_query_and_activate(FIND_QUERY.to_string());
                    driver.sync_effects();
                    black_box(driver.find_match_count());
                },
                BatchSize::LargeInput,
            );
        });

        group.throughput(Throughput::Elements(FIND_NEXT_STEPS as u64));
        group.bench_with_input(BenchmarkId::new("find_next", size.label()), &corpus, |b, corpus| {
            b.iter_batched(
                || {
                    let mut driver = ModelDriver::new(corpus.name(), corpus.text());
                    driver.model.update_find_query_and_activate(FIND_QUERY.to_string());
                    driver.sync_effects();
                    driver
                },
                |mut driver| {
                    for _ in 0..FIND_NEXT_STEPS {
                        driver.execute(EditorCommand::FindNext);
                    }
                    black_box(driver.cursor());
                },
                BatchSize::LargeInput,
            );
        });

        group.throughput(Throughput::Bytes(corpus.bytes()));
        group.bench_with_input(
            BenchmarkId::new("select_all_find_matches", size.label()),
            &corpus,
            |b, corpus| {
                b.iter_batched(
                    || {
                        let mut driver = ModelDriver::new(corpus.name(), corpus.text());
                        driver.model.update_find_query_and_activate(FIND_QUERY.to_string());
                        driver.sync_effects();
                        driver
                    },
                    |mut driver| {
                        driver.execute(EditorCommand::SelectAllFindMatches);
                        black_box(driver.selection_count());
                    },
                    BatchSize::LargeInput,
                );
            },
        );

        group.throughput(Throughput::Bytes(corpus.bytes()));
        group.bench_with_input(
            BenchmarkId::new("replace_all_matches", size.label()),
            &corpus,
            |b, corpus| {
                b.iter_batched(
                    || {
                        let mut driver = ModelDriver::new(corpus.name(), corpus.text());
                        driver.model.update_find_replacement(FIND_REPLACEMENT.to_string());
                        driver.model.update_find_query_and_activate(FIND_QUERY.to_string());
                        driver.sync_effects();
                        driver
                    },
                    |mut driver| {
                        driver.execute(EditorCommand::ReplaceAllMatches);
                        black_box((driver.text_len(), driver.find_match_count()));
                    },
                    BatchSize::LargeInput,
                );
            },
        );
    }

    group.finish();
}

fn bench_editor_multi_cursor(c: &mut Criterion) {
    let mut group = c.benchmark_group("editor_multi_cursor");

    for size in DocumentSize::all() {
        let corpus = rust_corpus(size);
        let occurrence_position =
            position_of(corpus.text(), OCCURRENCE_QUERY).expect("generated Rust corpus contains occurrence query");

        group.throughput(Throughput::Bytes(corpus.bytes()));
        group.bench_with_input(
            BenchmarkId::new("select_all_occurrences", size.label()),
            &corpus,
            |b, corpus| {
                b.iter_batched(
                    || {
                        let mut driver = ModelDriver::new(corpus.name(), corpus.text());
                        driver.set_cursor(occurrence_position);
                        driver
                    },
                    |mut driver| {
                        driver.execute(EditorCommand::SelectAllOccurrences);
                        black_box(driver.selection_count());
                    },
                    BatchSize::LargeInput,
                );
            },
        );

        group.throughput(Throughput::Elements(size.cursor_lines() as u64));
        group.bench_with_input(
            BenchmarkId::new("paste_across_line_end_cursors", size.label()),
            &corpus,
            |b, corpus| {
                b.iter_batched(
                    || {
                        let mut driver = ModelDriver::new(corpus.name(), corpus.text());
                        driver.select_first_lines(size.cursor_lines());
                        driver.execute(EditorCommand::AddCursorsToSelectedLineEnds);
                        driver
                    },
                    |mut driver| {
                        driver.paste_text(MULTI_CURSOR_PASTE);
                        black_box((driver.selection_count(), driver.text_len()));
                    },
                    BatchSize::LargeInput,
                );
            },
        );
    }

    group.finish();
}

fn bench_editor_line_edit(c: &mut Criterion) {
    let mut group = c.benchmark_group("editor_line_edit");

    for size in DocumentSize::all() {
        let corpus = rust_corpus(size);
        group.throughput(Throughput::Bytes(corpus.bytes()));
        group.bench_with_input(
            BenchmarkId::new("indent_whole_document", size.label()),
            &corpus,
            |b, corpus| {
                b.iter_batched(
                    || ModelDriver::new(corpus.name(), corpus.text()),
                    |mut driver| {
                        driver.execute(EditorCommand::SelectAll);
                        driver.execute(EditorCommand::InsertTab);
                        black_box(driver.text_len());
                    },
                    BatchSize::LargeInput,
                );
            },
        );
    }

    group.finish();
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
        bench_editor_open,
        bench_editor_typing,
        bench_editor_clipboard,
        bench_editor_navigation,
        bench_editor_find,
        bench_editor_multi_cursor,
        bench_editor_line_edit
}
criterion_main!(benches);
