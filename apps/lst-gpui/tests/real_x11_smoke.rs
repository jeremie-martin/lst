use std::{
    env,
    error::Error,
    ffi::OsStr,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const TEXT: &str = "quit clipboard smoke";
const PRIMARY_TEXT: &str = "middle paste smoke";

#[test]
#[ignore = "requires a real X11 display plus xclip and xdotool"]
fn real_x11_scratchpad_clipboards_and_primary_smoke() -> Result<(), Box<dyn Error>> {
    require_env("DISPLAY")?;
    require_tool("xdotool")?;
    require_tool("xclip")?;

    let root = temp_dir("lst-real-x11-smoke")?;

    let empty_dir = root.join("empty");
    fs::create_dir(&empty_dir)?;
    let mut empty = RunningEditor::spawn("empty", &empty_dir)?;
    let empty_path = wait_for_single_file(&empty_dir, Duration::from_secs(10))?;
    empty.focus()?;
    empty.close_active_tab()?;
    empty.wait_for_exit(Duration::from_secs(10))?;
    assert!(
        !empty_path.exists(),
        "closing an empty scratchpad should remove {}",
        empty_path.display()
    );
    assert_eq!(file_count(&empty_dir)?, 0);

    let text_path = root.join("quit-source.txt");
    fs::write(&text_path, TEXT)?;
    let mut text = RunningEditor::spawn_file("text", &text_path)?;
    text.focus()?;
    text.quit()?;
    text.wait_for_exit(Duration::from_secs(10))?;
    wait_for_xclip_text("clipboard", TEXT, Duration::from_secs(10))?;
    wait_for_xclip_text("primary", TEXT, Duration::from_secs(10))?;

    let primary_dir = root.join("primary");
    fs::create_dir(&primary_dir)?;
    let mut primary = RunningEditor::spawn("primary", &primary_dir)?;
    let primary_path = wait_for_single_file(&primary_dir, Duration::from_secs(10))?;
    if primary.is_visible()? {
        write_xclip_text("primary", PRIMARY_TEXT)?;
        primary.focus()?;
        primary.middle_click_editor()?;
        primary.save()?;
        wait_for_file_text(&primary_path, PRIMARY_TEXT, Duration::from_secs(10))?;
    } else {
        eprintln!(
            "skipping middle-click PRIMARY paste check because the X11 window is not viewable"
        );
    }
    primary.quit()?;
    primary.wait_for_exit(Duration::from_secs(10))?;

    fs::remove_dir_all(root)?;
    Ok(())
}

struct RunningEditor {
    child: Child,
    window: String,
}

impl RunningEditor {
    fn spawn(label: &str, scratchpad_dir: &Path) -> Result<Self, Box<dyn Error>> {
        Self::spawn_with_args(
            label,
            [OsStr::new("--scratchpad-dir"), scratchpad_dir.as_os_str()],
        )
    }

    fn spawn_file(label: &str, path: &Path) -> Result<Self, Box<dyn Error>> {
        Self::spawn_with_args(label, [path.as_os_str()])
    }

    fn spawn_with_args<I, S>(label: &str, args: I) -> Result<Self, Box<dyn Error>>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let title = format!("lst-real-x11-smoke-{label}-{}", unique_id());
        let mut child = Command::new(editor_binary()?)
            .arg("--title")
            .arg(&title)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()?;

        let window = wait_for_window(child.id(), &title, &mut child, Duration::from_secs(10))?;
        Ok(Self { child, window })
    }

    fn is_visible(&self) -> Result<bool, Box<dyn Error>> {
        let output = Command::new("xwininfo")
            .args(["-id", &self.window])
            .output()?;
        if !output.status.success() {
            return Ok(false);
        }
        Ok(String::from_utf8_lossy(&output.stdout).contains("Map State: IsViewable"))
    }

    fn focus(&self) -> Result<(), Box<dyn Error>> {
        run_command(
            "xdotool",
            [
                "mousemove",
                "--window",
                &self.window,
                "300",
                "300",
                "click",
                "1",
            ],
        )?;
        thread::sleep(Duration::from_millis(100));
        Ok(())
    }

    fn save(&self) -> Result<(), Box<dyn Error>> {
        run_command("xdotool", ["key", "--window", &self.window, "ctrl+s"])?;
        Ok(())
    }

    fn middle_click_editor(&self) -> Result<(), Box<dyn Error>> {
        run_command(
            "xdotool",
            [
                "mousemove",
                "--window",
                &self.window,
                "160",
                "170",
                "click",
                "2",
            ],
        )?;
        Ok(())
    }

    fn close_active_tab(&self) -> Result<(), Box<dyn Error>> {
        let _ = Command::new("xdotool")
            .args(["key", "--window", &self.window, "ctrl+w"])
            .stderr(Stdio::null())
            .status()?;
        Ok(())
    }

    fn quit(&self) -> Result<(), Box<dyn Error>> {
        let _ = Command::new("xdotool")
            .args(["key", "--window", &self.window, "ctrl+q"])
            .stderr(Stdio::null())
            .status()?;
        Ok(())
    }

    fn wait_for_exit(&mut self, timeout: Duration) -> Result<(), Box<dyn Error>> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.child.try_wait()? {
                if status.success() {
                    return Ok(());
                }
                return Err(io::Error::other(format!("editor exited with {status}")).into());
            }
            if Instant::now() >= deadline {
                return Err(io::Error::other("timed out waiting for editor to exit").into());
            }
            thread::sleep(Duration::from_millis(25));
        }
    }
}

impl Drop for RunningEditor {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn editor_binary() -> Result<PathBuf, Box<dyn Error>> {
    if let Some(path) = env::var_os("LST_GPUI_BIN") {
        return Ok(PathBuf::from(path));
    }
    if let Some(path) = option_env!("CARGO_BIN_EXE_lst") {
        return Ok(PathBuf::from(path));
    }
    if let Some(path) = option_env!("CARGO_BIN_EXE_lst-gpui") {
        return Ok(PathBuf::from(path));
    }

    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    for name in ["lst", "lst-gpui"] {
        let fallback = manifest_dir.join("../../target/debug").join(name);
        if fallback.exists() {
            return Ok(fallback);
        }
    }

    Err(io::Error::other(
        "could not find lst binary; run `cargo build -p lst-gpui --bin lst` or set LST_GPUI_BIN",
    )
    .into())
}

fn wait_for_window(
    pid: u32,
    title: &str,
    child: &mut Child,
    timeout: Duration,
) -> Result<String, Box<dyn Error>> {
    let deadline = Instant::now() + timeout;
    let pid = pid.to_string();
    loop {
        if let Some(status) = child.try_wait()? {
            return Err(io::Error::other(format!(
                "editor exited before its window appeared: {status}"
            ))
            .into());
        }

        let output = Command::new("xdotool")
            .args(["search", "--pid", &pid, "--name", title])
            .output()?;
        if output.status.success() {
            if let Some(window) = String::from_utf8_lossy(&output.stdout)
                .lines()
                .next()
                .filter(|window| !window.is_empty())
            {
                return Ok(window.to_string());
            }
        }

        if Instant::now() >= deadline {
            return Err(io::Error::other("timed out waiting for editor window").into());
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn wait_for_single_file(dir: &Path, timeout: Duration) -> Result<PathBuf, Box<dyn Error>> {
    let deadline = Instant::now() + timeout;
    loop {
        let files = read_files(dir)?;
        if files.len() == 1 {
            return Ok(files[0].clone());
        }
        if Instant::now() >= deadline {
            return Err(io::Error::other(format!(
                "timed out waiting for one scratchpad file in {}; found {}",
                dir.display(),
                files.len()
            ))
            .into());
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn wait_for_file_text(
    path: &Path,
    expected: &str,
    timeout: Duration,
) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + timeout;
    loop {
        if fs::read_to_string(path).unwrap_or_default() == expected {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(io::Error::other(format!(
                "timed out waiting for {} to contain expected text",
                path.display()
            ))
            .into());
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn wait_for_xclip_text(
    selection: &str,
    expected: &str,
    timeout: Duration,
) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + timeout;
    loop {
        if read_xclip_text(selection).as_deref() == Some(expected) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(io::Error::other(format!(
                "timed out waiting for X11 {selection} selection"
            ))
            .into());
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn read_xclip_text(selection: &str) -> Option<String> {
    let output = Command::new("xclip")
        .args(["-selection", selection, "-o"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn write_xclip_text(selection: &str, text: &str) -> Result<(), Box<dyn Error>> {
    let mut child = Command::new("xclip")
        .args(["-selection", selection, "-in"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(text.as_bytes())?;
    }
    let status = child.wait()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("xclip exited with {status}")).into())
    }
}

fn read_files(dir: &Path) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut files = fs::read_dir(dir)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    files.retain(|path| path.is_file());
    files.sort();
    Ok(files)
}

fn file_count(dir: &Path) -> Result<usize, Box<dyn Error>> {
    Ok(read_files(dir)?.len())
}

fn run_command<I, S>(program: &str, args: I) -> Result<(), Box<dyn Error>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let status = Command::new(program).args(args).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("{program} exited with {status}")).into())
    }
}

fn require_env(name: &str) -> Result<(), Box<dyn Error>> {
    if env::var_os(name).is_some() {
        Ok(())
    } else {
        Err(io::Error::other(format!("{name} must be set")).into())
    }
}

fn require_tool(name: &str) -> Result<(), Box<dyn Error>> {
    let status = Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {name} >/dev/null 2>&1"))
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("{name} is required")).into())
    }
}

fn temp_dir(label: &str) -> Result<PathBuf, Box<dyn Error>> {
    let dir = env::temp_dir().join(format!("{label}-{}", unique_id()));
    fs::create_dir(&dir)?;
    Ok(dir)
}

fn unique_id() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}
