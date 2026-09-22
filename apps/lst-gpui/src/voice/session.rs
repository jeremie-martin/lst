//! One capture owner and an ordered queue of immutable audio segments. Only
//! successful queue heads leave the session, so retries cannot duplicate text.
use super::{
    audio::{Audio, Recorder},
    provider::Provider,
};
use lst_editor::TabId;
use std::{
    collections::VecDeque,
    sync::{atomic::Ordering, mpsc, Arc},
    time::{Duration, Instant},
};

enum Capture {
    Recording { recorder: Recorder, since: Instant },
    Stopping { recorder: Recorder, finish: bool },
    Paused,
    Finished,
}

enum Upload {
    Idle,
    Running(mpsc::Receiver<Result<String, String>>),
    Failed(String),
    Ready(String),
}

pub(super) struct Session {
    pub tab: TabId,
    provider: Provider,
    capture: Capture,
    upload: Upload,
    queue: VecDeque<Arc<Audio>>,
    elapsed: Duration,
    pub notice: Option<String>,
}

impl Session {
    pub fn new(tab: TabId, provider: Provider) -> Result<Self, String> {
        Ok(Self {
            tab,
            provider,
            capture: Capture::Recording {
                recorder: Recorder::start()?,
                since: Instant::now(),
            },
            upload: Upload::Idle,
            queue: VecDeque::new(),
            elapsed: Duration::ZERO,
            notice: None,
        })
    }

    pub fn toggle(&mut self) {
        match &self.capture {
            Capture::Recording { recorder, .. } => {
                recorder.stop();
                let capture = std::mem::replace(&mut self.capture, Capture::Paused);
                if let Capture::Recording { recorder, .. } = capture {
                    self.capture = Capture::Stopping {
                        recorder,
                        finish: false,
                    };
                }
            }
            Capture::Paused => {
                if self.queue.len() >= 16 {
                    self.notice = Some("Finish or retry pending segments before recording more.".into());
                    return;
                }
                match Recorder::start() {
                    Ok(recorder) => {
                        self.capture = Capture::Recording {
                            recorder,
                            since: Instant::now(),
                        };
                        self.notice = None;
                    }
                    Err(error) => self.notice = Some(error),
                }
            }
            _ => {}
        }
    }

    pub fn finish(&mut self) {
        self.notice = None;
        self.toggle_if_recording();
        match &mut self.capture {
            Capture::Stopping { finish, .. } => *finish = true,
            Capture::Paused => self.capture = Capture::Finished,
            _ => {}
        }
    }

    fn toggle_if_recording(&mut self) {
        if matches!(self.capture, Capture::Recording { .. }) {
            self.toggle();
        }
    }

    pub fn retry(&mut self) {
        if matches!(self.upload, Upload::Failed(_)) {
            self.upload = Upload::Idle;
        }
    }

    pub fn failed(&self) -> bool {
        matches!(self.upload, Upload::Failed(_))
    }
    pub fn recording(&self) -> bool {
        matches!(self.capture, Capture::Recording { .. })
    }
    pub fn can_toggle(&self) -> bool {
        matches!(self.capture, Capture::Recording { .. } | Capture::Paused)
    }
    pub fn done(&self) -> bool {
        matches!(self.capture, Capture::Finished) && self.queue.is_empty() && matches!(self.upload, Upload::Idle)
    }

    pub fn status(&self) -> String {
        let (state, extra) = match &self.capture {
            Capture::Recording { recorder, since } => {
                if recorder.ready.load(Ordering::Acquire) {
                    ("Recording", since.elapsed())
                } else {
                    ("Starting microphone", Duration::ZERO)
                }
            }
            Capture::Stopping { .. } => ("Stopping microphone", Duration::ZERO),
            Capture::Paused => ("Paused", Duration::ZERO),
            Capture::Finished => ("Finishing", Duration::ZERO),
        };
        let seconds = (self.elapsed + extra).as_secs();
        let mut text = format!("{state} {}:{:02}", seconds / 60, seconds % 60);
        match &self.upload {
            Upload::Running(_) => text.push_str(&format!(" · Transcribing ({} pending)", self.queue.len())),
            Upload::Failed(error) => {
                text.push_str(" · ");
                text.push_str(error);
            }
            Upload::Ready(_) => text.push_str(" · Waiting for text composition to finish"),
            Upload::Idle => {}
        }
        if let Some(notice) = &self.notice {
            text.push_str(" · ");
            text.push_str(notice);
        }
        text
    }

    pub fn level(&self) -> u32 {
        match &self.capture {
            Capture::Recording { recorder, .. } => recorder.level.load(Ordering::Relaxed),
            _ => 0,
        }
    }

    pub fn transcript(&self) -> Option<&str> {
        match &self.upload {
            Upload::Ready(text) => Some(text),
            _ => None,
        }
    }

    pub fn accept_transcript(&mut self) {
        if matches!(self.upload, Upload::Ready(_)) {
            self.queue.pop_front();
            self.upload = Upload::Idle;
        }
    }

    /// A completed segment stays owned here until the document accepts it.
    /// In particular, an active IME composition must finish before insertion.
    pub fn poll(&mut self) {
        let captured = match &self.capture {
            Capture::Recording { recorder, .. } | Capture::Stopping { recorder, .. } => recorder.poll(),
            _ => None,
        };
        if let Some(result) = captured {
            self.capture = if matches!(self.capture, Capture::Stopping { finish: true, .. }) {
                Capture::Finished
            } else {
                Capture::Paused
            };
            match result {
                Ok(audio) => {
                    self.elapsed += audio.duration;
                    self.notice = audio.warning.clone();
                    if audio.duration >= Duration::from_millis(100) {
                        self.queue.push_back(Arc::new(audio));
                    }
                }
                Err(error) => {
                    self.notice = Some(format!("Recording failed: {error}"));
                    self.capture = Capture::Paused;
                }
            }
        }
        let result = match &self.upload {
            Upload::Running(rx) => match rx.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => Some(Err(
                    "Transcription worker stopped. Audio retained; retry or cancel.".into(),
                )),
            },
            _ => None,
        };
        if let Some(result) = result {
            match result {
                Ok(segment) => {
                    self.upload = Upload::Ready(segment);
                }
                Err(error) => {
                    self.upload = Upload::Failed(error);
                    self.toggle_if_recording();
                }
            }
        }
        if matches!(self.upload, Upload::Idle) {
            if let Some(audio) = self.queue.front() {
                let audio = audio.clone();
                let provider = self.provider.clone();
                let (tx, rx) = mpsc::channel();
                match std::thread::Builder::new()
                    .name("voice-transcribe".into())
                    .spawn(move || {
                        let _ = tx.send(provider.transcribe(&audio));
                    }) {
                    Ok(_) => self.upload = Upload::Running(rx),
                    Err(error) => self.upload = Upload::Failed(format!("Could not start transcription: {error}")),
                }
            }
        }
    }
}
