use std::fmt;
use std::time::Duration;

use serde::Deserialize;
use serde_json::json;

use super::prompt::SYSTEM_PROMPT;

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

        let payload: ChatResponse = response
            .into_json()
            .map_err(|err| LlmError::Parse(err.to_string()))?;

        let content = payload
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .ok_or(LlmError::EmptyResponse)?;

        let cleaned = strip_trailing_newline(content);
        if cleaned.is_empty() {
            return Err(LlmError::EmptyResponse);
        }

        Ok(cleaned)
    }
}

fn strip_trailing_newline(mut s: String) -> String {
    if s.ends_with('\n') {
        s.pop();
        if s.ends_with('\r') {
            s.pop();
        }
    }
    s
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_chat_response_extracts_content() {
        let raw = r#"{"choices":[{"message":{"role":"assistant","content":"hello world"}}]}"#;
        let parsed: ChatResponse = serde_json::from_str(raw).unwrap();
        assert_eq!(parsed.choices[0].message.content, "hello world");
    }

    #[test]
    fn strip_trailing_newline_removes_lf_and_crlf() {
        assert_eq!(strip_trailing_newline("hi\n".into()), "hi");
        assert_eq!(strip_trailing_newline("hi\r\n".into()), "hi");
        assert_eq!(strip_trailing_newline("hi".into()), "hi");
        assert_eq!(strip_trailing_newline("hi\n\n".into()), "hi\n");
    }
}
