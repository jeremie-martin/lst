use super::audio::Audio;
use reqwest::blocking::{multipart, Client};
use std::{io::Read, time::Duration};

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
            .filter(|s| !s.trim().is_empty())
            .ok_or("Set ELEVENLABS_API_KEY before starting a voice note.")?;
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

    pub fn transcribe(&self, audio: &Audio) -> Result<String, String> {
        self.request(audio)
            .map_err(|error| format!("Transcription failed: {error}. Audio retained; retry or cancel."))
    }

    fn request(&self, audio: &Audio) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(120))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let file = multipart::Part::file(audio.file.path())?
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
        let response = client
            .post(&self.endpoint)
            .header("xi-api-key", &self.key)
            .multipart(form)
            .send()?;
        let status = response.status();
        let mut body = Vec::new();
        response.take(4 * 1024 * 1024 + 1).read_to_end(&mut body)?;
        if body.len() > 4 * 1024 * 1024 {
            return Err("Transcription response is too large".into());
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
