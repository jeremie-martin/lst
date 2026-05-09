use std::{ops::Range, time::Duration};

use gpui::Context;
use lst_editor::{TabId, UndoBoundary};

use crate::LstGpuiApp;

impl LstGpuiApp {
    pub(crate) fn start_cleanup(&mut self, cx: &mut Context<Self>) {
        if self.cleanup_in_flight {
            return;
        }

        let client = match build_llm_client() {
            Ok(client) => client,
            Err(message) => {
                self.cleanup_message = Some(message);
                cx.notify();
                return;
            }
        };

        let tab = self.active_tab();
        let tab_id = tab.id();
        let revision = tab.revision();
        let (range, source_text) = if tab.has_selection() {
            let range = tab.selected_range();
            match tab.selected_text() {
                Some(text) if !text.is_empty() => (range, text),
                _ => {
                    self.cleanup_message = Some("Nothing to clean up.".to_string());
                    cx.notify();
                    return;
                }
            }
        } else {
            let text = tab.buffer_text();
            if text.is_empty() {
                self.cleanup_message = Some("Nothing to clean up.".to_string());
                cx.notify();
                return;
            }
            (0..tab.buffer().len_chars(), text)
        };

        self.cleanup_in_flight = true;
        self.cleanup_message = Some("\u{27F3} Cleaning\u{2026}".to_string());
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { client.cleanup(&source_text) })
                .await;
            let _ = this.update(cx, |app, cx| {
                app.cleanup_in_flight = false;
                match result {
                    Ok(cleaned) => app.apply_cleanup_result(tab_id, revision, range, cleaned, cx),
                    Err(err) => app.finish_cleanup_with_error(err, cx),
                }
            });
        })
        .detach();
    }

    fn apply_cleanup_result(
        &mut self,
        tab_id: TabId,
        revision: u64,
        range: Range<usize>,
        cleaned: String,
        cx: &mut Context<Self>,
    ) {
        let stale = match self.model.tab_by_id(tab_id) {
            Some(tab) => self.model.active_tab_id() != tab_id || tab.revision() != revision,
            None => true,
        };
        if stale {
            self.cleanup_message =
                Some("Buffer changed during cleanup; result discarded.".to_string());
            cx.notify();
            return;
        }

        self.update_model(cx, true, |model| {
            model.replace_text(Some(range), cleaned, UndoBoundary::Break);
        });
    }

    fn finish_cleanup_with_error(&mut self, err: crate::llm::LlmError, cx: &mut Context<Self>) {
        self.cleanup_message = Some(format!("Cleanup failed: {err}"));
        cx.notify();
    }
}

fn build_llm_client() -> Result<Box<dyn crate::llm::LlmClient>, String> {
    if let Some(canned) = std::env::var("LST_LLM_FAKE_RESPONSE")
        .ok()
        .filter(|s| !s.is_empty())
    {
        let delay_ms = std::env::var("LST_LLM_FAKE_DELAY_MS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        return Ok(Box::new(crate::llm::FakeLlmClient::new(
            canned,
            Duration::from_millis(delay_ms),
        )));
    }

    let api_key = match std::env::var("DEEPSEEK_API_KEY") {
        Ok(value) if !value.is_empty() => value,
        _ => return Err("DEEPSEEK_API_KEY not set".to_string()),
    };
    let model_name = std::env::var("DEEPSEEK_MODEL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| crate::llm::DEFAULT_DEEPSEEK_MODEL.to_string());
    Ok(Box::new(crate::llm::DeepSeekClient::new(
        api_key, model_name,
    )))
}
