use std::fmt;
use std::time::Duration;

use serde::Deserialize;
use serde_json::json;

pub(crate) const SYSTEM_PROMPT: &str = concat!(
    "You are a careful copy editor. The input is text that may contain transcription artifacts: filler words ",
    "(\"um\", \"uh\", \"like\", \"you know\"), repetitions, false starts, and occasional misrecognized words. ",
    "Your job is to produce a cleaned version that:\n\n",
    "- preserves the original meaning, voice, and content;\n",
    "- preserves paragraph and line-break structure;\n",
    "- does not add headings, bullet points, bold, or any markdown not present in the input;\n",
    "- does not summarize, expand, or restyle;\n",
    "- keeps the same language as the input.\n\n",
    "Output only the cleaned text. No preamble, no commentary, no code fences."
);

const DEEPSEEK_ENDPOINT: &str = "https://api.deepseek.com/v1/chat/completions";
pub(crate) const DEFAULT_DEEPSEEK_MODEL: &str = "deepseek-v4-flash";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

pub(crate) trait LlmClient: Send + Sync {
    fn cleanup(&self, text: &str) -> Result<String, LlmError>;
}

#[derive(Debug)]
pub(crate) enum LlmError {
    Transport(String),
    Status { code: u16, body: String },
    Parse(String),
    EmptyResponse,
}

impl fmt::Display for LlmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LlmError::Transport(msg) => write!(f, "network error: {msg}"),
            LlmError::Status { code, body } => {
                let snippet: String = body.chars().take(200).collect();
                write!(f, "HTTP {code}: {snippet}")
            }
            LlmError::Parse(msg) => write!(f, "could not parse response: {msg}"),
            LlmError::EmptyResponse => write!(f, "empty response from model"),
        }
    }
}

pub(crate) struct DeepSeekClient {
    api_key: String,
    model: String,
    endpoint: String,
}

impl DeepSeekClient {
    pub(crate) fn new(api_key: String, model: String) -> Self {
        Self {
            api_key,
            model,
            endpoint: DEEPSEEK_ENDPOINT.to_string(),
        }
    }
}

impl LlmClient for DeepSeekClient {
    fn cleanup(&self, text: &str) -> Result<String, LlmError> {
        let body = json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": SYSTEM_PROMPT},
                {"role": "user", "content": text},
            ],
            "stream": false,
            "temperature": 0.2,
        });

        let agent = ureq::AgentBuilder::new().timeout(REQUEST_TIMEOUT).build();
        let response = agent
            .post(&self.endpoint)
            .set("Authorization", &format!("Bearer {}", self.api_key))
            .set("Content-Type", "application/json")
            .send_json(body);

        let response = match response {
            Ok(resp) => resp,
            Err(ureq::Error::Status(code, resp)) => {
                let body = resp.into_string().unwrap_or_default();
                return Err(LlmError::Status { code, body });
            }
            Err(err) => return Err(LlmError::Transport(err.to_string())),
        };

        let payload: ChatResponse = response.into_json().map_err(|err| LlmError::Parse(err.to_string()))?;

        let content = payload
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .ok_or(LlmError::EmptyResponse)?;

        if content.is_empty() {
            return Err(LlmError::EmptyResponse);
        }

        Ok(content)
    }
}

/// In-process stand-in for [`DeepSeekClient`] used by the X11 e2e harness.
///
/// Activated when the `lst` process is launched with `LST_LLM_FAKE_RESPONSE`
/// set; the dispatch lives in `start_cleanup`. Unconditionally compiled so
/// the test harness can drive the real binary, the same approach
/// `StateTraceEmitter` uses for its env-var-gated trace channel. With no
/// env var set this code is dormant.
pub(crate) struct FakeLlmClient {
    canned: String,
    delay: Duration,
}

impl FakeLlmClient {
    pub(crate) fn new(canned: String, delay: Duration) -> Self {
        Self { canned, delay }
    }
}

impl LlmClient for FakeLlmClient {
    fn cleanup(&self, _text: &str) -> Result<String, LlmError> {
        if !self.delay.is_zero() {
            std::thread::sleep(self.delay);
        }
        Ok(self.canned.clone())
    }
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Deserialize)]
struct ChatMessage {
    content: String,
}
