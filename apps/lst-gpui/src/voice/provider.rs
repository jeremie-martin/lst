use super::audio::Audio;
use reqwest::{multipart, Client};
use std::{
    sync::{mpsc, Arc},
    time::Duration,
};
use tokio::sync::oneshot;

#[derive(Clone)]
pub(super) struct Provider {
    key: String,
    language: String,
    endpoint: String,
}

impl Provider {
    pub fn new(language: &str) -> Result<Self, String> {
        let key = std::env::var("ELEVENLABS_API_KEY")
            .ok()
            .map(|key| key.trim().to_owned())
            .filter(|key| !key.is_empty())
            .ok_or("Set ELEVENLABS_API_KEY before starting dictation.")?;
        let language = language.trim();
        if language != "auto" && !(matches!(language.len(), 2 | 3) && language.bytes().all(|b| b.is_ascii_lowercase()))
        {
            return Err("Voice language must be auto or a two/three-letter lowercase language code.".into());
        }
        let mut endpoint = "https://api.elevenlabs.io/v1/speech-to-text".to_string();
        if std::env::var_os("LST_X11_STATE_TRACE_FILE").is_some() {
            if let Ok(port) = std::env::var("LST_TEST_VOICE_PORT") {
                let port: u16 = port.parse().map_err(|_| "Invalid test server port")?;
                endpoint = format!("http://127.0.0.1:{port}/v1/speech-to-text");
            }
        }
        Ok(Self {
            key,
            language: language.into(),
            endpoint,
        })
    }

    pub fn start(&self, audio: Arc<Audio>) -> std::io::Result<Transcription> {
        let provider = self.clone();
        let (tx, result) = mpsc::channel();
        let (cancel, cancelled) = oneshot::channel::<()>();
        std::thread::Builder::new().name("voice-transcribe".into()).spawn(move || {
            let result = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                Ok(runtime) => runtime.block_on(async {
                    tokio::select! {
                        result = provider.request(&audio) => result.map_err(|error| format!("Transcription failed: {error}. Audio retained; retry or cancel.")),
                        _ = cancelled => Err("Transcription cancelled".into()),
                    }
                }),
                Err(error) => Err(format!("Could not start transcription: {error}")),
            };
            let _ = tx.send(result);
        })?;
        Ok(Transcription {
            result,
            _cancel: cancel,
        })
    }

    async fn request(&self, audio: &Audio) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .timeout(upload_timeout(audio.file.as_file().metadata()?.len()))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let file = multipart::Part::file(audio.file.path())
            .await?
            .file_name("audio.wav")
            .mime_str("audio/wav")?;
        let mut form = multipart::Form::new()
            .part("file", file)
            .text("model_id", "scribe_v2")
            .text("tag_audio_events", "false")
            .text("diarize", "false")
            .text("no_verbatim", "true");
        if self.language != "auto" {
            form = form.text("language_code", self.language.clone());
        }
        let mut response = client
            .post(&self.endpoint)
            .header("xi-api-key", &self.key)
            .multipart(form)
            .send()
            .await?;
        let status = response.status();
        let mut body = Vec::new();
        while let Some(chunk) = tokio::time::timeout(Duration::from_secs(120), response.chunk()).await?? {
            if body.len().saturating_add(chunk.len()) > 4 * 1024 * 1024 {
                return Err("Transcription response is too large".into());
            }
            body.extend_from_slice(&chunk);
        }
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
        if !status.is_success() {
            let detail = value
                .pointer("/detail/message")
                .or_else(|| value.get("detail"))
                .and_then(|v| v.as_str())
                .unwrap_or("Service request failed");
            return Err(format!(
                "HTTP {}: {}",
                status.as_u16(),
                detail.chars().take(300).collect::<String>()
            )
            .into());
        }
        let text = value
            .get("text")
            .and_then(|v| v.as_str())
            .ok_or("Service returned no transcript")?;
        Ok(text.replace("\r\n", "\n").replace('\r', "\n").trim().to_string())
    }
}

// Budget transfer time at 16 KiB/s, plus two minutes for service processing.
// Cancellation drops the request future, including stalled uploads, immediately.
fn upload_timeout(bytes: u64) -> Duration {
    Duration::from_secs(120u64.saturating_add(bytes.div_ceil(16 * 1024)))
}

pub(super) struct Transcription {
    result: mpsc::Receiver<Result<String, String>>,
    _cancel: oneshot::Sender<()>,
}

impl Transcription {
    pub fn poll(&self) -> Option<Result<String, String>> {
        match self.result.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(
                "Transcription worker stopped. Audio retained; retry or cancel.".into(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Read, net::TcpListener, time::Instant};

    #[test]
    fn transfer_budget_covers_a_full_segment_on_a_slow_link() {
        assert_eq!(upload_timeout(0), Duration::from_secs(120));
        assert_eq!(upload_timeout(1), Duration::from_secs(121));
        // Fifteen minutes of 48 kHz mono PCM, plus its WAV header.
        assert_eq!(upload_timeout(900 * 48_000 * 2 + 44), Duration::from_secs(5394));
    }

    #[test]
    fn cancelling_a_stalled_request_closes_the_connection_and_releases_audio() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let (received, ready) = mpsc::channel();
        let server = std::thread::spawn(move || {
            let (mut connection, _) = listener.accept().unwrap();
            connection.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            let mut bytes = [0; 8192];
            assert!(connection.read(&mut bytes).unwrap() > 0);
            received.send(()).unwrap();
            // Never respond. Cancellation must close this connection promptly,
            // including when the server has not finished reading the upload.
            loop {
                match connection.read(&mut bytes) {
                    Ok(0) => break,
                    Ok(_) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => break,
                    Err(error) => panic!("cancelled connection remained open: {error}"),
                }
            }
        });
        let file = tempfile::NamedTempFile::new().unwrap();
        file.as_file().set_len(4 * 1024 * 1024).unwrap();
        let path = file.path().to_path_buf();
        let audio = Arc::new(Audio {
            file,
            duration: Duration::from_secs(10),
            warning: None,
        });
        let released = Arc::downgrade(&audio);
        let request = Provider {
            key: "fixture".into(),
            language: "auto".into(),
            endpoint,
        }
        .start(audio)
        .unwrap();
        ready.recv_timeout(Duration::from_secs(3)).unwrap();
        drop(request);
        server.join().unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while released.upgrade().is_some() || path.exists() {
            assert!(Instant::now() < deadline, "cancelled worker retained the audio");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
