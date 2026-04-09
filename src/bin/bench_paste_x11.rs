use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use x11rb::connection::Connection as _;
use x11rb::protocol::damage::{self, ConnectionExt as _};
use x11rb::protocol::xkb::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{self, AtomEnum, ConnectionExt as _, MapState};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;
use x11rb::NONE;

const SCENARIO: &str = "paste_growth_x11_real";
const CORPUS_REL: &str = "benchmarks/paste-corpus.rs";
const WINDOW_REQUESTED_LOGICAL: &str = "980x680";
const WRAP: &str = "on";
const HIGHLIGHT_DEFAULT: &str = "rust-tree-sitter-line-default";
const PRIMING_RUNS: usize = 1;
const REPETITIONS: usize = 7;
const TRACE_PASTE_COUNT: usize = 10;
const TRACE_TOTAL_MS: u64 = 5_000;
const TRACE_PASTE_INTERVAL_MS: u64 = TRACE_TOTAL_MS / TRACE_PASTE_COUNT as u64;
const INTER_RUN_SLEEP_MS: u64 = 1_000;
const QUIET_MS: u64 = 75;
const WINDOW_DISCOVERY_TIMEOUT_MS: u64 = 10_000;
const TRACE_TIMEOUT_MS: u64 = 10_000;
const POINTER_SETTLE_MS: u64 = 50;
const COPY_SETTLE_MS: u64 = 100;
const FILE_STABLE_MS: u64 = 200;
const FILE_STABLE_TIMEOUT_MS: u64 = 5_000;
const BUTTON_LEFT: u8 = 1;
const KEYSYM_CONTROL_L: u32 = 0xffe3;
const KEYSYM_END: u32 = 0xff57;
const KEYSYM_S: u32 = b's' as u32;
const KEYSYM_V: u32 = b'v' as u32;

fn main() -> Result<(), Box<dyn Error>> {
    let corpus = corpus_info()?;
    let editor = editor_path()?;
    let session_env = resolve_session_env()?;
    apply_session_env(&session_env);

    let (conn, screen_num) = x11rb::connect(Some(session_env.display.as_str()))?;
    conn.damage_query_version(1, 1)?.reply()?;
    conn.xtest_get_version(2, 2)?.reply()?;
    conn.xkb_use_extension(1, 0)?.reply()?;
    let screen = &conn.setup().roots[screen_num];

    let atoms = Atoms::intern(&conn)?;
    let keycodes = Keycodes::resolve(&conn)?;
    let ticks_per_second = clock_ticks_per_second()?;

    let mut runs = Vec::with_capacity(REPETITIONS);
    let mut expected_window = None;
    let mut expected_final = None;

    let total_runs = PRIMING_RUNS + REPETITIONS;
    for run_index in 0..total_runs {
        let measured = run_index >= PRIMING_RUNS;
        let metrics = run_once(
            &conn,
            screen.root,
            &atoms,
            &keycodes,
            &session_env,
            &editor,
            &corpus,
            ticks_per_second,
            run_index,
        )?;

        if let Some((width, height)) = expected_window {
            if (metrics.window_width, metrics.window_height) != (width, height) {
                return Err(io::Error::other(format!(
                    "window size changed during benchmark: expected {width}x{height}, got {}x{}",
                    metrics.window_width, metrics.window_height
                ))
                .into());
            }
        } else {
            expected_window = Some((metrics.window_width, metrics.window_height));
        }

        if metrics.final_file_bytes <= corpus.bytes || metrics.final_file_lines <= corpus.lines {
            return Err(io::Error::other(format!(
                "paste-growth benchmark did not grow the document as expected: initial {} bytes/{} lines, final {} bytes/{} lines",
                corpus.bytes, corpus.lines, metrics.final_file_bytes, metrics.final_file_lines
            ))
            .into());
        }

        if let Some((bytes, lines)) = expected_final {
            if (metrics.final_file_bytes, metrics.final_file_lines) != (bytes, lines) {
                return Err(io::Error::other(format!(
                    "final file size changed across runs: expected {bytes} bytes/{lines} lines, got {} bytes/{} lines",
                    metrics.final_file_bytes, metrics.final_file_lines
                ))
                .into());
            }
        } else {
            expected_final = Some((metrics.final_file_bytes, metrics.final_file_lines));
        }

        if measured {
            runs.push(metrics);
        }

        if run_index + 1 < total_runs {
            thread::sleep(Duration::from_millis(INTER_RUN_SLEEP_MS));
        }
    }

    let (window_width, window_height) =
        expected_window.ok_or_else(|| io::Error::other("benchmark produced no runs"))?;
    let summary = Summary::from_runs(&runs)?;

    println!("scenario={SCENARIO}");
    println!("display={}", session_env.display);
    println!("file={CORPUS_REL}");
    println!("file_bytes={}", corpus.bytes);
    println!("file_lines={}", corpus.lines);
    println!("wrap={WRAP}");
    println!("highlight={}", highlight_label());
    println!("window_requested_logical={WINDOW_REQUESTED_LOGICAL}");
    println!("window_client_px={}x{}", window_width, window_height);
    println!("priming_runs={PRIMING_RUNS}");
    println!("repetitions={REPETITIONS}");
    println!("setup_clipboard_seeded=true");
    println!("setup_move_to_document_end=true");
    println!("trace_paste_count={TRACE_PASTE_COUNT}");
    println!("trace_paste_interval_ms={TRACE_PASTE_INTERVAL_MS}");
    println!("trace_duration_ms={TRACE_TOTAL_MS}");
    println!("inter_run_sleep_ms={INTER_RUN_SLEEP_MS}");
    println!("quiet_ms={QUIET_MS}");
    println!("startup_ms_runs={}", join_csv_f64(&runs, |run| run.startup_ms));
    println!(
        "trace_wall_ms_runs={}",
        join_csv_f64(&runs, |run| run.trace_wall_ms)
    );
    println!("user_cpu_ms_runs={}", join_csv_f64(&runs, |run| run.user_cpu_ms));
    println!("sys_cpu_ms_runs={}", join_csv_f64(&runs, |run| run.sys_cpu_ms));
    println!("cpu_ms_runs={}", join_csv_f64(&runs, |run| run.cpu_ms()));
    println!("peak_rss_mb_runs={}", join_csv_f64(&runs, |run| run.peak_rss_mb));
    println!(
        "final_file_bytes_runs={}",
        join_csv_u64(&runs, |run| run.final_file_bytes)
    );
    println!(
        "final_file_lines_runs={}",
        join_csv_usize(&runs, |run| run.final_file_lines)
    );
    println!("startup_ms={:.3}", summary.startup_ms);
    println!("trace_wall_ms={:.3}", summary.trace_wall_ms);
    println!("user_cpu_ms={:.3}", summary.user_cpu_ms);
    println!("sys_cpu_ms={:.3}", summary.sys_cpu_ms);
    println!("cpu_ms={:.3}", summary.cpu_ms);
    println!("peak_rss_mb={:.3}", summary.peak_rss_mb);
    println!("final_file_bytes={}", summary.final_file_bytes);
    println!("final_file_lines={}", summary.final_file_lines);
    println!("score={:.3}", summary.cpu_ms);

    Ok(())
}

fn highlight_label() -> &'static str {
    match std::env::var("LST_HIGHLIGHT_BACKEND").ok().as_deref() {
        Some("syntect") => "rust-syntect-fallback",
        _ => HIGHLIGHT_DEFAULT,
    }
}

fn run_once(
    conn: &RustConnection,
    root: xproto::Window,
    atoms: &Atoms,
    keycodes: &Keycodes,
    session_env: &SessionEnv,
    editor: &Path,
    corpus: &CorpusInfo,
    ticks_per_second: u64,
    run_index: usize,
) -> Result<RunMetrics, Box<dyn Error>> {
    let working_copy = working_copy_path(run_index);
    fs::write(&working_copy, &corpus.contents)?;
    seed_clipboard(&corpus.contents, session_env)?;

    let title = format!("lst-bench-paste-{}-{run_index}", std::process::id());
    let mut child = spawn_editor(editor, &working_copy, session_env, &title)?;
    let pid = child.id();

    let result = (|| {
        let startup_started = Instant::now();
        debug_phase(run_index, "find_window");
        let window = find_window(
            conn,
            root,
            atoms,
            pid,
            &title,
            &mut child,
            Duration::from_millis(WINDOW_DISCOVERY_TIMEOUT_MS),
        )?;
        let damage =
            damage::DamageWrapper::create(conn, window.id, damage::ReportLevel::NON_EMPTY)?;
        conn.flush()?;
        debug_phase(run_index, "startup_quiet");
        wait_for_damage_quiet(
            conn,
            damage.damage(),
            window.id,
            &mut child,
            Duration::from_millis(QUIET_MS),
            Duration::from_millis(TRACE_TIMEOUT_MS),
        )?;
        let startup_ms = startup_started.elapsed().as_secs_f64() * 1000.0;

        debug_phase(run_index, "focus_window");
        move_pointer_to_window_center(conn, root, &window)?;
        thread::sleep(Duration::from_millis(POINTER_SETTLE_MS));
        inject_button_click(conn, root, BUTTON_LEFT)?;
        debug_phase(run_index, "post_click_quiet");
        wait_for_damage_quiet(
            conn,
            damage.damage(),
            window.id,
            &mut child,
            Duration::from_millis(QUIET_MS),
            Duration::from_millis(TRACE_TIMEOUT_MS),
        )?;

        debug_phase(run_index, "ctrl_end");
        inject_ctrl_chord(conn, root, keycodes.control_l, keycodes.end)?;
        debug_phase(run_index, "post_ctrl_end_quiet");
        wait_for_damage_quiet(
            conn,
            damage.damage(),
            window.id,
            &mut child,
            Duration::from_millis(QUIET_MS),
            Duration::from_millis(TRACE_TIMEOUT_MS),
        )?;

        debug_phase(run_index, "trace_start");
        let before = proc_sample(pid)?;
        let trace_started = Instant::now();
        inject_paste_burst(
            conn,
            root,
            keycodes,
            TRACE_PASTE_COUNT,
            Duration::from_millis(TRACE_TOTAL_MS),
        )?;
        debug_phase(run_index, "post_trace_quiet");
        wait_for_damage_quiet(
            conn,
            damage.damage(),
            window.id,
            &mut child,
            Duration::from_millis(QUIET_MS),
            Duration::from_millis(TRACE_TIMEOUT_MS),
        )?;
        let trace_wall_ms = trace_started.elapsed().as_secs_f64() * 1000.0;
        let after = proc_sample(pid)?;

        debug_phase(run_index, "ctrl_s");
        inject_ctrl_chord(conn, root, keycodes.control_l, keycodes.s)?;
        debug_phase(run_index, "wait_file_stable");
        let final_stats = wait_for_file_stable(
            &working_copy,
            Duration::from_millis(FILE_STABLE_MS),
            Duration::from_millis(FILE_STABLE_TIMEOUT_MS),
        )?;

        Ok(RunMetrics {
            startup_ms,
            trace_wall_ms,
            user_cpu_ms: ticks_to_ms(
                after.utime_ticks.saturating_sub(before.utime_ticks),
                ticks_per_second,
            ),
            sys_cpu_ms: ticks_to_ms(
                after.stime_ticks.saturating_sub(before.stime_ticks),
                ticks_per_second,
            ),
            peak_rss_mb: after.vmhwm_kb as f64 / 1024.0,
            final_file_bytes: final_stats.bytes,
            final_file_lines: final_stats.lines,
            window_width: window.width,
            window_height: window.height,
        })
    })();

    let terminate_result = terminate_child(&mut child);
    let cleanup_result = fs::remove_file(&working_copy);

    if let Err(error) = terminate_result {
        return Err(error);
    }
    if let Err(error) = cleanup_result {
        return Err(error.into());
    }

    result
}

fn working_copy_path(run_index: usize) -> PathBuf {
    std::env::temp_dir().join(format!(
        "lst-bench-paste-{}-{run_index}.rs",
        std::process::id()
    ))
}

fn spawn_editor(
    editor: &Path,
    file: &Path,
    session_env: &SessionEnv,
    title: &str,
) -> Result<Child, Box<dyn Error>> {
    let mut command = Command::new(editor);
    command
        .arg("--title")
        .arg(title)
        .arg(file)
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
        .env("DISPLAY", &session_env.display);

    if let Some(xauthority) = &session_env.xauthority {
        command.env("XAUTHORITY", xauthority);
    }
    if let Some(dbus) = &session_env.dbus_session_bus_address {
        command.env("DBUS_SESSION_BUS_ADDRESS", dbus);
    }

    Ok(command.spawn()?)
}

fn debug_enabled() -> bool {
    std::env::var_os("LST_BENCH_DEBUG").is_some()
}

fn debug_phase(run_index: usize, phase: &str) {
    if debug_enabled() {
        eprintln!("bench_paste_x11 run={run_index} phase={phase}");
    }
}

fn corpus_info() -> Result<CorpusInfo, Box<dyn Error>> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(CORPUS_REL);
    let contents = fs::read_to_string(&path)?;
    let bytes = fs::metadata(&path)?.len();
    let lines = contents.lines().count();
    Ok(CorpusInfo {
        contents,
        bytes,
        lines,
    })
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
        let attrs = conn.get_window_attributes(window)?.reply()?;
        if attrs.map_state == MapState::VIEWABLE {
            let geometry = conn.get_geometry(window)?.reply()?;
            let translated = conn.translate_coordinates(window, root, 0, 0)?.reply()?;
            return Ok(Some(WindowInfo {
                id: window,
                root_x: translated.dst_x,
                root_y: translated.dst_y,
                width: geometry.width,
                height: geometry.height,
            }));
        }
    }

    for child in conn.query_tree(window)?.reply()?.children {
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
    let reply = conn
        .get_property(false, window, atoms.net_wm_pid, AtomEnum::CARDINAL, 0, 1)?
        .reply()?;
    Ok(reply.value32().and_then(|mut values| values.next()))
}

fn window_title(
    conn: &RustConnection,
    window: xproto::Window,
    atoms: &Atoms,
) -> Result<Option<String>, Box<dyn Error>> {
    let utf8 = conn
        .get_property(false, window, atoms.net_wm_name, atoms.utf8_string, 0, 1024)?
        .reply()?;
    if !utf8.value.is_empty() {
        return Ok(Some(String::from_utf8_lossy(&utf8.value).into_owned()));
    }

    let legacy = conn
        .get_property(false, window, AtomEnum::WM_NAME, AtomEnum::STRING, 0, 1024)?
        .reply()?;
    if legacy.value.is_empty() {
        return Ok(None);
    }

    Ok(Some(String::from_utf8_lossy(&legacy.value).into_owned()))
}

fn move_pointer_to_window_center(
    conn: &RustConnection,
    root: xproto::Window,
    window: &WindowInfo,
) -> Result<(), Box<dyn Error>> {
    let x = clamp_i16(i32::from(window.root_x) + i32::from(window.width) / 2);
    let y = clamp_i16(i32::from(window.root_y) + i32::from(window.height) / 2);
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

fn inject_paste_burst(
    conn: &RustConnection,
    root: xproto::Window,
    keycodes: &Keycodes,
    count: usize,
    total_duration: Duration,
) -> Result<(), Box<dyn Error>> {
    let start = Instant::now();

    for index in 0..count {
        if debug_enabled() {
            eprintln!("bench_paste_x11 paste={}/{}", index + 1, count);
        }
        inject_ctrl_chord(conn, root, keycodes.control_l, keycodes.v)?;

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

fn seed_clipboard(contents: &str, session_env: &SessionEnv) -> Result<(), Box<dyn Error>> {
    let mut command = Command::new("xclip");
    command
        .args(["-selection", "clipboard"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("DISPLAY", &session_env.display);

    if let Some(xauthority) = &session_env.xauthority {
        command.env("XAUTHORITY", xauthority);
    }
    if let Some(dbus) = &session_env.dbus_session_bus_address {
        command.env("DBUS_SESSION_BUS_ADDRESS", dbus);
    }

    let mut child = command.spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write as _;
        stdin.write_all(contents.as_bytes())?;
    }

    let status = child.wait()?;
    if !status.success() {
        return Err(io::Error::other("xclip failed to seed clipboard").into());
    }

    thread::sleep(Duration::from_millis(COPY_SETTLE_MS));
    Ok(())
}

fn wait_for_damage_quiet(
    conn: &RustConnection,
    damage_id: damage::Damage,
    window: xproto::Window,
    child: &mut Child,
    quiet_for: Duration,
    timeout: Duration,
) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + timeout;
    let mut last_damage = Instant::now();

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
                    conn.damage_subtract(damage_id, NONE, NONE)?;
                }
            }
        }

        conn.flush()?;

        if last_damage.elapsed() >= quiet_for {
            return Ok(());
        }

        if Instant::now() >= deadline {
            return Err(io::Error::other("timed out waiting for redraw quiet period").into());
        }

        thread::sleep(Duration::from_millis(5));
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

fn wait_for_file_stable(
    path: &Path,
    stable_for: Duration,
    timeout: Duration,
) -> Result<FileStats, Box<dyn Error>> {
    let deadline = Instant::now() + timeout;
    let mut last_stats = read_file_stats(path)?;
    let mut last_change = Instant::now();

    loop {
        let stats = read_file_stats(path)?;
        if stats != last_stats {
            last_stats = stats;
            last_change = Instant::now();
        }

        if last_change.elapsed() >= stable_for {
            return Ok(last_stats);
        }

        if Instant::now() >= deadline {
            return Err(io::Error::other("timed out waiting for file to stabilize").into());
        }

        thread::sleep(Duration::from_millis(20));
    }
}

fn read_file_stats(path: &Path) -> Result<FileStats, Box<dyn Error>> {
    let contents = fs::read_to_string(path)?;
    let bytes = fs::metadata(path)?.len();
    let lines = contents.lines().count();
    Ok(FileStats { bytes, lines })
}

fn editor_path() -> Result<PathBuf, Box<dyn Error>> {
    let current = std::env::current_exe()?;
    let sibling = current
        .parent()
        .ok_or_else(|| io::Error::other("benchmark binary has no parent directory"))?
        .join("lst");
    if sibling.exists() {
        return Ok(sibling);
    }

    Err(io::Error::other(
        "could not find sibling editor binary 'lst'; build both bins with `cargo build --release --bin lst --bin bench_paste_x11`",
    )
    .into())
}

fn resolve_session_env() -> Result<SessionEnv, Box<dyn Error>> {
    if let Ok(display) = std::env::var("DISPLAY") {
        return Ok(SessionEnv {
            display,
            xauthority: std::env::var("XAUTHORITY").ok(),
            dbus_session_bus_address: std::env::var("DBUS_SESSION_BUS_ADDRESS").ok(),
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
    std::env::set_var("DISPLAY", &env.display);
    if let Some(xauthority) = &env.xauthority {
        std::env::set_var("XAUTHORITY", xauthority);
    }
    if let Some(dbus) = &env.dbus_session_bus_address {
        std::env::set_var("DBUS_SESSION_BUS_ADDRESS", dbus);
    }
}

fn clamp_i16(value: i32) -> i16 {
    value.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
}

fn median_f64(values: &[f64]) -> Result<f64, Box<dyn Error>> {
    if values.is_empty() {
        return Err(io::Error::other("cannot compute median of empty sample").into());
    }

    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    Ok(sorted[sorted.len() / 2])
}

fn median_u64(values: &[u64]) -> Result<u64, Box<dyn Error>> {
    if values.is_empty() {
        return Err(io::Error::other("cannot compute median of empty sample").into());
    }

    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    Ok(sorted[sorted.len() / 2])
}

fn median_usize(values: &[usize]) -> Result<usize, Box<dyn Error>> {
    if values.is_empty() {
        return Err(io::Error::other("cannot compute median of empty sample").into());
    }

    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    Ok(sorted[sorted.len() / 2])
}

fn join_csv_f64(runs: &[RunMetrics], selector: impl Fn(&RunMetrics) -> f64) -> String {
    runs.iter()
        .map(|run| format!("{:.3}", selector(run)))
        .collect::<Vec<_>>()
        .join(",")
}

fn join_csv_u64(runs: &[RunMetrics], selector: impl Fn(&RunMetrics) -> u64) -> String {
    runs.iter()
        .map(|run| selector(run).to_string())
        .collect::<Vec<_>>()
        .join(",")
}

fn join_csv_usize(runs: &[RunMetrics], selector: impl Fn(&RunMetrics) -> usize) -> String {
    runs.iter()
        .map(|run| selector(run).to_string())
        .collect::<Vec<_>>()
        .join(",")
}

#[derive(Clone)]
struct SessionEnv {
    display: String,
    xauthority: Option<String>,
    dbus_session_bus_address: Option<String>,
}

struct CorpusInfo {
    contents: String,
    bytes: u64,
    lines: usize,
}

struct ProcSample {
    utime_ticks: u64,
    stime_ticks: u64,
    vmhwm_kb: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
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

struct RunMetrics {
    startup_ms: f64,
    trace_wall_ms: f64,
    user_cpu_ms: f64,
    sys_cpu_ms: f64,
    peak_rss_mb: f64,
    final_file_bytes: u64,
    final_file_lines: usize,
    window_width: u16,
    window_height: u16,
}

impl RunMetrics {
    fn cpu_ms(&self) -> f64 {
        self.user_cpu_ms + self.sys_cpu_ms
    }
}

struct Summary {
    startup_ms: f64,
    trace_wall_ms: f64,
    user_cpu_ms: f64,
    sys_cpu_ms: f64,
    cpu_ms: f64,
    peak_rss_mb: f64,
    final_file_bytes: u64,
    final_file_lines: usize,
}

impl Summary {
    fn from_runs(runs: &[RunMetrics]) -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            startup_ms: median_f64(&runs.iter().map(|run| run.startup_ms).collect::<Vec<_>>())?,
            trace_wall_ms: median_f64(
                &runs.iter().map(|run| run.trace_wall_ms).collect::<Vec<_>>(),
            )?,
            user_cpu_ms: median_f64(&runs.iter().map(|run| run.user_cpu_ms).collect::<Vec<_>>())?,
            sys_cpu_ms: median_f64(&runs.iter().map(|run| run.sys_cpu_ms).collect::<Vec<_>>())?,
            cpu_ms: median_f64(&runs.iter().map(|run| run.cpu_ms()).collect::<Vec<_>>())?,
            peak_rss_mb: median_f64(
                &runs.iter().map(|run| run.peak_rss_mb).collect::<Vec<_>>(),
            )?,
            final_file_bytes: median_u64(
                &runs.iter().map(|run| run.final_file_bytes).collect::<Vec<_>>(),
            )?,
            final_file_lines: median_usize(
                &runs.iter().map(|run| run.final_file_lines).collect::<Vec<_>>(),
            )?,
        })
    }
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
    end: xproto::Keycode,
    s: xproto::Keycode,
    v: xproto::Keycode,
}

impl Keycodes {
    fn resolve(conn: &RustConnection) -> Result<Self, Box<dyn Error>> {
        let setup = conn.setup();
        let count = setup.max_keycode - setup.min_keycode + 1;
        let reply = conn.get_keyboard_mapping(setup.min_keycode, count)?.reply()?;
        let active_group = active_keyboard_group(conn)?;

        Ok(Self {
            control_l: find_keycode(&reply, setup.min_keycode, KEYSYM_CONTROL_L, active_group)?,
            end: find_keycode(&reply, setup.min_keycode, KEYSYM_END, active_group)?,
            s: find_keycode(&reply, setup.min_keycode, KEYSYM_S, active_group)?,
            v: find_keycode(&reply, setup.min_keycode, KEYSYM_V, active_group)?,
        })
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
