//! Replace microphone and HTTP service boundaries; drive the production app
//! with real input, real WAV uploads, real edits, and real file saves.
mod support;

use std::{
    collections::VecDeque,
    ffi::{OsStr, OsString},
    io::{Read, Write},
    net::TcpListener,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
use support::{secs, EditorTestExt, TestResult};

struct Reply {
    status: u16,
    text: &'static str,
    gate: Option<PathBuf>,
}
impl Reply {
    fn ok(text: &'static str) -> Self {
        Self {
            status: 200,
            text,
            gate: None,
        }
    }
}
struct Service {
    port: String,
    requests: Arc<Mutex<Vec<Vec<u8>>>>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Service {
    fn new(replies: Vec<Reply>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port().to_string();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let received = requests.clone();
        let thread = thread::spawn(move || {
            let mut replies: VecDeque<_> = replies.into();
            while !stopped.load(Ordering::Acquire) {
                let (mut stream, _) = match listener.accept() {
                    Ok(pair) => pair,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                        continue;
                    }
                    Err(error) => panic!("{error}"),
                };
                stream.set_read_timeout(Some(secs(5))).unwrap();
                let mut request = Vec::new();
                let end;
                loop {
                    let mut bytes = [0; 8192];
                    let len = stream.read(&mut bytes).unwrap();
                    assert!(len > 0);
                    request.extend_from_slice(&bytes[..len]);
                    if let Some(index) = request.windows(4).position(|s| s == b"\r\n\r\n") {
                        end = index + 4;
                        break;
                    }
                }
                let header = String::from_utf8_lossy(&request[..end]).to_lowercase();
                let len: usize = header
                    .lines()
                    .find_map(|s| s.strip_prefix("content-length: "))
                    .unwrap()
                    .parse()
                    .unwrap();
                while request.len() < end + len {
                    let mut bytes = [0; 8192];
                    let n = stream.read(&mut bytes).unwrap();
                    assert!(n > 0);
                    request.extend_from_slice(&bytes[..n]);
                }
                received.lock().unwrap().push(request);
                let reply = replies.pop_front().expect("unexpected duplicate request");
                let deadline = Instant::now() + secs(20);
                while reply.gate.as_ref().is_some_and(|path| path.exists()) && !stopped.load(Ordering::Acquire) {
                    assert!(Instant::now() < deadline, "fixture gate timed out");
                    thread::sleep(Duration::from_millis(10));
                }
                let body = if reply.status == 200 {
                    serde_json::json!({"text": reply.text})
                } else {
                    serde_json::json!({"detail": {"message": reply.text}})
                }
                .to_string();
                let response = format!("HTTP/1.1 {} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", reply.status, body.len(), body);
                let _ = stream.write_all(response.as_bytes());
            }
        });
        Self {
            port,
            requests,
            stop,
            thread: Some(thread),
        }
    }
    fn wait_requests(&self, count: usize) {
        let deadline = Instant::now() + secs(5);
        while self.requests.lock().unwrap().len() < count {
            assert!(Instant::now() < deadline, "request did not reach fixture");
            thread::sleep(Duration::from_millis(10));
        }
    }
    fn assert_uploads(&self, count: usize) {
        let requests = self.requests.lock().unwrap();
        assert_eq!(requests.len(), count);
        for request in requests.iter() {
            let text = String::from_utf8_lossy(request);
            assert!(text.contains("name=\"model_id\"\r\n\r\nscribe_v2"));
            assert!(text.contains("name=\"no_verbatim\"\r\n\r\ntrue"));
            assert!(text.contains("name=\"tag_audio_events\"\r\n\r\nfalse"));
            assert!(request.windows(4).any(|bytes| bytes == b"RIFF"));
            assert!(text.contains("xi-api-key: fixture-key"));
        }
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.thread.take() {
            if !thread::panicking() {
                worker.join().unwrap();
            }
        }
    }
}

fn configure(
    session: &support::ScratchpadSession,
    server: &Service,
) -> support::SupportResult<Vec<(OsString, OsString)>> {
    session.seed_settings(
        r#"version = 1
[keybindings]
"voice.new_note" = ["ctrl-alt-1"]
"voice.pause_resume" = ["ctrl-alt-2"]
"voice.finish" = ["ctrl-alt-3"]
"voice.cancel" = ["ctrl-alt-4"]
"voice.retry" = ["ctrl-alt-5"]
"#,
    )?;
    let path = session.root().join("microphone.wav");
    let mut wav = hound::WavWriter::create(
        &path,
        hound::WavSpec {
            channels: 1,
            sample_rate: 16000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )?;
    for i in 0..8000 {
        wav.write_sample(((i as f32 / 20.0).sin() * 2000.0) as i16)?;
    }
    wav.finalize()?;
    Ok(vec![
        ("ELEVENLABS_API_KEY".into(), "fixture-key".into()),
        ("LST_TEST_VOICE_AUDIO".into(), path.into_os_string()),
        ("LST_TEST_VOICE_PORT".into(), server.port.clone().into()),
    ])
}
fn env_refs(env: &[(OsString, OsString)]) -> Vec<(&OsStr, &OsStr)> {
    env.iter().map(|(k, v)| (k.as_os_str(), v.as_os_str())).collect()
}
fn voice(
    editor: &mut lst_x11_harness::Editor<'_>,
    text: &str,
) -> support::SupportResult<lst_x11_harness::StateTraceRecord> {
    editor.wait_state(text, secs(10), |state| {
        state
            .voice_status
            .as_deref()
            .is_some_and(|status| status.contains(text))
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn voice_segments_preserve_edits_and_follow_the_note_across_tabs() -> TestResult {
    support::run_x11_test("voice-segments", |session| {
        let gate = session.seed_file("gate", "wait")?;
        let server = Service::new(vec![
            Reply::ok("First."),
            Reply {
                gate: Some(gate.clone()),
                ..Reply::ok("Second.")
            },
        ]);
        let env = configure(session, &server)?;
        let (mut editor, original) = session.open_with_env("scratch", &env_refs(&env))?;
        editor.keys("<C-A-1>")?;
        let state = voice(&mut editor, "Recording")?;
        let note = PathBuf::from(state.active_tab_path.unwrap());
        assert!(note.parent().unwrap().ends_with("audio-notes"));
        editor.keys("<C-A-2>")?;
        editor.expect_file(&note, "First.")?;
        editor.keys("<C-home>Edited <C-A-2>")?;
        voice(&mut editor, "Recording")?;
        editor.keys("<C-A-2>")?;
        voice(&mut editor, "Transcribing")?;
        // The second upload is held at the gate, so the tab switch and typing
        // happen while it is in flight.
        server.wait_requests(2);
        editor.keys("<C-tab>Other tab")?;
        std::fs::remove_file(gate)?;
        editor.expect_file(&note, "Edited First. Second.")?;
        editor.save_then_expect_file(&original, "Other tab")?;
        editor.keys("<C-A-3>")?;
        voice(&mut editor, "Dictation finished")?;
        editor.keys("<C-tab><C-z>")?;
        editor.save_then_expect_file(&note, "Edited First.")?;
        server.assert_uploads(2);
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn launch_dictation_retries_retained_audio_and_blocks_premature_close() -> TestResult {
    support::run_x11_test("voice-retry", |session| {
        let gate = session.seed_file("gate", "wait")?;
        let server = Service::new(vec![
            Reply {
                status: 503,
                text: "Try again",
                gate: Some(gate.clone()),
            },
            Reply::ok("Recovered."),
            Reply::ok("Then more."),
        ]);
        let env = configure(session, &server)?;
        let (mut editor, note) = session.open_dictation_with_env("dictate", &env_refs(&env))?;
        assert_eq!(editor.read_state()?.active_tab_index, 0);
        editor.keys("<C-tab>")?;
        assert_eq!(editor.read_state()?.active_tab_path.as_deref(), note.to_str());
        editor.keys("<C-w>")?;
        voice(&mut editor, "Finish or cancel")?;
        editor.keys("<C-A-2>")?;
        voice(&mut editor, "Transcribing")?;
        server.wait_requests(1);
        editor.keys("<C-A-2>")?;
        voice(&mut editor, "Recording")?;
        editor.keys("<C-A-3>")?;
        voice(&mut editor, "2 pending")?;
        std::fs::remove_file(gate)?;
        voice(&mut editor, "Audio retained")?;
        assert_eq!(std::fs::read_to_string(&note)?, "");
        editor.keys("<C-A-5>")?;
        voice(&mut editor, "Dictation finished")?;
        editor.expect_file(&note, "Recovered. Then more.")?;
        editor.keys("<C-z>")?;
        editor.save_then_expect_file(&note, "Recovered.")?;
        editor.keys("<C-z>")?;
        editor.save_then_expect_file(&note, "")?;
        editor.keys("<C-n>")?;
        let scratch = editor.wait_state("ordinary scratchpad keeps its own directory", secs(5), |state| {
            state
                .active_tab_path
                .as_deref()
                .is_some_and(|path| PathBuf::from(path).parent().unwrap().ends_with(".local/share/lst"))
        })?;
        assert_ne!(scratch.active_tab_path.as_deref(), note.to_str());
        server.assert_uploads(3);
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn cancelled_response_cannot_enter_a_new_recording_session() -> TestResult {
    support::run_x11_test("voice-cancel", |session| {
        let gate = session.seed_file("gate", "wait")?;
        let server = Service::new(vec![
            Reply {
                gate: Some(gate.clone()),
                ..Reply::ok("Discarded.")
            },
            Reply::ok("Kept."),
        ]);
        let env = configure(session, &server)?;
        let (mut editor, _) = session.open_with_env("scratch", &env_refs(&env))?;
        editor.keys("<C-A-1>")?;
        let first = PathBuf::from(voice(&mut editor, "Recording")?.active_tab_path.unwrap());
        editor.keys("<C-A-3>")?;
        voice(&mut editor, "Transcribing")?;
        server.wait_requests(1);
        editor.keys("<C-A-4><C-A-1>")?;
        let second = PathBuf::from(voice(&mut editor, "Recording")?.active_tab_path.unwrap());
        assert_ne!(first, second);
        std::fs::remove_file(gate)?;
        editor.keys("<C-A-3>")?;
        voice(&mut editor, "Dictation finished")?;
        editor.expect_file(&second, "Kept.")?;
        assert_eq!(std::fs::read_to_string(first)?, "");
        server.assert_uploads(2);
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn microphone_failure_can_resume_without_losing_the_note() -> TestResult {
    support::run_x11_test("voice-microphone", |session| {
        let server = Service::new(vec![Reply::ok("Microphone recovered.")]);
        let mut env = configure(session, &server)?;
        let original = session.root().join("microphone.wav");
        let missing = session.root().join("missing.wav");
        env.iter_mut().find(|(k, _)| k == "LST_TEST_VOICE_AUDIO").unwrap().1 = missing.clone().into_os_string();
        let (mut editor, _) = session.open_with_env("scratch", &env_refs(&env))?;
        editor.keys("<C-A-1>")?;
        let note = PathBuf::from(voice(&mut editor, "Recording failed")?.active_tab_path.unwrap());
        std::fs::copy(original, missing)?;
        editor.keys("<C-A-2>")?;
        voice(&mut editor, "Recording")?;
        editor.keys("<C-A-3>")?;
        voice(&mut editor, "Dictation finished")?;
        editor.expect_file(&note, "Microphone recovered.")?;
        server.assert_uploads(1);
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn missing_credentials_leave_the_document_and_microphone_alone() -> TestResult {
    support::run_x11_test("voice-no-key", |session| {
        let server = Service::new(vec![]);
        let mut env = configure(session, &server)?;
        env.iter_mut().find(|(k, _)| k == "ELEVENLABS_API_KEY").unwrap().1 = "".into();
        let (mut editor, original) = session.open_with_env("scratch", &env_refs(&env))?;
        editor.keys("Keep me<C-A-1>")?;
        voice(&mut editor, "Set ELEVENLABS_API_KEY")?;
        assert_eq!(editor.read_state()?.active_tab_path.as_deref(), original.to_str());
        editor.save_then_expect_file(&original, "Keep me")?;
        server.assert_uploads(0);
        Ok(())
    })
}

fn click_voice(editor: &mut lst_x11_harness::Editor<'_>, id: &str) -> support::SupportResult<()> {
    let state = editor.wait_state("voice button laid out", secs(3), |r| {
        r.voice_button_bounds_px.contains_key(id)
    })?;
    let (x, y, w, h) = state.voice_button_bounds_px[id];
    editor.click_at(
        ((x + w / 2.0) * state.viewport.scale_factor).round() as i32,
        ((y + h / 2.0) * state.viewport.scale_factor).round() as i32,
    )?;
    Ok(())
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn failed_finalization_after_finish_keeps_audio_and_allows_closing() -> TestResult {
    support::run_x11_test("voice-finalize-failure", |session| {
        let server = Service::new(vec![]);
        let mut env = configure(session, &server)?;
        env.push(("LST_TEST_VOICE_FINALIZE_ERROR".into(), "1".into()));
        let fixture = std::fs::read(session.root().join("microphone.wav"))?;
        let (mut editor, _) = session.open_with_env("scratch", &env_refs(&env))?;
        editor.keys("<C-A-1>")?;
        let state = voice(&mut editor, "Recording")?;
        let note = PathBuf::from(state.active_tab_path.unwrap());
        editor.keys("<C-A-3>")?;
        let failed = voice(&mut editor, "not transcribed")?;
        assert!(failed.voice_status.unwrap().contains("Dictation finished"));
        let audio: Vec<_> = std::fs::read_dir(note.parent().unwrap())?
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "wav"))
            .collect();
        assert_eq!(audio.len(), 1);
        assert_eq!(std::fs::read(&audio[0])?, fixture);
        editor.keys("<C-w>")?;
        editor.wait_state("finished note can close", secs(3), |r| {
            r.active_tab_id != state.active_tab_id
        })?;
        assert!(audio[0].exists(), "closing the note must preserve recovery bytes");
        assert_eq!(server.requests.lock().unwrap().len(), 0);
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn dismissing_voice_message_returns_typing_to_the_editor() -> TestResult {
    support::run_x11_test("voice-dismiss-focus", |session| {
        let server = Service::new(vec![]);
        let env = configure(session, &server)?;
        let (mut editor, path) = session.open_with_env("scratch", &env_refs(&env))?;
        editor.keys("<C-A-1>")?;
        voice(&mut editor, "Recording")?;
        editor.keys("<C-A-4><C-tab>")?;
        voice(&mut editor, "cancelled")?;
        click_voice(&mut editor, "voice-dismiss")?;
        editor.wait_state("voice message dismissed with editor focus", secs(3), |r| {
            r.voice_status.is_none() && r.focused_input == "editor"
        })?;
        editor.keys("typing works")?;
        editor.save_then_expect_file(&path, "typing works")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn arriving_transcript_preserves_status_and_external_primary_selection() -> TestResult {
    use lst_x11_harness::clipboard::{wait_clipboard_text, write_clipboard_text, Selection};
    support::run_x11_test("voice-background-effects", |session| {
        let gate = session.seed_file("gate", "wait")?;
        let server = Service::new(vec![Reply {
            gate: Some(gate.clone()),
            ..Reply::ok("Arrived.")
        }]);
        let env = configure(session, &server)?;
        let settings = session.root().join("config/lst/config.toml");
        let (mut editor, _) = session.open_with_env("scratch", &env_refs(&env))?;
        editor.keys("<C-A-1>")?;
        let state = voice(&mut editor, "Recording")?;
        let note = PathBuf::from(state.active_tab_path.unwrap());
        editor.keys("Selected<C-a><C-A-2>")?;
        server.wait_requests(1);
        write_clipboard_text(Selection::Primary, "another application's selection")?;
        std::fs::write(settings, "not valid [ toml")?;
        let failed = editor.wait_state("unrelated settings error", secs(5), |r| {
            r.status_message.contains("Settings reload failed")
        })?;
        std::fs::remove_file(gate)?;
        editor.expect_file(&note, "Selected Arrived.")?;
        let after = editor.read_state()?;
        assert_eq!(after.status_message, failed.status_message);
        assert_eq!(
            serde_json::to_value(&after.cursors)?,
            serde_json::to_value(&failed.cursors)?
        );
        wait_clipboard_text(Selection::Primary, "another application's selection", secs(1))?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn prompt_review_blocks_voice_and_conflict_buttons() -> TestResult {
    use std::os::unix::fs::PermissionsExt;
    support::run_x11_test("voice-review-modal", |session| {
        let server = Service::new(vec![]);
        let mut env = configure(session, &server)?;
        let filter = session.seed_file(
            "prompt-add",
            "#!/usr/bin/python3\nimport sys\nsys.stdin.read()\nsys.stdout.write('polished')\n",
        )?;
        std::fs::set_permissions(&filter, std::fs::Permissions::from_mode(0o755))?;
        let mut paths = vec![session.root().to_path_buf()];
        paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()));
        env.push(("PATH".into(), std::env::join_paths(paths)?));
        let path = session.seed_file("prompt.txt", "original")?;
        let mut editor = session.open_file_with_env("prompt", &path, &env_refs(&env))?;
        editor.keys("<C-A-1>")?;
        voice(&mut editor, "Recording")?;
        editor.keys("<C-tab><C-end> dirty")?;
        std::fs::write(&path, "changed on disk")?;
        editor.wait_state("conflict banner visible", secs(5), |r| {
            r.file_conflict_button_bounds_px.reload.is_some()
        })?;
        editor.keys("<C-a>")?;
        editor.click_cleanup_button()?;
        let review = editor.wait_state("review ready", secs(5), |r| r.prompt_review_view.is_some())?;
        click_voice(&mut editor, "voice-show")?;
        editor.keys("<tab>")?;
        let after_show = editor.wait_state("review still receives keys", secs(3), |r| {
            r.prompt_review_view.as_deref() == Some("result")
        })?;
        assert_eq!(after_show.active_tab_id, review.active_tab_id);
        assert_eq!(after_show.revision, review.revision);
        click_voice(&mut editor, "voice-cancel")?;
        let (x, y, w, h) = after_show.file_conflict_button_bounds_px.reload.unwrap();
        let scale = after_show.viewport.scale_factor;
        editor.click_at(
            ((x + w / 2.0) * scale).round() as i32,
            ((y + h / 2.0) * scale).round() as i32,
        )?;
        editor.keys("<tab>")?;
        let after_reload = editor.wait_state("review remains intact", secs(3), |r| {
            r.prompt_review_view.as_deref() == Some("changes")
        })?;
        assert_eq!(after_reload.revision, review.revision);
        assert!(after_reload.voice_status.as_deref().unwrap().contains("Recording"));
        assert!(after_reload.file_conflict_path.is_some());
        // The live microphone meter redraws every 100 ms. Wait within that cadence.
        editor.wait_quiet(Duration::from_millis(30), secs(3))?;
        // Keep an inspectable image of the disabled controls around the review.
        editor.screenshot()?.write_ppm(path.with_extension("review.ppm"))?;
        editor.keys("<esc>")?;
        click_voice(&mut editor, "voice-cancel")?;
        voice(&mut editor, "cancelled")?;
        Ok(())
    })
}
