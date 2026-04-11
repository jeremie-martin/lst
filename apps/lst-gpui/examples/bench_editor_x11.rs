use std::{
    collections::HashMap,
    env,
    error::Error,
    fs, io,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use x11rb::connection::Connection as _;
use x11rb::errors::ReplyError;
use x11rb::protocol::damage::{self, ConnectionExt as _};
use x11rb::protocol::xkb::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{self, AtomEnum, ConnectionExt as _, MapState};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::protocol::ErrorKind;
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;
use x11rb::NONE;

const LARGE_CORPUS_REL: &str = "../../benchmarks/paste-corpus-20k.rs";
const MEDIUM_CORPUS_REL: &str = "../../benchmarks/editing-corpus.rs";
const DEFAULT_REPETITIONS: usize = 7;
const DEFAULT_PRIMING_RUNS: usize = 1;
const INTER_RUN_SLEEP_MS: u64 = 1_000;
const QUIET_MS: u64 = 75;
const WINDOW_DISCOVERY_TIMEOUT_MS: u64 = 10_000;
const TRACE_TIMEOUT_MS: u64 = 30_000;
const POINTER_SETTLE_MS: u64 = 50;
const CLIPBOARD_TIMEOUT_MS: u64 = 20_000;
const FILE_STABLE_MS: u64 = 200;
const FILE_STABLE_TIMEOUT_MS: u64 = 20_000;
const SAVE_RETRY_MS: u64 = 100;
const SCROLL_WHEEL_COUNT: usize = 240;
const SCROLL_HALF_MS: u64 = 1_500;
const TYPING_CHARS: usize = 320;
const SEARCH_QUERY: &str = "fn ";
const FIND_QUERY_CLICK_X_FRACTION: f32 = 0.82;
const FIND_QUERY_CLICK_Y_FRACTION: f32 = 0.10;
const BUTTON_LEFT: u8 = 1;
const BUTTON_WHEEL_UP: u8 = 4;
const BUTTON_WHEEL_DOWN: u8 = 5;
const KEYSYM_CONTROL_L: u32 = 0xffe3;
const KEYSYM_TAB: u32 = 0xff09;
const KEYSYM_SPACE: u32 = 0x20;

fn main() -> Result<(), Box<dyn Error>> {
    if env::args()
        .skip(1)
        .any(|arg| matches!(arg.as_str(), "-h" | "--help"))
    {
        print_usage();
        return Ok(());
    }

    let args = parse_args_from(env::args().skip(1)).unwrap_or_else(|error| {
        eprintln!("{error}");
        print_usage();
        std::process::exit(2);
    });

    let session_env = resolve_session_env()?;
    apply_session_env(&session_env);

    let editor = editor_path()?;
    let (conn, screen_num) = x11rb::connect(Some(session_env.display.as_str()))?;
    conn.damage_query_version(1, 1)?.reply()?;
    conn.xtest_get_version(2, 2)?.reply()?;
    conn.xkb_use_extension(1, 0)?.reply()?;

    let root = conn.setup().roots[screen_num].root;
    let atoms = Atoms::intern(&conn)?;
    let keycodes = Keycodes::resolve(&conn)?;
    let bench = Bench {
        conn,
        root,
        atoms,
        keycodes,
        ticks_per_second: clock_ticks_per_second()?,
        session_env,
        editor,
    };

    for scenario in args.scenario.measured_cases() {
        bench.run_scenario(scenario, &args)?;
    }

    Ok(())
}

fn print_usage() {
    eprintln!(
        "Usage:
  cargo run --release -p lst-gpui --example bench_editor_x11 -- [options]

Options:
  --scenario <name>     all, large-paste, typing-medium, typing-large,
                        scroll-highlighted, scroll-plain, open-large, search-large
                        (default: all)
  --repetitions <n>     measured repetitions after priming (default: 7)
  --priming <n>         unreported warm-up runs (default: 1)
  --keep-temp-on-failure
                        leave benchmark temp files in /tmp when a scenario fails"
    );
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Scenario {
    All,
    LargePaste,
    TypingMedium,
    TypingLarge,
    ScrollHighlighted,
    ScrollPlain,
    OpenLarge,
    SearchLarge,
}

impl Scenario {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "all" => Ok(Self::All),
            "large-paste" => Ok(Self::LargePaste),
            "typing-medium" => Ok(Self::TypingMedium),
            "typing-large" => Ok(Self::TypingLarge),
            "scroll-highlighted" => Ok(Self::ScrollHighlighted),
            "scroll-plain" => Ok(Self::ScrollPlain),
            "open-large" => Ok(Self::OpenLarge),
            "search-large" => Ok(Self::SearchLarge),
            _ => Err(format!("unknown scenario: {value}")),
        }
    }

    fn measured_cases(self) -> Vec<Self> {
        match self {
            Self::All => vec![
                Self::LargePaste,
                Self::TypingMedium,
                Self::TypingLarge,
                Self::ScrollHighlighted,
                Self::ScrollPlain,
                Self::OpenLarge,
                Self::SearchLarge,
            ],
            scenario => vec![scenario],
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::LargePaste => "large-paste",
            Self::TypingMedium => "typing-medium",
            Self::TypingLarge => "typing-large",
            Self::ScrollHighlighted => "scroll-highlighted",
            Self::ScrollPlain => "scroll-plain",
            Self::OpenLarge => "open-large",
            Self::SearchLarge => "search-large",
        }
    }

    fn primary_metric(self) -> &'static str {
        match self {
            Self::All => "primary_value",
            Self::LargePaste => "paste_complete_ms",
            Self::TypingMedium | Self::TypingLarge => "typing_ms_per_char",
            Self::ScrollHighlighted | Self::ScrollPlain => "scroll_overrun_ms",
            Self::OpenLarge => "open_to_quiet_ms",
            Self::SearchLarge => "search_reindex_ms",
        }
    }

    fn corpus_kind(self) -> CorpusKind {
        match self {
            Self::TypingMedium => CorpusKind::MediumRust,
            Self::ScrollPlain => CorpusKind::LargePlain,
            Self::All
            | Self::LargePaste
            | Self::TypingLarge
            | Self::ScrollHighlighted
            | Self::OpenLarge
            | Self::SearchLarge => CorpusKind::LargeRust,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CorpusKind {
    MediumRust,
    LargeRust,
    LargePlain,
}

impl CorpusKind {
    fn source_rel(self) -> &'static str {
        match self {
            Self::MediumRust => MEDIUM_CORPUS_REL,
            Self::LargeRust | Self::LargePlain => LARGE_CORPUS_REL,
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Self::MediumRust | Self::LargeRust => "rs",
            Self::LargePlain => "txt",
        }
    }

    fn highlight_label(self) -> &'static str {
        match self {
            Self::MediumRust | Self::LargeRust => "rust-tree-sitter",
            Self::LargePlain => "plain",
        }
    }
}

#[derive(Debug)]
struct Args {
    scenario: Scenario,
    repetitions: usize,
    priming_runs: usize,
    keep_temp_on_failure: bool,
}

fn parse_args_from<I, S>(raw_args: I) -> Result<Args, String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut args = Args {
        scenario: Scenario::All,
        repetitions: DEFAULT_REPETITIONS,
        priming_runs: DEFAULT_PRIMING_RUNS,
        keep_temp_on_failure: false,
    };
    let mut raw_args = raw_args.into_iter().map(Into::into);

    while let Some(arg) = raw_args.next() {
        match arg.as_str() {
            "--scenario" => {
                let value = raw_args
                    .next()
                    .ok_or_else(|| "--scenario requires a value".to_string())?;
                args.scenario = Scenario::parse(&value)?;
            }
            "--repetitions" => {
                let value = raw_args
                    .next()
                    .ok_or_else(|| "--repetitions requires a value".to_string())?;
                args.repetitions = parse_positive_usize("--repetitions", &value)?;
            }
            "--priming" => {
                let value = raw_args
                    .next()
                    .ok_or_else(|| "--priming requires a value".to_string())?;
                args.priming_runs = value
                    .parse::<usize>()
                    .map_err(|_| format!("invalid --priming value: {value}"))?;
            }
            "--keep-temp-on-failure" => args.keep_temp_on_failure = true,
            unknown => return Err(format!("unknown argument: {unknown}")),
        }
    }

    Ok(args)
}

fn parse_positive_usize(name: &str, value: &str) -> Result<usize, String> {
    let parsed = value
        .parse::<usize>()
        .map_err(|_| format!("invalid {name} value: {value}"))?;
    if parsed == 0 {
        return Err(format!("{name} must be greater than zero"));
    }
    Ok(parsed)
}

struct Bench {
    conn: RustConnection,
    root: xproto::Window,
    atoms: Atoms,
    keycodes: Keycodes,
    ticks_per_second: u64,
    session_env: SessionEnv,
    editor: PathBuf,
}

impl Bench {
    fn run_scenario(&self, scenario: Scenario, args: &Args) -> Result<(), Box<dyn Error>> {
        let corpus = Corpus::load(scenario.corpus_kind())?;
        let mut runs = Vec::with_capacity(args.repetitions);
        let mut expected_window = None;
        let total_runs = args.priming_runs + args.repetitions;

        for run_index in 0..total_runs {
            let measured = run_index >= args.priming_runs;
            let metrics = match scenario {
                Scenario::All => unreachable!("all is expanded before execution"),
                Scenario::LargePaste => {
                    self.run_large_paste(scenario, &corpus, run_index, args.keep_temp_on_failure)?
                }
                Scenario::TypingMedium | Scenario::TypingLarge => {
                    self.run_typing(scenario, &corpus, run_index, args.keep_temp_on_failure)?
                }
                Scenario::ScrollHighlighted | Scenario::ScrollPlain => {
                    self.run_scroll(scenario, &corpus, run_index, args.keep_temp_on_failure)?
                }
                Scenario::OpenLarge => {
                    self.run_open_large(scenario, &corpus, run_index, args.keep_temp_on_failure)?
                }
                Scenario::SearchLarge => {
                    self.run_search(scenario, &corpus, run_index, args.keep_temp_on_failure)?
                }
            };

            if let Some((width, height)) = expected_window {
                if metrics.window_size != (width, height) {
                    return Err(io::Error::other(format!(
                        "{} window size changed: expected {width}x{height}, got {}x{}",
                        scenario.as_str(),
                        metrics.window_size.0,
                        metrics.window_size.1
                    ))
                    .into());
                }
            } else {
                expected_window = Some(metrics.window_size);
            }

            if measured {
                runs.push(metrics);
            }

            if run_index + 1 < total_runs {
                thread::sleep(Duration::from_millis(INTER_RUN_SLEEP_MS));
            }
        }

        emit_summary(
            scenario,
            args,
            &self.session_env,
            &corpus,
            expected_window,
            &runs,
        )
    }

    fn run_large_paste(
        &self,
        scenario: Scenario,
        corpus: &Corpus,
        run_index: usize,
        keep_temp_on_failure: bool,
    ) -> Result<RunMetrics, Box<dyn Error>> {
        let source_path = temp_path(scenario, run_index, "source", "rs");
        let target_path = temp_path(scenario, run_index, "target", "rs");
        let trace_path = temp_path(scenario, run_index, "trace", "log");
        fs::write(&source_path, &corpus.text)?;
        fs::write(&target_path, "")?;

        let title = bench_title(scenario, run_index);
        let files = [source_path.as_path(), target_path.as_path()];
        let mut child = self.spawn_editor(&files, &title, Some(&trace_path))?;
        let pid = child.id();

        let result = (|| {
            let startup_started = Instant::now();
            let window = find_window(
                &self.conn,
                self.root,
                &self.atoms,
                pid,
                &title,
                &mut child,
                Duration::from_millis(WINDOW_DISCOVERY_TIMEOUT_MS),
            )?;
            let damage = damage::DamageWrapper::create(
                &self.conn,
                window.id,
                damage::ReportLevel::NON_EMPTY,
            )?;
            self.conn.flush()?;
            let _ = wait_for_damage_quiet(
                &self.conn,
                damage.damage(),
                window.id,
                &mut child,
                Duration::from_millis(QUIET_MS),
                Duration::from_millis(TRACE_TIMEOUT_MS),
            )?;
            let startup_ms = elapsed_ms(startup_started);

            focus_window(&self.conn, self.root, &window)?;
            let _ = wait_for_damage_quiet(
                &self.conn,
                damage.damage(),
                window.id,
                &mut child,
                Duration::from_millis(QUIET_MS),
                Duration::from_millis(TRACE_TIMEOUT_MS),
            )?;

            let before = proc_sample(pid)?;
            let trace_started = Instant::now();

            let select_all_started = Instant::now();
            inject_ctrl_chord(
                &self.conn,
                self.root,
                self.keycodes.control_l,
                self.keycodes.a,
            )?;
            let mut damage_events = wait_for_damage_quiet(
                &self.conn,
                damage.damage(),
                window.id,
                &mut child,
                Duration::from_millis(QUIET_MS),
                Duration::from_millis(TRACE_TIMEOUT_MS),
            )?;
            let select_all_ms = elapsed_ms(select_all_started);

            let copy_started = Instant::now();
            inject_ctrl_chord(
                &self.conn,
                self.root,
                self.keycodes.control_l,
                self.keycodes.c,
            )?;
            wait_for_clipboard_bytes(corpus.bytes, Duration::from_millis(CLIPBOARD_TIMEOUT_MS))?;
            let copy_clipboard_ms = elapsed_ms(copy_started);

            let tab_started = Instant::now();
            inject_ctrl_chord(
                &self.conn,
                self.root,
                self.keycodes.control_l,
                self.keycodes.tab,
            )?;
            damage_events += wait_for_damage_quiet(
                &self.conn,
                damage.damage(),
                window.id,
                &mut child,
                Duration::from_millis(QUIET_MS),
                Duration::from_millis(TRACE_TIMEOUT_MS),
            )?;
            let tab_switch_ms = elapsed_ms(tab_started);

            let paste_started = Instant::now();
            inject_ctrl_chord(
                &self.conn,
                self.root,
                self.keycodes.control_l,
                self.keycodes.v,
            )?;
            let (paste_damage_events, save_retry_count, final_stats) =
                wait_for_file_text_with_save_retry(
                    &self.conn,
                    damage.damage(),
                    window.id,
                    &mut child,
                    self.root,
                    &self.keycodes,
                    &target_path,
                    &corpus.text,
                    Duration::from_millis(FILE_STABLE_MS),
                    Duration::from_millis(SAVE_RETRY_MS),
                    Duration::from_millis(FILE_STABLE_TIMEOUT_MS),
                )?;
            let paste_complete_ms = elapsed_ms(paste_started);
            damage_events += paste_damage_events;

            let trace_wall_ms = elapsed_ms(trace_started);
            let after = proc_sample(pid)?;
            let trace = read_editor_trace(&trace_path)?;
            let mut metrics = RunMetrics::new(window.width, window.height);
            metrics.set("startup_ms", startup_ms);
            metrics.set("select_all_ms", select_all_ms);
            metrics.set("copy_clipboard_ms", copy_clipboard_ms);
            metrics.set("tab_switch_ms", tab_switch_ms);
            metrics.set("paste_complete_ms", paste_complete_ms);
            metrics.set("trace_wall_ms", trace_wall_ms);
            metrics.set("damage_events", damage_events as f64);
            metrics.set("paste_damage_events", paste_damage_events as f64);
            metrics.set("save_retry_count", save_retry_count as f64);
            metrics.set("final_file_bytes", final_stats.bytes as f64);
            metrics.set("final_file_lines", final_stats.lines as f64);
            add_process_metrics(&mut metrics, &before, &after, self.ticks_per_second);
            add_trace_last(
                &mut metrics,
                &trace,
                "paste_clipboard_apply_ms",
                "paste_apply_ms",
            );
            add_trace_last(
                &mut metrics,
                &trace,
                "paste_clipboard_clipboard_read_ms",
                "paste_clipboard_read_ms",
            );
            add_trace_last(&mut metrics, &trace, "paste_clipboard_bytes", "paste_bytes");
            add_trace_last(&mut metrics, &trace, "paste_clipboard_lines", "paste_lines");
            Ok(metrics)
        })();

        let terminate_result = terminate_child(&mut child);
        cleanup_paths_if(
            [source_path, target_path, trace_path],
            result.is_ok() || !keep_temp_on_failure,
        );
        terminate_result?;
        result
    }

    fn run_typing(
        &self,
        scenario: Scenario,
        corpus: &Corpus,
        run_index: usize,
        keep_temp_on_failure: bool,
    ) -> Result<RunMetrics, Box<dyn Error>> {
        let file_path = temp_path(scenario, run_index, "file", corpus.extension);
        let trace_path = temp_path(scenario, run_index, "trace", "log");
        fs::write(&file_path, &corpus.text)?;

        let payload = typing_payload(TYPING_CHARS);
        let expected_text = format!("{payload}{}", corpus.text);
        let title = bench_title(scenario, run_index);
        let files = [file_path.as_path()];
        let mut child = self.spawn_editor(&files, &title, Some(&trace_path))?;
        let pid = child.id();

        let result = (|| {
            let startup_started = Instant::now();
            let window = find_window(
                &self.conn,
                self.root,
                &self.atoms,
                pid,
                &title,
                &mut child,
                Duration::from_millis(WINDOW_DISCOVERY_TIMEOUT_MS),
            )?;
            let damage = damage::DamageWrapper::create(
                &self.conn,
                window.id,
                damage::ReportLevel::NON_EMPTY,
            )?;
            self.conn.flush()?;
            let _ = wait_for_damage_quiet(
                &self.conn,
                damage.damage(),
                window.id,
                &mut child,
                Duration::from_millis(QUIET_MS),
                Duration::from_millis(TRACE_TIMEOUT_MS),
            )?;
            let startup_ms = elapsed_ms(startup_started);

            focus_window_for_keyboard(&self.conn, &window)?;

            let before = proc_sample(pid)?;
            let trace_started = Instant::now();
            let typing_started = Instant::now();
            inject_text(&self.conn, self.root, &self.keycodes, &payload)?;
            let damage_events = wait_for_damage_quiet(
                &self.conn,
                damage.damage(),
                window.id,
                &mut child,
                Duration::from_millis(QUIET_MS),
                Duration::from_millis(TRACE_TIMEOUT_MS),
            )?;
            let typing_input_to_quiet_ms = elapsed_ms(typing_started);
            let (_file_damage_events, save_retry_count, final_stats) =
                wait_for_file_text_with_save_retry(
                    &self.conn,
                    damage.damage(),
                    window.id,
                    &mut child,
                    self.root,
                    &self.keycodes,
                    &file_path,
                    &expected_text,
                    Duration::from_millis(FILE_STABLE_MS),
                    Duration::from_millis(SAVE_RETRY_MS),
                    Duration::from_millis(FILE_STABLE_TIMEOUT_MS),
                )?;
            let trace_wall_ms = elapsed_ms(trace_started);
            let after = proc_sample(pid)?;
            let trace = read_editor_trace(&trace_path)?;

            let mut metrics = RunMetrics::new(window.width, window.height);
            metrics.set("startup_ms", startup_ms);
            metrics.set("typing_input_to_quiet_ms", typing_input_to_quiet_ms);
            metrics.set(
                "typing_ms_per_char",
                typing_input_to_quiet_ms / payload.chars().count() as f64,
            );
            metrics.set("typing_completion_ms", trace_wall_ms);
            metrics.set("trace_wall_ms", trace_wall_ms);
            metrics.set("damage_events", damage_events as f64);
            metrics.set("save_retry_count", save_retry_count as f64);
            metrics.set("typed_chars", payload.chars().count() as f64);
            metrics.set("final_file_bytes", final_stats.bytes as f64);
            metrics.set("final_file_lines", final_stats.lines as f64);
            add_process_metrics(&mut metrics, &before, &after, self.ticks_per_second);
            add_trace_aggregate(
                &mut metrics,
                &trace,
                "text_input_apply_ms",
                "text_input_apply_ms_sum",
                "text_input_apply_ms_max",
                "text_input_apply_ms_count",
            );
            Ok(metrics)
        })();

        let terminate_result = terminate_child(&mut child);
        cleanup_paths_if(
            [file_path, trace_path],
            result.is_ok() || !keep_temp_on_failure,
        );
        terminate_result?;
        result
    }

    fn run_scroll(
        &self,
        scenario: Scenario,
        corpus: &Corpus,
        run_index: usize,
        keep_temp_on_failure: bool,
    ) -> Result<RunMetrics, Box<dyn Error>> {
        let file_path = temp_path(scenario, run_index, "file", corpus.extension);
        fs::write(&file_path, &corpus.text)?;

        let title = bench_title(scenario, run_index);
        let files = [file_path.as_path()];
        let mut child = self.spawn_editor(&files, &title, None)?;
        let pid = child.id();

        let result = (|| {
            let startup_started = Instant::now();
            let window = find_window(
                &self.conn,
                self.root,
                &self.atoms,
                pid,
                &title,
                &mut child,
                Duration::from_millis(WINDOW_DISCOVERY_TIMEOUT_MS),
            )?;
            let damage = damage::DamageWrapper::create(
                &self.conn,
                window.id,
                damage::ReportLevel::NON_EMPTY,
            )?;
            self.conn.flush()?;
            let _ = wait_for_damage_quiet(
                &self.conn,
                damage.damage(),
                window.id,
                &mut child,
                Duration::from_millis(QUIET_MS),
                Duration::from_millis(TRACE_TIMEOUT_MS),
            )?;
            let startup_ms = elapsed_ms(startup_started);

            focus_window(&self.conn, self.root, &window)?;
            inject_wheel_burst(
                &self.conn,
                self.root,
                BUTTON_WHEEL_DOWN,
                20,
                Duration::from_millis(150),
            )?;
            let _ = wait_for_damage_quiet(
                &self.conn,
                damage.damage(),
                window.id,
                &mut child,
                Duration::from_millis(QUIET_MS),
                Duration::from_millis(TRACE_TIMEOUT_MS),
            )?;

            let before = proc_sample(pid)?;
            let trace_started = Instant::now();
            inject_wheel_burst(
                &self.conn,
                self.root,
                BUTTON_WHEEL_DOWN,
                SCROLL_WHEEL_COUNT,
                Duration::from_millis(SCROLL_HALF_MS),
            )?;
            inject_wheel_burst(
                &self.conn,
                self.root,
                BUTTON_WHEEL_UP,
                SCROLL_WHEEL_COUNT,
                Duration::from_millis(SCROLL_HALF_MS),
            )?;
            let damage_events = wait_for_damage_quiet(
                &self.conn,
                damage.damage(),
                window.id,
                &mut child,
                Duration::from_millis(QUIET_MS),
                Duration::from_millis(TRACE_TIMEOUT_MS),
            )?;
            let trace_wall_ms = elapsed_ms(trace_started);
            let scheduled_ms = (SCROLL_HALF_MS * 2) as f64;
            let after = proc_sample(pid)?;

            let mut metrics = RunMetrics::new(window.width, window.height);
            metrics.set("startup_ms", startup_ms);
            metrics.set("scroll_scheduled_ms", scheduled_ms);
            metrics.set("trace_wall_ms", trace_wall_ms);
            metrics.set("scroll_overrun_ms", (trace_wall_ms - scheduled_ms).max(0.0));
            metrics.set("damage_events", damage_events as f64);
            metrics.set(
                "damage_hz_proxy",
                damage_hz_proxy(damage_events, trace_wall_ms),
            );
            add_process_metrics(&mut metrics, &before, &after, self.ticks_per_second);
            Ok(metrics)
        })();

        let terminate_result = terminate_child(&mut child);
        cleanup_paths_if([file_path], result.is_ok() || !keep_temp_on_failure);
        terminate_result?;
        result
    }

    fn run_open_large(
        &self,
        scenario: Scenario,
        corpus: &Corpus,
        run_index: usize,
        keep_temp_on_failure: bool,
    ) -> Result<RunMetrics, Box<dyn Error>> {
        let file_path = temp_path(scenario, run_index, "file", corpus.extension);
        fs::write(&file_path, &corpus.text)?;

        let title = bench_title(scenario, run_index);
        let files = [file_path.as_path()];
        let open_started = Instant::now();
        let mut child = self.spawn_editor(&files, &title, None)?;
        let pid = child.id();
        let before = proc_sample(pid);

        let result = (|| {
            let before = before?;
            let window = find_window(
                &self.conn,
                self.root,
                &self.atoms,
                pid,
                &title,
                &mut child,
                Duration::from_millis(WINDOW_DISCOVERY_TIMEOUT_MS),
            )?;
            let damage = damage::DamageWrapper::create(
                &self.conn,
                window.id,
                damage::ReportLevel::NON_EMPTY,
            )?;
            self.conn.flush()?;
            let damage_events = wait_for_damage_quiet(
                &self.conn,
                damage.damage(),
                window.id,
                &mut child,
                Duration::from_millis(QUIET_MS),
                Duration::from_millis(TRACE_TIMEOUT_MS),
            )?;
            let open_to_quiet_ms = elapsed_ms(open_started);
            let after = proc_sample(pid)?;

            let mut metrics = RunMetrics::new(window.width, window.height);
            metrics.set("open_to_quiet_ms", open_to_quiet_ms);
            metrics.set("startup_ms", open_to_quiet_ms);
            metrics.set("trace_wall_ms", open_to_quiet_ms);
            metrics.set("damage_events", damage_events as f64);
            add_process_metrics(&mut metrics, &before, &after, self.ticks_per_second);
            Ok(metrics)
        })();

        let terminate_result = terminate_child(&mut child);
        cleanup_paths_if([file_path], result.is_ok() || !keep_temp_on_failure);
        terminate_result?;
        result
    }

    fn run_search(
        &self,
        scenario: Scenario,
        corpus: &Corpus,
        run_index: usize,
        keep_temp_on_failure: bool,
    ) -> Result<RunMetrics, Box<dyn Error>> {
        let file_path = temp_path(scenario, run_index, "file", corpus.extension);
        let trace_path = temp_path(scenario, run_index, "trace", "log");
        fs::write(&file_path, &corpus.text)?;

        let title = bench_title(scenario, run_index);
        let files = [file_path.as_path()];
        let mut child = self.spawn_editor(&files, &title, Some(&trace_path))?;
        let pid = child.id();

        let result = (|| {
            let startup_started = Instant::now();
            let window = find_window(
                &self.conn,
                self.root,
                &self.atoms,
                pid,
                &title,
                &mut child,
                Duration::from_millis(WINDOW_DISCOVERY_TIMEOUT_MS),
            )?;
            let damage = damage::DamageWrapper::create(
                &self.conn,
                window.id,
                damage::ReportLevel::NON_EMPTY,
            )?;
            self.conn.flush()?;
            let _ = wait_for_damage_quiet(
                &self.conn,
                damage.damage(),
                window.id,
                &mut child,
                Duration::from_millis(QUIET_MS),
                Duration::from_millis(TRACE_TIMEOUT_MS),
            )?;
            let startup_ms = elapsed_ms(startup_started);

            focus_window(&self.conn, self.root, &window)?;
            let before = proc_sample(pid)?;
            let trace_started = Instant::now();
            inject_ctrl_chord(
                &self.conn,
                self.root,
                self.keycodes.control_l,
                self.keycodes.f,
            )?;
            let _ = wait_for_damage_quiet(
                &self.conn,
                damage.damage(),
                window.id,
                &mut child,
                Duration::from_millis(QUIET_MS),
                Duration::from_millis(TRACE_TIMEOUT_MS),
            )?;
            wait_for_trace_label(
                &trace_path,
                "focus_applied",
                "find_query",
                Duration::from_millis(TRACE_TIMEOUT_MS),
            )?;
            thread::sleep(Duration::from_millis(150));
            let _ = wait_for_damage_quiet(
                &self.conn,
                damage.damage(),
                window.id,
                &mut child,
                Duration::from_millis(QUIET_MS),
                Duration::from_millis(TRACE_TIMEOUT_MS),
            )?;
            click_find_query_field(&self.conn, self.root, &window)?;
            thread::sleep(Duration::from_millis(150));
            let _ = wait_for_damage_quiet(
                &self.conn,
                damage.damage(),
                window.id,
                &mut child,
                Duration::from_millis(QUIET_MS),
                Duration::from_millis(TRACE_TIMEOUT_MS),
            )?;
            let search_input_started = Instant::now();
            inject_text(&self.conn, self.root, &self.keycodes, SEARCH_QUERY)?;
            let damage_events = wait_for_damage_quiet(
                &self.conn,
                damage.damage(),
                window.id,
                &mut child,
                Duration::from_millis(QUIET_MS),
                Duration::from_millis(TRACE_TIMEOUT_MS),
            )?;
            let search_input_to_quiet_ms = elapsed_ms(search_input_started);
            let trace_wall_ms = elapsed_ms(trace_started);
            let after = proc_sample(pid)?;
            let trace = read_editor_trace(&trace_path)?;
            let reindex_ms = trace
                .last("find_reindex_ms")
                .ok_or_else(|| io::Error::other("search benchmark produced no find trace"))?;
            let query_len = trace.last("find_query_len").unwrap_or_default();
            if query_len as usize != SEARCH_QUERY.chars().count() {
                return Err(io::Error::other(format!(
                    "search benchmark ended with query length {query_len}, expected {}",
                    SEARCH_QUERY.chars().count()
                ))
                .into());
            }

            let mut metrics = RunMetrics::new(window.width, window.height);
            metrics.set("startup_ms", startup_ms);
            metrics.set("search_input_to_quiet_ms", search_input_to_quiet_ms);
            metrics.set("search_reindex_ms", reindex_ms);
            metrics.set("trace_wall_ms", trace_wall_ms);
            metrics.set("damage_events", damage_events as f64);
            add_trace_last(&mut metrics, &trace, "find_match_count", "find_match_count");
            add_trace_last(&mut metrics, &trace, "find_query_len", "find_query_len");
            add_process_metrics(&mut metrics, &before, &after, self.ticks_per_second);
            Ok(metrics)
        })();

        let terminate_result = terminate_child(&mut child);
        cleanup_paths_if(
            [file_path, trace_path],
            result.is_ok() || !keep_temp_on_failure,
        );
        terminate_result?;
        result
    }

    fn spawn_editor(
        &self,
        files: &[&Path],
        title: &str,
        trace_path: Option<&Path>,
    ) -> Result<Child, Box<dyn Error>> {
        let mut command = Command::new(&self.editor);
        command
            .arg("--title")
            .arg(title)
            .stdin(Stdio::null())
            .stdout(if debug_enabled() {
                Stdio::inherit()
            } else {
                Stdio::null()
            })
            .stderr(if debug_enabled() {
                Stdio::inherit()
            } else {
                Stdio::null()
            })
            .env("DISPLAY", &self.session_env.display);

        if let Some(trace_path) = trace_path {
            command.env("LST_BENCH_TRACE_FILE", trace_path);
        }
        if let Some(xauthority) = &self.session_env.xauthority {
            command.env("XAUTHORITY", xauthority);
        }
        if let Some(dbus) = &self.session_env.dbus_session_bus_address {
            command.env("DBUS_SESSION_BUS_ADDRESS", dbus);
        }
        for file in files {
            command.arg(file);
        }

        Ok(command.spawn()?)
    }
}

#[derive(Debug)]
struct Corpus {
    label: String,
    text: String,
    bytes: u64,
    lines: usize,
    extension: &'static str,
    highlight: &'static str,
}

impl Corpus {
    fn load(kind: CorpusKind) -> Result<Self, Box<dyn Error>> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(kind.source_rel());
        let text = fs::read_to_string(&path)?;
        Ok(Self {
            label: kind.source_rel().to_string(),
            bytes: text.len() as u64,
            lines: text.lines().count(),
            text,
            extension: kind.extension(),
            highlight: kind.highlight_label(),
        })
    }
}

#[derive(Debug)]
struct RunMetrics {
    values: HashMap<&'static str, f64>,
    window_size: (u16, u16),
}

impl RunMetrics {
    fn new(width: u16, height: u16) -> Self {
        Self {
            values: HashMap::new(),
            window_size: (width, height),
        }
    }

    fn set(&mut self, name: &'static str, value: f64) {
        self.values.insert(name, value);
    }

    fn get(&self, name: &str) -> Result<f64, Box<dyn Error>> {
        self.values
            .get(name)
            .copied()
            .ok_or_else(|| io::Error::other(format!("missing metric {name}")).into())
    }
}

fn emit_summary(
    scenario: Scenario,
    args: &Args,
    session_env: &SessionEnv,
    corpus: &Corpus,
    window_size: Option<(u16, u16)>,
    runs: &[RunMetrics],
) -> Result<(), Box<dyn Error>> {
    if runs.is_empty() {
        return Err(io::Error::other("benchmark produced no measured runs").into());
    }
    let primary_metric = scenario.primary_metric();
    let primary_value = median_metric(runs, primary_metric)?;
    let window_size = window_size.unwrap_or((0, 0));

    println!("scenario={}", scenario.as_str());
    println!("display={}", session_env.display);
    println!("file={}", corpus.label);
    println!("file_bytes={}", corpus.bytes);
    println!("file_lines={}", corpus.lines);
    println!("highlight={}", corpus.highlight);
    println!("window_client_px={}x{}", window_size.0, window_size.1);
    println!("priming_runs={}", args.priming_runs);
    println!("repetitions={}", args.repetitions);
    println!("quiet_ms={QUIET_MS}");
    println!("primary_metric={primary_metric}");
    println!("primary_value={:.3}", primary_value);

    for metric in metric_order(scenario) {
        print_metric(runs, metric)?;
    }

    println!("score={:.3}", primary_value);
    println!();
    Ok(())
}

fn metric_order(scenario: Scenario) -> &'static [&'static str] {
    match scenario {
        Scenario::All => &[],
        Scenario::LargePaste => &[
            "paste_complete_ms",
            "select_all_ms",
            "copy_clipboard_ms",
            "tab_switch_ms",
            "paste_apply_ms",
            "paste_clipboard_read_ms",
            "paste_bytes",
            "paste_lines",
            "trace_wall_ms",
            "damage_events",
            "paste_damage_events",
            "save_retry_count",
            "user_cpu_ms",
            "sys_cpu_ms",
            "cpu_ms",
            "peak_rss_mb",
            "final_file_bytes",
            "final_file_lines",
        ],
        Scenario::TypingMedium | Scenario::TypingLarge => &[
            "typing_ms_per_char",
            "typing_input_to_quiet_ms",
            "typing_completion_ms",
            "typed_chars",
            "text_input_apply_ms_sum",
            "text_input_apply_ms_max",
            "text_input_apply_ms_count",
            "trace_wall_ms",
            "damage_events",
            "save_retry_count",
            "user_cpu_ms",
            "sys_cpu_ms",
            "cpu_ms",
            "peak_rss_mb",
            "final_file_bytes",
            "final_file_lines",
        ],
        Scenario::ScrollHighlighted | Scenario::ScrollPlain => &[
            "scroll_overrun_ms",
            "scroll_scheduled_ms",
            "trace_wall_ms",
            "damage_events",
            "damage_hz_proxy",
            "user_cpu_ms",
            "sys_cpu_ms",
            "cpu_ms",
            "peak_rss_mb",
        ],
        Scenario::OpenLarge => &[
            "open_to_quiet_ms",
            "damage_events",
            "user_cpu_ms",
            "sys_cpu_ms",
            "cpu_ms",
            "peak_rss_mb",
        ],
        Scenario::SearchLarge => &[
            "search_reindex_ms",
            "search_input_to_quiet_ms",
            "find_match_count",
            "find_query_len",
            "trace_wall_ms",
            "damage_events",
            "user_cpu_ms",
            "sys_cpu_ms",
            "cpu_ms",
            "peak_rss_mb",
        ],
    }
}

fn print_metric(runs: &[RunMetrics], metric: &'static str) -> Result<(), Box<dyn Error>> {
    println!("{}_runs={}", metric, join_metric_runs(runs, metric)?);
    let median = median_metric(runs, metric)?;
    if is_integer_metric(metric) {
        println!("{metric}={:.0}", median);
    } else {
        println!("{metric}={:.3}", median);
    }
    Ok(())
}

fn join_metric_runs(runs: &[RunMetrics], metric: &str) -> Result<String, Box<dyn Error>> {
    let mut values = Vec::with_capacity(runs.len());
    for run in runs {
        let value = run.get(metric)?;
        if is_integer_metric(metric) {
            values.push(format!("{value:.0}"));
        } else {
            values.push(format!("{value:.3}"));
        }
    }
    Ok(values.join(","))
}

fn median_metric(runs: &[RunMetrics], metric: &str) -> Result<f64, Box<dyn Error>> {
    median_f64(
        &runs
            .iter()
            .map(|run| run.get(metric))
            .collect::<Result<Vec<_>, _>>()?,
    )
}

fn is_integer_metric(metric: &str) -> bool {
    metric.ends_with("_bytes")
        || metric.ends_with("_lines")
        || metric.ends_with("_count")
        || metric.ends_with("_events")
        || metric.ends_with("_chars")
        || metric.ends_with("_len")
}

fn add_process_metrics(
    metrics: &mut RunMetrics,
    before: &ProcSample,
    after: &ProcSample,
    ticks_per_second: u64,
) {
    let user_cpu_ms = ticks_to_ms(
        after.utime_ticks.saturating_sub(before.utime_ticks),
        ticks_per_second,
    );
    let sys_cpu_ms = ticks_to_ms(
        after.stime_ticks.saturating_sub(before.stime_ticks),
        ticks_per_second,
    );
    metrics.set("user_cpu_ms", user_cpu_ms);
    metrics.set("sys_cpu_ms", sys_cpu_ms);
    metrics.set("cpu_ms", user_cpu_ms + sys_cpu_ms);
    metrics.set("peak_rss_mb", after.vmhwm_kb as f64 / 1024.0);
}

fn add_trace_last(metrics: &mut RunMetrics, trace: &EditorTrace, from: &str, to: &'static str) {
    if let Some(value) = trace.last(from) {
        metrics.set(to, value);
    }
}

fn add_trace_aggregate(
    metrics: &mut RunMetrics,
    trace: &EditorTrace,
    from: &str,
    sum_name: &'static str,
    max_name: &'static str,
    count_name: &'static str,
) {
    if let Some(value) = trace.sum(from) {
        metrics.set(sum_name, value);
    }
    if let Some(value) = trace.max(from) {
        metrics.set(max_name, value);
    }
    if let Some(value) = trace.count(from) {
        metrics.set(count_name, value as f64);
    }
}

#[derive(Default, Debug)]
struct EditorTrace {
    last_values: HashMap<String, f64>,
    last_labels: HashMap<String, String>,
    sum_values: HashMap<String, f64>,
    max_values: HashMap<String, f64>,
    counts: HashMap<String, usize>,
}

impl EditorTrace {
    fn parse(contents: &str) -> Self {
        let mut trace = Self::default();
        for line in contents.lines() {
            let Some((label, value)) = line.split_once('=') else {
                continue;
            };
            let Ok(value) = value.parse::<f64>() else {
                trace
                    .last_labels
                    .insert(label.to_string(), value.to_string());
                continue;
            };
            trace.last_values.insert(label.to_string(), value);
            *trace.sum_values.entry(label.to_string()).or_insert(0.0) += value;
            trace
                .max_values
                .entry(label.to_string())
                .and_modify(|max| *max = max.max(value))
                .or_insert(value);
            *trace.counts.entry(label.to_string()).or_insert(0) += 1;
        }
        trace
    }

    fn last(&self, label: &str) -> Option<f64> {
        self.last_values.get(label).copied()
    }

    fn last_label(&self, label: &str) -> Option<&str> {
        self.last_labels.get(label).map(String::as_str)
    }

    fn sum(&self, label: &str) -> Option<f64> {
        self.sum_values.get(label).copied()
    }

    fn max(&self, label: &str) -> Option<f64> {
        self.max_values.get(label).copied()
    }

    fn count(&self, label: &str) -> Option<usize> {
        self.counts.get(label).copied()
    }
}

fn read_editor_trace(path: &Path) -> Result<EditorTrace, Box<dyn Error>> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(EditorTrace::default()),
        Err(error) => return Err(error.into()),
    };
    Ok(EditorTrace::parse(&contents))
}

fn wait_for_trace_label(
    path: &Path,
    label: &str,
    expected: &str,
    timeout: Duration,
) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + timeout;

    loop {
        let trace = read_editor_trace(path)?;
        if trace.last_label(label) == Some(expected) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(io::Error::other(format!(
                "timed out waiting for trace {label}={expected}"
            ))
            .into());
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn find_window(
    conn: &RustConnection,
    root: xproto::Window,
    atoms: &Atoms,
    pid: u32,
    title: &str,
    child: &mut Child,
    timeout: Duration,
) -> Result<WindowInfo, Box<dyn Error>> {
    let deadline = Instant::now() + timeout;

    loop {
        if let Some(status) = child.try_wait()? {
            return Err(io::Error::other(format!(
                "editor exited before its window appeared: {status}"
            ))
            .into());
        }

        if let Some(info) = find_window_recursive(conn, root, root, atoms, pid, title)? {
            return Ok(info);
        }

        if Instant::now() >= deadline {
            return Err(io::Error::other("timed out waiting for benchmark window").into());
        }

        thread::sleep(Duration::from_millis(25));
    }
}

fn find_window_recursive(
    conn: &RustConnection,
    root: xproto::Window,
    window: xproto::Window,
    atoms: &Atoms,
    pid: u32,
    title: &str,
) -> Result<Option<WindowInfo>, Box<dyn Error>> {
    if window_matches(conn, window, atoms, pid, title)? {
        let attrs = match conn.get_window_attributes(window)?.reply() {
            Ok(attrs) => attrs,
            Err(error) if is_stale_window_error(&error) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        if attrs.map_state == MapState::VIEWABLE {
            let geometry = match conn.get_geometry(window)?.reply() {
                Ok(geometry) => geometry,
                Err(error) if is_stale_window_error(&error) => return Ok(None),
                Err(error) => return Err(error.into()),
            };
            let translated = match conn.translate_coordinates(window, root, 0, 0)?.reply() {
                Ok(translated) => translated,
                Err(error) if is_stale_window_error(&error) => return Ok(None),
                Err(error) => return Err(error.into()),
            };
            return Ok(Some(WindowInfo {
                id: window,
                root_x: translated.dst_x,
                root_y: translated.dst_y,
                width: geometry.width,
                height: geometry.height,
            }));
        }
    }

    let tree = match conn.query_tree(window)?.reply() {
        Ok(tree) => tree,
        Err(error) if is_stale_window_error(&error) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    for child in tree.children {
        if let Some(info) = find_window_recursive(conn, root, child, atoms, pid, title)? {
            return Ok(Some(info));
        }
    }

    Ok(None)
}

fn window_matches(
    conn: &RustConnection,
    window: xproto::Window,
    atoms: &Atoms,
    pid: u32,
    title: &str,
) -> Result<bool, Box<dyn Error>> {
    let Some(window_pid) = window_pid(conn, window, atoms)? else {
        return Ok(false);
    };
    if window_pid != pid {
        return Ok(false);
    }

    Ok(window_title(conn, window, atoms)?.as_deref() == Some(title))
}

fn window_pid(
    conn: &RustConnection,
    window: xproto::Window,
    atoms: &Atoms,
) -> Result<Option<u32>, Box<dyn Error>> {
    let reply = match conn
        .get_property(false, window, atoms.net_wm_pid, AtomEnum::CARDINAL, 0, 1)?
        .reply()
    {
        Ok(reply) => reply,
        Err(error) if is_stale_window_error(&error) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    Ok(reply.value32().and_then(|mut values| values.next()))
}

fn window_title(
    conn: &RustConnection,
    window: xproto::Window,
    atoms: &Atoms,
) -> Result<Option<String>, Box<dyn Error>> {
    let utf8 = match conn
        .get_property(false, window, atoms.net_wm_name, atoms.utf8_string, 0, 1024)?
        .reply()
    {
        Ok(reply) => reply,
        Err(error) if is_stale_window_error(&error) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if !utf8.value.is_empty() {
        return Ok(Some(String::from_utf8_lossy(&utf8.value).into_owned()));
    }

    let legacy = match conn
        .get_property(false, window, AtomEnum::WM_NAME, AtomEnum::STRING, 0, 1024)?
        .reply()
    {
        Ok(reply) => reply,
        Err(error) if is_stale_window_error(&error) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if legacy.value.is_empty() {
        return Ok(None);
    }

    Ok(Some(String::from_utf8_lossy(&legacy.value).into_owned()))
}

fn is_stale_window_error(error: &ReplyError) -> bool {
    matches!(
        error,
        ReplyError::X11Error(error) if error.error_kind == ErrorKind::Window
    )
}

fn focus_window(
    conn: &RustConnection,
    root: xproto::Window,
    window: &WindowInfo,
) -> Result<(), Box<dyn Error>> {
    move_pointer_to_window_center(conn, root, window)?;
    thread::sleep(Duration::from_millis(POINTER_SETTLE_MS));
    inject_button_click(conn, root, BUTTON_LEFT)
}

fn click_find_query_field(
    conn: &RustConnection,
    root: xproto::Window,
    window: &WindowInfo,
) -> Result<(), Box<dyn Error>> {
    move_pointer_to_window_point(
        conn,
        root,
        window,
        (f32::from(window.width) * FIND_QUERY_CLICK_X_FRACTION).round() as i32,
        (f32::from(window.height) * FIND_QUERY_CLICK_Y_FRACTION).round() as i32,
    )?;
    thread::sleep(Duration::from_millis(POINTER_SETTLE_MS));
    inject_button_click(conn, root, BUTTON_LEFT)
}

fn focus_window_for_keyboard(
    conn: &RustConnection,
    window: &WindowInfo,
) -> Result<(), Box<dyn Error>> {
    conn.set_input_focus(xproto::InputFocus::PARENT, window.id, x11rb::CURRENT_TIME)?;
    conn.flush()?;
    thread::sleep(Duration::from_millis(POINTER_SETTLE_MS));
    Ok(())
}

fn move_pointer_to_window_center(
    conn: &RustConnection,
    root: xproto::Window,
    window: &WindowInfo,
) -> Result<(), Box<dyn Error>> {
    move_pointer_to_window_point(
        conn,
        root,
        window,
        i32::from(window.width) / 2,
        i32::from(window.height) / 2,
    )
}

fn move_pointer_to_window_point(
    conn: &RustConnection,
    root: xproto::Window,
    window: &WindowInfo,
    local_x: i32,
    local_y: i32,
) -> Result<(), Box<dyn Error>> {
    let x = clamp_i16(i32::from(window.root_x) + local_x);
    let y = clamp_i16(i32::from(window.root_y) + local_y);
    conn.xtest_fake_input(xproto::MOTION_NOTIFY_EVENT, 0, 0, root, x, y, 0)?;
    conn.flush()?;
    Ok(())
}

fn inject_button_click(
    conn: &RustConnection,
    root: xproto::Window,
    button: u8,
) -> Result<(), Box<dyn Error>> {
    conn.xtest_fake_input(xproto::BUTTON_PRESS_EVENT, button, 0, root, 0, 0, 0)?;
    conn.xtest_fake_input(xproto::BUTTON_RELEASE_EVENT, button, 0, root, 0, 0, 0)?;
    conn.flush()?;
    Ok(())
}

fn inject_ctrl_chord(
    conn: &RustConnection,
    root: xproto::Window,
    control_keycode: xproto::Keycode,
    keycode: xproto::Keycode,
) -> Result<(), Box<dyn Error>> {
    inject_key_press(conn, root, control_keycode)?;
    inject_key_press(conn, root, keycode)?;
    inject_key_release(conn, root, keycode)?;
    inject_key_release(conn, root, control_keycode)?;
    conn.flush()?;
    Ok(())
}

fn inject_text(
    conn: &RustConnection,
    root: xproto::Window,
    keycodes: &Keycodes,
    text: &str,
) -> Result<(), Box<dyn Error>> {
    for ch in text.chars() {
        let keycode = keycodes
            .text_keycode(ch)
            .ok_or_else(|| io::Error::other(format!("unsupported benchmark input char: {ch:?}")))?;
        inject_key_press(conn, root, keycode)?;
        inject_key_release(conn, root, keycode)?;
        conn.flush()?;
    }
    Ok(())
}

fn inject_wheel_burst(
    conn: &RustConnection,
    root: xproto::Window,
    button: u8,
    count: usize,
    total_duration: Duration,
) -> Result<(), Box<dyn Error>> {
    let start = Instant::now();

    for index in 0..count {
        conn.xtest_fake_input(xproto::BUTTON_PRESS_EVENT, button, 0, root, 0, 0, 0)?;
        conn.xtest_fake_input(xproto::BUTTON_RELEASE_EVENT, button, 0, root, 0, 0, 0)?;
        conn.flush()?;

        if !total_duration.is_zero() && count > 0 {
            let target_time = start + total_duration.mul_f64((index + 1) as f64 / count as f64);
            let now = Instant::now();
            if target_time > now {
                thread::sleep(target_time - now);
            }
        }
    }

    Ok(())
}

fn inject_key_press(
    conn: &RustConnection,
    root: xproto::Window,
    keycode: xproto::Keycode,
) -> Result<(), Box<dyn Error>> {
    conn.xtest_fake_input(xproto::KEY_PRESS_EVENT, keycode, 0, root, 0, 0, 0)?;
    Ok(())
}

fn inject_key_release(
    conn: &RustConnection,
    root: xproto::Window,
    keycode: xproto::Keycode,
) -> Result<(), Box<dyn Error>> {
    conn.xtest_fake_input(xproto::KEY_RELEASE_EVENT, keycode, 0, root, 0, 0, 0)?;
    Ok(())
}

fn wait_for_damage_quiet(
    conn: &RustConnection,
    damage_id: damage::Damage,
    window: xproto::Window,
    child: &mut Child,
    quiet_for: Duration,
    timeout: Duration,
) -> Result<u64, Box<dyn Error>> {
    let deadline = Instant::now() + timeout;
    let mut last_damage = Instant::now();
    let mut damage_events = 0u64;

    loop {
        if let Some(status) = child.try_wait()? {
            return Err(io::Error::other(format!(
                "editor exited while benchmark was waiting for redraws to finish: {status}"
            ))
            .into());
        }

        while let Some(event) = conn.poll_for_event()? {
            if let Event::DamageNotify(notify) = event {
                if notify.damage == damage_id && notify.drawable == window {
                    last_damage = Instant::now();
                    damage_events += 1;
                    conn.damage_subtract(damage_id, NONE, NONE)?;
                }
            }
        }

        conn.flush()?;

        if last_damage.elapsed() >= quiet_for {
            return Ok(damage_events);
        }

        if Instant::now() >= deadline {
            return Err(io::Error::other("timed out waiting for redraw quiet period").into());
        }

        thread::sleep(Duration::from_millis(5));
    }
}

fn wait_for_file_text_with_save_retry(
    conn: &RustConnection,
    damage_id: damage::Damage,
    window: xproto::Window,
    child: &mut Child,
    root: xproto::Window,
    keycodes: &Keycodes,
    path: &Path,
    expected_text: &str,
    stable_for: Duration,
    save_retry_every: Duration,
    timeout: Duration,
) -> Result<(u64, u64, FileStats), Box<dyn Error>> {
    let deadline = Instant::now() + timeout;
    let mut last_text = fs::read_to_string(path).unwrap_or_default();
    let mut last_change = Instant::now();
    let mut last_save: Option<Instant> = None;
    let mut damage_events = 0u64;
    let mut save_retry_count = 0u64;

    loop {
        if let Some(status) = child.try_wait()? {
            return Err(io::Error::other(format!(
                "editor exited while benchmark was waiting for file contents: {status}"
            ))
            .into());
        }

        while let Some(event) = conn.poll_for_event()? {
            if let Event::DamageNotify(notify) = event {
                if notify.damage == damage_id && notify.drawable == window {
                    damage_events += 1;
                    conn.damage_subtract(damage_id, NONE, NONE)?;
                }
            }
        }
        conn.flush()?;

        let current = fs::read_to_string(path).unwrap_or_default();
        if current != last_text {
            last_text = current;
            last_change = Instant::now();
        }

        if last_text == expected_text && last_change.elapsed() >= stable_for {
            return Ok((
                damage_events,
                save_retry_count,
                FileStats {
                    bytes: last_text.len() as u64,
                    lines: last_text.lines().count(),
                },
            ));
        }

        let should_retry_save = match last_save {
            Some(saved_at) => saved_at.elapsed() >= save_retry_every,
            None => true,
        };
        if should_retry_save {
            inject_ctrl_chord(conn, root, keycodes.control_l, keycodes.s)?;
            last_save = Some(Instant::now());
            save_retry_count += 1;
        }

        if Instant::now() >= deadline {
            return Err(io::Error::other(format!(
                "timed out waiting for {} to reach {} bytes; last observed {} bytes",
                path.display(),
                expected_text.len(),
                last_text.len()
            ))
            .into());
        }

        thread::sleep(Duration::from_millis(20));
    }
}

fn wait_for_clipboard_bytes(expected_bytes: u64, timeout: Duration) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + timeout;

    loop {
        if let Some(actual_bytes) = read_clipboard_bytes()? {
            if actual_bytes == expected_bytes {
                return Ok(());
            }
        }

        if Instant::now() >= deadline {
            return Err(io::Error::other(format!(
                "timed out waiting for clipboard to reach {expected_bytes} bytes"
            ))
            .into());
        }

        thread::sleep(Duration::from_millis(20));
    }
}

fn read_clipboard_bytes() -> Result<Option<u64>, Box<dyn Error>> {
    let output = Command::new("xclip")
        .args(["-selection", "clipboard", "-o"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()?;

    if output.status.success() {
        Ok(Some(output.stdout.len() as u64))
    } else {
        Ok(None)
    }
}

fn terminate_child(child: &mut Child) -> Result<(), Box<dyn Error>> {
    if child.try_wait()?.is_some() {
        return Ok(());
    }

    unsafe {
        libc::kill(child.id() as i32, libc::SIGTERM);
    }

    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if child.try_wait()?.is_some() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }

    child.kill()?;
    child.wait()?;
    Ok(())
}

fn proc_sample(pid: u32) -> Result<ProcSample, Box<dyn Error>> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat"))?;
    let end = stat
        .rfind(')')
        .ok_or_else(|| io::Error::other("failed to parse /proc stat"))?;
    let fields: Vec<&str> = stat[end + 2..].split_whitespace().collect();
    if fields.len() <= 12 {
        return Err(io::Error::other("unexpected /proc stat field count").into());
    }

    let status = fs::read_to_string(format!("/proc/{pid}/status"))?;
    let vmhwm_kb = status
        .lines()
        .find_map(|line| {
            let value = line.strip_prefix("VmHWM:")?;
            value.split_whitespace().next()?.parse::<u64>().ok()
        })
        .unwrap_or(0);

    Ok(ProcSample {
        utime_ticks: fields[11].parse()?,
        stime_ticks: fields[12].parse()?,
        vmhwm_kb,
    })
}

fn clock_ticks_per_second() -> Result<u64, Box<dyn Error>> {
    let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    if ticks <= 0 {
        return Err(io::Error::other("sysconf(_SC_CLK_TCK) failed").into());
    }
    Ok(ticks as u64)
}

fn ticks_to_ms(ticks: u64, ticks_per_second: u64) -> f64 {
    ticks as f64 * 1000.0 / ticks_per_second as f64
}

fn editor_path() -> Result<PathBuf, Box<dyn Error>> {
    let current = env::current_exe()?;
    let profile_dir = current
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| io::Error::other("benchmark binary has no profile directory"))?;
    let sibling = profile_dir.join("lst-gpui");
    if sibling.exists() {
        return Ok(sibling);
    }

    Err(io::Error::other(
        "could not find sibling editor binary 'lst-gpui'; build with `cargo build --release -p lst-gpui --bin lst-gpui --example bench_editor_x11`",
    )
    .into())
}

fn resolve_session_env() -> Result<SessionEnv, Box<dyn Error>> {
    if let Ok(display) = env::var("DISPLAY") {
        return Ok(SessionEnv {
            display,
            xauthority: env::var("XAUTHORITY").ok(),
            dbus_session_bus_address: env::var("DBUS_SESSION_BUS_ADDRESS").ok(),
        });
    }

    let proc_dir = fs::read_dir("/proc")?;
    let mut best: Option<(usize, SessionEnv)> = None;

    for entry in proc_dir {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.chars().all(|ch| ch.is_ascii_digit()) {
            continue;
        }

        let environ = match fs::read(entry.path().join("environ")) {
            Ok(environ) => environ,
            Err(_) => continue,
        };
        let vars = parse_proc_environ(&environ);
        let Some(display) = vars.get("DISPLAY").cloned() else {
            continue;
        };
        if matches!(vars.get("XDG_SESSION_TYPE"), Some(value) if value != "x11") {
            continue;
        }

        let candidate = SessionEnv {
            display,
            xauthority: vars.get("XAUTHORITY").cloned(),
            dbus_session_bus_address: vars.get("DBUS_SESSION_BUS_ADDRESS").cloned(),
        };
        let score = usize::from(candidate.xauthority.is_some())
            + usize::from(candidate.dbus_session_bus_address.is_some());
        if best
            .as_ref()
            .map(|(best_score, _)| score > *best_score)
            .unwrap_or(true)
        {
            best = Some((score, candidate));
        }
    }

    best.map(|(_, env)| env).ok_or_else(|| {
        io::Error::other(
            "could not find an X11 desktop session; run from a desktop terminal or set DISPLAY/XAUTHORITY explicitly",
        )
        .into()
    })
}

fn parse_proc_environ(bytes: &[u8]) -> HashMap<String, String> {
    let mut vars = HashMap::new();

    for entry in bytes.split(|byte| *byte == 0) {
        if entry.is_empty() {
            continue;
        }
        let text = String::from_utf8_lossy(entry);
        let Some((key, value)) = text.split_once('=') else {
            continue;
        };
        vars.insert(key.to_string(), value.to_string());
    }

    vars
}

fn apply_session_env(env: &SessionEnv) {
    env::set_var("DISPLAY", &env.display);
    if let Some(xauthority) = &env.xauthority {
        env::set_var("XAUTHORITY", xauthority);
    }
    if let Some(dbus) = &env.dbus_session_bus_address {
        env::set_var("DBUS_SESSION_BUS_ADDRESS", dbus);
    }
}

fn median_f64(values: &[f64]) -> Result<f64, Box<dyn Error>> {
    if values.is_empty() {
        return Err(io::Error::other("cannot compute median of empty sample").into());
    }

    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    Ok(sorted[sorted.len() / 2])
}

fn damage_hz_proxy(damage_events: u64, trace_wall_ms: f64) -> f64 {
    if trace_wall_ms <= 0.0 {
        0.0
    } else {
        damage_events as f64 * 1000.0 / trace_wall_ms
    }
}

fn elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

fn typing_payload(chars: usize) -> String {
    let seed = "the quick brown fox jumps over a lazy dog ";
    seed.chars().cycle().take(chars).collect()
}

fn temp_path(scenario: Scenario, run_index: usize, label: &str, extension: &str) -> PathBuf {
    env::temp_dir().join(format!(
        "lst-gpui-bench-{}-{}-{run_index}-{label}.{extension}",
        std::process::id(),
        scenario.as_str()
    ))
}

fn bench_title(scenario: Scenario, run_index: usize) -> String {
    format!(
        "lst-gpui-bench-{}-{}-{run_index}",
        std::process::id(),
        scenario.as_str()
    )
}

fn cleanup_paths_if(paths: impl IntoIterator<Item = PathBuf>, should_cleanup: bool) {
    if !should_cleanup {
        return;
    }

    for path in paths {
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => eprintln!(
                "failed to remove temporary benchmark file {}: {error}",
                path.display()
            ),
        }
    }
}

fn debug_enabled() -> bool {
    env::var_os("LST_BENCH_DEBUG").is_some()
}

fn clamp_i16(value: i32) -> i16 {
    value.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
}

#[derive(Clone)]
struct SessionEnv {
    display: String,
    xauthority: Option<String>,
    dbus_session_bus_address: Option<String>,
}

struct ProcSample {
    utime_ticks: u64,
    stime_ticks: u64,
    vmhwm_kb: u64,
}

struct FileStats {
    bytes: u64,
    lines: usize,
}

struct WindowInfo {
    id: xproto::Window,
    root_x: i16,
    root_y: i16,
    width: u16,
    height: u16,
}

struct Atoms {
    net_wm_name: xproto::Atom,
    net_wm_pid: xproto::Atom,
    utf8_string: xproto::Atom,
}

impl Atoms {
    fn intern(conn: &RustConnection) -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            net_wm_name: intern_atom(conn, b"_NET_WM_NAME")?,
            net_wm_pid: intern_atom(conn, b"_NET_WM_PID")?,
            utf8_string: intern_atom(conn, b"UTF8_STRING")?,
        })
    }
}

struct Keycodes {
    control_l: xproto::Keycode,
    a: xproto::Keycode,
    c: xproto::Keycode,
    f: xproto::Keycode,
    s: xproto::Keycode,
    tab: xproto::Keycode,
    v: xproto::Keycode,
    space: xproto::Keycode,
    lower: HashMap<char, xproto::Keycode>,
}

impl Keycodes {
    fn resolve(conn: &RustConnection) -> Result<Self, Box<dyn Error>> {
        let setup = conn.setup();
        let count = setup.max_keycode - setup.min_keycode + 1;
        let reply = conn
            .get_keyboard_mapping(setup.min_keycode, count)?
            .reply()?;
        let active_group = active_keyboard_group(conn)?;
        let mut lower = HashMap::new();
        for byte in b'a'..=b'z' {
            let ch = char::from(byte);
            lower.insert(
                ch,
                find_keycode(&reply, setup.min_keycode, u32::from(byte), active_group)?,
            );
        }

        Ok(Self {
            control_l: find_keycode(&reply, setup.min_keycode, KEYSYM_CONTROL_L, active_group)?,
            a: *lower.get(&'a').expect("resolved lowercase a"),
            c: *lower.get(&'c').expect("resolved lowercase c"),
            f: *lower.get(&'f').expect("resolved lowercase f"),
            s: *lower.get(&'s').expect("resolved lowercase s"),
            tab: find_keycode(&reply, setup.min_keycode, KEYSYM_TAB, active_group)?,
            v: *lower.get(&'v').expect("resolved lowercase v"),
            space: find_keycode(&reply, setup.min_keycode, KEYSYM_SPACE, active_group)?,
            lower,
        })
    }

    fn text_keycode(&self, ch: char) -> Option<xproto::Keycode> {
        match ch {
            ' ' => Some(self.space),
            'a'..='z' => self.lower.get(&ch).copied(),
            _ => None,
        }
    }
}

fn active_keyboard_group(conn: &RustConnection) -> Result<usize, Box<dyn Error>> {
    let reply = conn.xkb_get_state(xkb::ID::USE_CORE_KBD.into())?.reply()?;
    Ok(usize::from(u8::from(reply.group)))
}

fn find_keycode(
    reply: &xproto::GetKeyboardMappingReply,
    min_keycode: xproto::Keycode,
    keysym: u32,
    active_group: usize,
) -> Result<xproto::Keycode, Box<dyn Error>> {
    let keysyms_per_keycode = reply.keysyms_per_keycode as usize;
    let active_start = active_group.saturating_mul(2);

    for (index, group) in reply.keysyms.chunks(keysyms_per_keycode).enumerate() {
        if keysyms_match_group(group, keysym, active_start) {
            return Ok(min_keycode + index as u8);
        }
    }

    for (index, group) in reply.keysyms.chunks(keysyms_per_keycode).enumerate() {
        if keysyms_match_group(group, keysym, 0) {
            return Ok(min_keycode + index as u8);
        }
    }

    for (index, group) in reply.keysyms.chunks(keysyms_per_keycode).enumerate() {
        if group.contains(&keysym) {
            return Ok(min_keycode + index as u8);
        }
    }

    Err(io::Error::other(format!("could not resolve X11 keysym 0x{keysym:x}")).into())
}

fn keysyms_match_group(group: &[u32], keysym: u32, start: usize) -> bool {
    let Some(window) = group.get(start..start.saturating_add(2)) else {
        return false;
    };
    window.contains(&keysym)
}

fn intern_atom(conn: &RustConnection, name: &[u8]) -> Result<xproto::Atom, Box<dyn Error>> {
    Ok(conn.intern_atom(false, name)?.reply()?.atom)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_targeted_scenario_and_counts() {
        let args = parse_args_from([
            "--scenario",
            "typing-large",
            "--repetitions",
            "3",
            "--priming",
            "0",
        ])
        .expect("args should parse");

        assert_eq!(args.scenario, Scenario::TypingLarge);
        assert_eq!(args.repetitions, 3);
        assert_eq!(args.priming_runs, 0);
    }

    #[test]
    fn all_expands_to_every_measured_scenario() {
        assert_eq!(
            Scenario::All.measured_cases(),
            vec![
                Scenario::LargePaste,
                Scenario::TypingMedium,
                Scenario::TypingLarge,
                Scenario::ScrollHighlighted,
                Scenario::ScrollPlain,
                Scenario::OpenLarge,
                Scenario::SearchLarge,
            ]
        );
    }

    #[test]
    fn each_measured_scenario_has_one_primary_metric() {
        let mut primary_metrics = HashMap::new();
        for scenario in Scenario::All.measured_cases() {
            primary_metrics.insert(scenario.as_str(), scenario.primary_metric());
        }

        assert_eq!(primary_metrics["large-paste"], "paste_complete_ms");
        assert_eq!(primary_metrics["typing-medium"], "typing_ms_per_char");
        assert_eq!(primary_metrics["typing-large"], "typing_ms_per_char");
        assert_eq!(primary_metrics["scroll-highlighted"], "scroll_overrun_ms");
        assert_eq!(primary_metrics["scroll-plain"], "scroll_overrun_ms");
        assert_eq!(primary_metrics["open-large"], "open_to_quiet_ms");
        assert_eq!(primary_metrics["search-large"], "search_reindex_ms");
    }

    #[test]
    fn median_uses_upper_middle_for_existing_benchmark_style() {
        assert_eq!(median_f64(&[4.0, 1.0, 2.0]).unwrap(), 2.0);
        assert_eq!(median_f64(&[4.0, 1.0, 2.0, 3.0]).unwrap(), 3.0);
    }

    #[test]
    fn trace_parser_keeps_last_and_aggregates_repeated_values() {
        let trace = EditorTrace::parse(
            "text_input_apply_ms=1.5\ntext_input_apply_ms=2.5\nfind_query_len=3\n",
        );

        assert_eq!(trace.last("text_input_apply_ms"), Some(2.5));
        assert_eq!(trace.sum("text_input_apply_ms"), Some(4.0));
        assert_eq!(trace.max("text_input_apply_ms"), Some(2.5));
        assert_eq!(trace.count("text_input_apply_ms"), Some(2));
        assert_eq!(trace.last("find_query_len"), Some(3.0));
    }
}
