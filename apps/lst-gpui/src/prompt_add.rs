//! The prompt-add filter owns editorial behavior, provider access, and persistent history.
use std::io::{self, Read, Write};
use std::os::{fd::AsRawFd, unix::process::CommandExt};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

const TIMEOUT: Duration = Duration::from_secs(60);

pub(crate) struct Rewrite {
    pub text: String,
    pub warning: String,
}

pub(crate) fn rewrite(source: &str) -> Result<Rewrite, String> {
    rewrite_with_command(
        Command::new("prompt-add").args(["-", "--label", "lst", "--no-clipboard"]),
        source,
        TIMEOUT,
    )
}

fn rewrite_with_command(command: &mut Command, source: &str, timeout: Duration) -> Result<Rewrite, String> {
    let child = command
        .process_group(0)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Could not start prompt-add: {error}. Install it and make it available on PATH."))?;
    let (output, written) = RunningFilter { child, finished: false }
        .run(source.as_bytes(), timeout)
        .map_err(|error| format!("prompt-add: {error}"))?;
    let diagnostic = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !output.status.success() {
        return Err(if diagnostic.is_empty() {
            format!("prompt-add exited with {}", output.status)
        } else {
            diagnostic
        });
    }
    written.map_err(|error| format!("Could not send text to prompt-add: {error}"))?;
    let text = String::from_utf8(output.stdout).map_err(|_| "prompt-add returned invalid UTF-8".to_string())?;
    if text.trim().is_empty() {
        return Err("prompt-add returned an empty message".to_string());
    }
    let warning = diagnostic
        .lines()
        .filter(|line| !matches!(*line, "Rewritten" | "Unchanged"))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(Rewrite { text, warning })
}

// Own cleanup across every return path, including I/O setup errors and timeouts.
// A private process group also lets us terminate helpers retaining pipe handles.
struct RunningFilter {
    child: Child,
    finished: bool,
}

impl RunningFilter {
    fn run(mut self, source: &[u8], timeout: Duration) -> io::Result<(Output, io::Result<()>)> {
        let output = exchange(&mut self.child, source, timeout)?;
        self.finished = true;
        Ok(output)
    }
}

impl Drop for RunningFilter {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        // SAFETY: spawn assigned this child's PID as its private process group.
        unsafe {
            libc::kill(-(self.child.id() as libc::pid_t), libc::SIGKILL);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn nonblocking(pipe: &impl AsRawFd) -> io::Result<()> {
    // SAFETY: the borrowed pipe owns a live descriptor throughout these calls.
    let flags = unsafe { libc::fcntl(pipe.as_raw_fd(), libc::F_GETFL) };
    if flags == -1 || unsafe { libc::fcntl(pipe.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

// Read one bounded chunk per turn so continuous output cannot starve the deadline
// or the other pipe. EOF closes the handle; WouldBlock leaves it for the next turn.
fn read_chunk(pipe: &mut Option<impl Read>, output: &mut Vec<u8>) -> io::Result<bool> {
    let Some(reader) = pipe else {
        return Ok(false);
    };
    let mut bytes = [0; 8192];
    match reader.read(&mut bytes) {
        Ok(0) => {
            *pipe = None;
            Ok(true)
        }
        Ok(count) => {
            output.extend_from_slice(&bytes[..count]);
            Ok(true)
        }
        Err(error) if matches!(error.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted) => Ok(false),
        Err(error) => Err(error),
    }
}

fn exchange(child: &mut Child, source: &[u8], timeout: Duration) -> io::Result<(Output, io::Result<()>)> {
    let deadline = Instant::now() + timeout;
    let mut stdin = child.stdin.take();
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    nonblocking(stdin.as_ref().expect("piped stdin"))?;
    nonblocking(stdout.as_ref().expect("piped stdout"))?;
    nonblocking(stderr.as_ref().expect("piped stderr"))?;
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let mut sent = 0;
    let mut written = Ok(());
    loop {
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("timed out after {} seconds", timeout.as_secs_f64()),
            ));
        }
        let mut progressed = read_chunk(&mut stdout, &mut out)?;
        progressed |= read_chunk(&mut stderr, &mut err)?;
        if sent == source.len() {
            stdin = None;
        }
        if let Some(writer) = stdin.as_mut() {
            match writer.write(&source[sent..source.len().min(sent + 8192)]) {
                Ok(0) => {
                    written = Err(io::Error::new(io::ErrorKind::WriteZero, "filter closed stdin"));
                    stdin = None;
                }
                Ok(count) => {
                    sent += count;
                    progressed = true;
                }
                Err(error) if matches!(error.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted) => {}
                Err(error) => {
                    written = Err(error);
                    stdin = None;
                }
            }
        }
        // Do not reap while a helper may still own a pipe: retaining the child's
        // PID prevents process-group ID reuse before timeout cleanup.
        if stdout.is_none() && stderr.is_none() && stdin.is_none() {
            if let Some(status) = child.try_wait()? {
                return Ok((
                    Output {
                        status,
                        stdout: out,
                        stderr: err,
                    },
                    written,
                ));
            }
        }
        if !progressed {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deadline_covers_blocked_input_inherited_pipes_and_closed_pipes() {
        for script in ["sleep 60", "sleep 60 & exit 0", "exec 0<&- 1>&- 2>&-; sleep 60"] {
            let started = Instant::now();
            let result = rewrite_with_command(
                Command::new("sh").args(["-c", script]),
                &"x".repeat(1024 * 1024),
                Duration::from_millis(100),
            );
            assert!(matches!(result, Err(ref error) if error.contains("timed out")));
            assert!(started.elapsed() < Duration::from_secs(3));
        }
    }

    #[test]
    fn timeout_reaps_the_direct_child() {
        let child = Command::new("sleep")
            .arg("60")
            .process_group(0)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let pid = child.id() as libc::pid_t;
        let filter = RunningFilter { child, finished: false };
        assert_eq!(
            filter.run(b"text", Duration::from_millis(100)).unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        // SAFETY: waitpid is queried only for the direct child created above.
        assert_eq!(unsafe { libc::waitpid(pid, std::ptr::null_mut(), libc::WNOHANG) }, -1);
        assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::ECHILD));
    }

    #[test]
    fn drains_both_output_pipes_before_writing_large_input() {
        let source = "a".repeat(256 * 1024);
        let result = rewrite_with_command(Command::new("python3").args(["-c",
            "import sys; sys.stdout.write('x'*131072); sys.stdout.flush(); sys.stderr.write('w'*131072); sys.stderr.flush(); sys.stdout.write(sys.stdin.read())"
        ]), &source, Duration::from_secs(5)).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(result.text, format!("{}{source}", "x".repeat(131072)));
        assert_eq!(result.warning, "w".repeat(131072));
    }

    #[test]
    fn early_failure_reports_stderr_instead_of_broken_pipe() {
        let result = rewrite_with_command(
            Command::new("sh").args(["-c", "echo provider-unavailable >&2; exit 1"]),
            &"a".repeat(1024 * 1024),
            Duration::from_secs(2),
        );
        assert!(matches!(result, Err(ref error) if error == "provider-unavailable"));
    }
}
