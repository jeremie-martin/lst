use gpui::{
    App, Application, Bounds, Context, FocusHandle, Focusable, IntoElement, KeyBinding, Render,
    Window, WindowBounds, WindowOptions, actions, div, prelude::*, px, rgb, size, uniform_list,
};
use ropey::Rope;
use std::{fs, ops::Range, process, time::Instant};

const WINDOW_WIDTH: f32 = 1360.0;
const WINDOW_HEIGHT: f32 = 860.0;
const ROW_HEIGHT: f32 = 22.0;
const GUTTER_WIDTH: f32 = 76.0;
const CODE_FONT_SIZE: f32 = 13.0;
const CORPUS_PATH: &str = "benchmarks/paste-corpus-20k.rs";
const PREMADE_CORPUS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../benchmarks/paste-corpus-20k.rs"
));

actions!(
    gpui_spike,
    [
        LoadCorpus,
        ClearBuffer,
        ReplaceFromClipboard,
        AppendFromClipboard,
        ToggleGutter,
        Quit,
    ]
);

#[derive(Clone, Debug)]
struct OperationStats {
    label: &'static str,
    bytes: usize,
    lines: usize,
    clipboard_read_ms: Option<f64>,
    apply_ms: f64,
}

impl OperationStats {
    fn summary(&self) -> String {
        match self.clipboard_read_ms {
            Some(read_ms) => format!(
                "{} | {} bytes | {} lines | clipboard_read_ms={read_ms:.3} | apply_ms={:.3}",
                self.label, self.bytes, self.lines, self.apply_ms
            ),
            None => format!(
                "{} | {} bytes | {} lines | apply_ms={:.3}",
                self.label, self.bytes, self.lines, self.apply_ms
            ),
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum BenchAction {
    Replace,
    Append,
}

impl BenchAction {
    fn action_name(self) -> &'static str {
        match self {
            Self::Replace => "replace",
            Self::Append => "append",
        }
    }

    fn operation_label(self) -> &'static str {
        match self {
            Self::Replace => "bench_replace",
            Self::Append => "bench_append",
        }
    }
}

#[derive(Clone, Debug)]
struct AutoBench {
    action: BenchAction,
    source: String,
    text: String,
}

struct GpuiPasteLab {
    focus_handle: FocusHandle,
    buffer: Rope,
    show_gutter: bool,
    status: String,
    last_operation: OperationStats,
}

impl GpuiPasteLab {
    fn new(cx: &mut Context<Self>) -> Self {
        let apply_started = Instant::now();
        let buffer = Rope::from_str(PREMADE_CORPUS);
        let last_operation = OperationStats {
            label: "load_corpus_startup",
            bytes: buffer.len_bytes(),
            lines: buffer.len_lines(),
            clipboard_read_ms: None,
            apply_ms: elapsed_ms(apply_started),
        };

        eprintln!("lst_gpui_spike {}", last_operation.summary());

        Self {
            focus_handle: cx.focus_handle(),
            buffer,
            show_gutter: true,
            status: format!("Ready. Loaded {CORPUS_PATH} at startup."),
            last_operation,
        }
    }

    fn button(
        label: &'static str,
        cx: &mut Context<Self>,
        on_click: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> impl IntoElement {
        div()
            .id(label)
            .flex_none()
            .cursor_pointer()
            .px_3()
            .py_1()
            .bg(rgb(0x1F6F78))
            .text_color(rgb(0xFFF9F0))
            .active(|style| style.opacity(0.85))
            .child(label.to_string())
            .on_click(cx.listener(move |this, _, _, cx| on_click(this, cx)))
    }

    fn buffer_bytes(&self) -> usize {
        self.buffer.len_bytes()
    }

    fn buffer_lines(&self) -> usize {
        self.buffer.len_lines()
    }

    fn record_operation(
        &mut self,
        label: &'static str,
        clipboard_read_ms: Option<f64>,
        apply_ms: f64,
    ) {
        self.last_operation = OperationStats {
            label,
            bytes: self.buffer_bytes(),
            lines: self.buffer_lines(),
            clipboard_read_ms,
            apply_ms,
        };
        self.status = self.last_operation.summary();
        eprintln!("lst_gpui_spike {}", self.last_operation.summary());
    }

    fn replace_all_text(
        &mut self,
        label: &'static str,
        text: &str,
        clipboard_read_ms: Option<f64>,
        cx: &mut Context<Self>,
    ) {
        let apply_started = Instant::now();
        self.buffer = Rope::from_str(text);
        self.record_operation(label, clipboard_read_ms, elapsed_ms(apply_started));
        cx.notify();
    }

    fn append_text(
        &mut self,
        label: &'static str,
        text: &str,
        clipboard_read_ms: Option<f64>,
        cx: &mut Context<Self>,
    ) {
        let apply_started = Instant::now();
        let insert_at = self.buffer.len_chars();
        self.buffer.insert(insert_at, text);
        self.record_operation(label, clipboard_read_ms, elapsed_ms(apply_started));
        cx.notify();
    }

    fn line_text(&self, line_ix: usize) -> String {
        let mut line = self.buffer.line(line_ix).to_string();
        while matches!(line.as_bytes().last(), Some(b'\n' | b'\r')) {
            line.pop();
        }

        if line.is_empty() {
            " ".to_string()
        } else {
            line
        }
    }

    fn load_corpus_inner(&mut self, cx: &mut Context<Self>) {
        self.replace_all_text("load_corpus", PREMADE_CORPUS, None, cx);
    }

    fn clear_buffer_inner(&mut self, cx: &mut Context<Self>) {
        self.replace_all_text("clear_buffer", "", None, cx);
    }

    fn replace_from_clipboard_inner(&mut self, cx: &mut Context<Self>) {
        let read_started = Instant::now();
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            self.status = "Clipboard does not currently contain plain text.".to_string();
            eprintln!("lst_gpui_spike clipboard_empty");
            cx.notify();
            return;
        };

        self.replace_all_text(
            "replace_clipboard",
            &text,
            Some(elapsed_ms(read_started)),
            cx,
        );
    }

    fn append_from_clipboard_inner(&mut self, cx: &mut Context<Self>) {
        let read_started = Instant::now();
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            self.status = "Clipboard does not currently contain plain text.".to_string();
            eprintln!("lst_gpui_spike clipboard_empty");
            cx.notify();
            return;
        };

        self.append_text(
            "append_clipboard",
            &text,
            Some(elapsed_ms(read_started)),
            cx,
        );
    }

    fn toggle_gutter_inner(&mut self, cx: &mut Context<Self>) {
        self.show_gutter = !self.show_gutter;
        self.status = if self.show_gutter {
            "Line gutter enabled.".to_string()
        } else {
            "Line gutter disabled.".to_string()
        };
        eprintln!(
            "lst_gpui_spike gutter={}",
            if self.show_gutter { "on" } else { "off" }
        );
        cx.notify();
    }

    fn run_auto_bench(
        &mut self,
        bench: AutoBench,
        window: &mut Window,
        cx: &mut Context<Self>,
        startup_to_action_ms: f64,
        process_started: Instant,
    ) {
        let action_started = Instant::now();

        match bench.action {
            BenchAction::Replace => {
                self.replace_all_text(bench.action.operation_label(), &bench.text, None, cx)
            }
            BenchAction::Append => {
                self.append_text(bench.action.operation_label(), &bench.text, None, cx)
            }
        }

        let operation = self.last_operation.clone();
        let action = bench.action;
        let source = bench.source;

        window.on_next_frame(move |_window, cx| {
            eprintln!(
                "lst_gpui_spike bench action={} source={} startup_to_action_ms={startup_to_action_ms:.3} action_to_next_frame_ms={:.3} total_wall_ms={:.3} final_bytes={} final_lines={} apply_ms={:.3}",
                action.action_name(),
                source,
                elapsed_ms(action_started),
                elapsed_ms(process_started),
                operation.bytes,
                operation.lines,
                operation.apply_ms,
            );
            cx.quit();
        });
    }

    fn load_corpus(&mut self, _: &LoadCorpus, _: &mut Window, cx: &mut Context<Self>) {
        self.load_corpus_inner(cx);
    }

    fn clear_buffer(&mut self, _: &ClearBuffer, _: &mut Window, cx: &mut Context<Self>) {
        self.clear_buffer_inner(cx);
    }

    fn replace_from_clipboard(
        &mut self,
        _: &ReplaceFromClipboard,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replace_from_clipboard_inner(cx);
    }

    fn append_from_clipboard(
        &mut self,
        _: &AppendFromClipboard,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.append_from_clipboard_inner(cx);
    }

    fn toggle_gutter(&mut self, _: &ToggleGutter, _: &mut Window, cx: &mut Context<Self>) {
        self.toggle_gutter_inner(cx);
    }

    fn metrics_line(&self) -> String {
        format!(
            "{} | {} bytes | {} lines | gutter={} | last={}",
            CORPUS_PATH,
            self.buffer_bytes(),
            self.buffer_lines(),
            if self.show_gutter { "on" } else { "off" },
            self.last_operation.summary()
        )
    }

    fn shortcut_line(&self) -> &'static str {
        "Ctrl-R reload corpus | Ctrl-V replace from clipboard | Ctrl-Shift-V append from clipboard | Ctrl-L clear | Ctrl-G toggle gutter | Ctrl-Q quit"
    }
}

impl Focusable for GpuiPasteLab {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for GpuiPasteLab {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::load_corpus))
            .on_action(cx.listener(Self::clear_buffer))
            .on_action(cx.listener(Self::replace_from_clipboard))
            .on_action(cx.listener(Self::append_from_clipboard))
            .on_action(cx.listener(Self::toggle_gutter))
            .size_full()
            .bg(rgb(0xEFE6D7))
            .text_color(rgb(0x231A12))
            .child(
                div()
                    .flex_none()
                    .flex()
                    .justify_between()
                    .items_start()
                    .gap_4()
                    .px_4()
                    .py_3()
                    .bg(rgb(0xF7F1E6))
                    .border_b_1()
                    .border_color(rgb(0xC8BBA7))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_grow()
                            .gap_1()
                            .child(
                                div()
                                    .text_xl()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .child("GPUI Paste Lab"),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(0x685C50))
                                    .child(
                                        "Custom Ropey buffer + GPUI uniform_list. This is a spike for large-file and large-paste behavior, not a full editor.",
                                    ),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(0x8A3B12))
                                    .child(self.metrics_line()),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(0x685C50))
                                    .child(self.shortcut_line()),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(0x685C50))
                                    .child(self.status.clone()),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .gap_2()
                            .child(Self::button("Load 20k corpus", cx, |this, cx| {
                                this.load_corpus_inner(cx)
                            }))
                            .child(Self::button("Replace clipboard", cx, |this, cx| {
                                this.replace_from_clipboard_inner(cx)
                            }))
                            .child(Self::button("Append clipboard", cx, |this, cx| {
                                this.append_from_clipboard_inner(cx)
                            }))
                            .child(Self::button("Clear", cx, |this, cx| {
                                this.clear_buffer_inner(cx)
                            }))
                            .child(Self::button("Toggle gutter", cx, |this, cx| {
                                this.toggle_gutter_inner(cx)
                            })),
                    ),
            )
            .child(
                div().flex_grow().p_3().child(
                    uniform_list(
                        "buffer-lines",
                        self.buffer_lines(),
                        cx.processor(|this, range: Range<usize>, _window, _cx| {
                            range
                                .map(|line_ix| {
                                    let mut row = div()
                                        .id(line_ix)
                                        .h(px(ROW_HEIGHT))
                                        .w_full()
                                        .flex()
                                        .items_center()
                                        .px_2()
                                        .bg(if line_ix % 2 == 0 {
                                            rgb(0xFFFDF8)
                                        } else {
                                            rgb(0xF6EFE4)
                                        });

                                    if this.show_gutter {
                                        row = row.child(
                                            div()
                                                .w(px(GUTTER_WIDTH))
                                                .flex_none()
                                                .pr_3()
                                                .font_family(".ZedMono")
                                                .text_size(px(CODE_FONT_SIZE))
                                                .text_color(rgb(0x8D7F70))
                                                .text_right()
                                                .child(format!("{:>6}", line_ix + 1)),
                                        );
                                    }

                                    row.child(
                                        div()
                                            .flex_grow()
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .font_family(".ZedMono")
                                            .text_size(px(CODE_FONT_SIZE))
                                            .text_color(rgb(0x201A16))
                                            .child(this.line_text(line_ix)),
                                    )
                                })
                                .collect()
                        }),
                    )
                    .h_full()
                    .w_full()
                    .border_1()
                    .border_color(rgb(0xC8BBA7)),
                ),
            )
    }
}

fn elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

fn usage() -> &'static str {
    "Usage:
  cargo run
  cargo run -- --bench-replace-corpus
  cargo run -- --bench-append-corpus
  cargo run -- --bench-replace-file /path/to/file.rs
  cargo run -- --bench-append-file /path/to/file.rs"
}

fn parse_auto_bench() -> Option<AutoBench> {
    let mut args = std::env::args().skip(1);
    let Some(flag) = args.next() else {
        return None;
    };

    if flag == "--help" || flag == "-h" {
        println!("{}", usage());
        process::exit(0);
    }

    let finish_file_arg = |action: BenchAction, path: String, trailing: Option<String>| {
        if let Some(extra) = trailing {
            eprintln!("unexpected extra argument: {extra}\n\n{}", usage());
            process::exit(2);
        }

        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(err) => {
                eprintln!("failed to read benchmark file {path}: {err}");
                process::exit(2);
            }
        };

        AutoBench {
            action,
            source: path,
            text,
        }
    };

    match flag.as_str() {
        "--bench-replace-corpus" => {
            if let Some(extra) = args.next() {
                eprintln!("unexpected extra argument: {extra}\n\n{}", usage());
                process::exit(2);
            }

            Some(AutoBench {
                action: BenchAction::Replace,
                source: CORPUS_PATH.to_string(),
                text: PREMADE_CORPUS.to_string(),
            })
        }
        "--bench-append-corpus" => {
            if let Some(extra) = args.next() {
                eprintln!("unexpected extra argument: {extra}\n\n{}", usage());
                process::exit(2);
            }

            Some(AutoBench {
                action: BenchAction::Append,
                source: CORPUS_PATH.to_string(),
                text: PREMADE_CORPUS.to_string(),
            })
        }
        "--bench-replace-file" => {
            let Some(path) = args.next() else {
                eprintln!("missing file path for --bench-replace-file\n\n{}", usage());
                process::exit(2);
            };
            Some(finish_file_arg(BenchAction::Replace, path, args.next()))
        }
        "--bench-append-file" => {
            let Some(path) = args.next() else {
                eprintln!("missing file path for --bench-append-file\n\n{}", usage());
                process::exit(2);
            };
            Some(finish_file_arg(BenchAction::Append, path, args.next()))
        }
        _ => {
            eprintln!("unknown argument: {flag}\n\n{}", usage());
            process::exit(2);
        }
    }
}

fn main() {
    let auto_bench = parse_auto_bench();
    let process_started = Instant::now();
    let has_graphical_env =
        std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some();

    if !has_graphical_env {
        eprintln!(
            "lst_gpui_spike requires a graphical session. Run it from a real X11 or Wayland desktop."
        );
        process::exit(1);
    }

    Application::new().run(move |cx: &mut App| {
        cx.bind_keys([
            KeyBinding::new("ctrl-r", LoadCorpus, None),
            KeyBinding::new("cmd-r", LoadCorpus, None),
            KeyBinding::new("ctrl-l", ClearBuffer, None),
            KeyBinding::new("cmd-l", ClearBuffer, None),
            KeyBinding::new("ctrl-v", ReplaceFromClipboard, None),
            KeyBinding::new("cmd-v", ReplaceFromClipboard, None),
            KeyBinding::new("ctrl-shift-v", AppendFromClipboard, None),
            KeyBinding::new("cmd-shift-v", AppendFromClipboard, None),
            KeyBinding::new("ctrl-g", ToggleGutter, None),
            KeyBinding::new("cmd-g", ToggleGutter, None),
            KeyBinding::new("ctrl-q", Quit, None),
            KeyBinding::new("cmd-q", Quit, None),
        ]);
        cx.on_action(|_: &Quit, cx| cx.quit());

        let bounds = Bounds::centered(None, size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), cx);
        let window = match cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(GpuiPasteLab::new),
        ) {
            Ok(window) => window,
            Err(err) => {
                eprintln!(
                    "lst_gpui_spike failed to open a GPUI window: {err}. On this host, Xvfb is not sufficient because GPUI surface creation requires a real presentation backend."
                );
                process::exit(1);
            }
        };

        let view = window
            .update(cx, |view, window, cx| {
                window.focus(&view.focus_handle(cx));
                cx.activate(true);
                cx.entity()
            })
            .unwrap();

        if let Some(bench) = auto_bench.clone() {
            window
                .update(cx, move |_view, window, _cx| {
                    let view = view.clone();
                    let bench = bench.clone();
                    window.on_next_frame(move |window, cx| {
                        let startup_to_action_ms = elapsed_ms(process_started);
                        let _ = view.update(cx, |view, cx| {
                            view.run_auto_bench(
                                bench,
                                window,
                                cx,
                                startup_to_action_ms,
                                process_started,
                            );
                        });
                    });
                })
                .unwrap();
        }
    });
}
