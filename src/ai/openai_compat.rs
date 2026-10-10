use std::{collections::BTreeMap, sync::LazyLock};

use crate::{
    ai::{AiEngine, ChatMessage, Generation, GenerationRequest, is_truncation_reason},
    config::{Config, default_api_url_for_provider},
    errors::normalize_provider_error,
    prompt::sanitize_model_output,
    token::count_messages,
};
use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::{Client, Proxy};
use serde::{Deserialize, Serialize};

const CONNECT_TIMEOUT_SECS: u64 = 10;

/// OpenCode Go rejects requests without a per-conversation session header, and
/// serves stale (sometimes empty) cached responses for a reused id. One `aic`
/// process is one conversation, so a single id is generated per process.
static OPENCODE_SESSION_ID: LazyLock<String> = LazyLock::new(|| uuid::Uuid::new_v4().to_string());

fn has_header(headers: &BTreeMap<String, String>, name: &str) -> bool {
    headers.keys().any(|key| key.eq_ignore_ascii_case(name))
}

#[derive(Debug, Clone)]
pub struct OpenAiCompatEngine {
    config: Config,
    client: Client,
    base_url: String,
    session_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: &'a [ChatMessage],
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_completion_tokens: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ResponseMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ResponseMessage {
    content: Option<String>,
}

impl OpenAiCompatEngine {
    pub fn new(config: Config) -> Result<Self> {
        let mut builder =
            Client::builder().connect_timeout(std::time::Duration::from_secs(CONNECT_TIMEOUT_SECS));
        if config.http_timeout > 0 {
            builder = builder.timeout(std::time::Duration::from_secs(config.http_timeout as u64));
        }
        if let Some(proxy) = &config.proxy {
            builder = builder.proxy(Proxy::all(proxy)?);
        }

        let client = builder.build()?;
        let base_url = match config.ai_provider.as_str() {
            "azure-openai" => config.api_url.clone().context(
                "AIC_API_URL is required for Azure OpenAI; use https://<resource>.openai.azure.com/openai/v1",
            )?,
            "groq" | "ollama" => config
                .api_url
                .clone()
                .or_else(|| default_api_url_for_provider(&config.ai_provider).map(str::to_owned))
                .unwrap_or_else(|| {
                    if config.ai_provider == "ollama" {
                        "http://localhost:11434/v1".to_owned()
                    } else {
                        "https://api.groq.com/openai/v1".to_owned()
                    }
                }),
            "opencode-go" => config
                .api_url
                .clone()
                .or_else(|| default_api_url_for_provider("opencode-go").map(str::to_owned))
                .unwrap_or_else(|| "https://opencode.ai/zen/go/v1".to_owned()),
            _ => config
                .api_url
                .clone()
                .unwrap_or_else(|| "https://api.openai.com/v1".to_owned()),
        };

        // Scoped to the Go path: the sibling Zen gateway (/zen/v1) does not
        // document the session header, and OpenCode Zen is a separate
        // pay-per-token product from the Go subscription.
        let is_opencode_go =
            config.ai_provider == "opencode-go" || base_url.contains("opencode.ai/zen/go");
        let session_id = (is_opencode_go
            && !has_header(&config.api_custom_headers, "x-opencode-session"))
        .then(|| OPENCODE_SESSION_ID.clone());

        Ok(Self {
            config,
            client,
            base_url,
            session_id,
        })
    }

    fn chat_url(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        if base.ends_with("/chat/completions") {
            base.to_owned()
        } else {
            format!("{base}/chat/completions")
        }
    }
}

#[async_trait]
impl AiEngine for OpenAiCompatEngine {
    async fn generate_commit_message(&self, messages: &[ChatMessage]) -> Result<String> {
        Ok(self
            .generate_with_options(messages, &GenerationRequest::default())
            .await?
            .text)
    }

    async fn generate_with_options(
        &self,
        messages: &[ChatMessage],
        request: &GenerationRequest,
    ) -> Result<Generation> {
        let max_output_tokens = request
            .max_output_tokens
            .unwrap_or(self.config.tokens_max_output)
            .max(1);
        let request_tokens = count_messages(messages);
        if request_tokens
            > self
                .config
                .tokens_max_input
                .saturating_sub(max_output_tokens)
        {
            return Err(crate::errors::AicError::TooManyTokens.into());
        }

        let is_reasoning_model = self.config.model.starts_with("o1")
            || self.config.model.starts_with("o3")
            || self.config.model.starts_with("o4")
            || self.config.model.starts_with("gpt-5");

        let payload = ChatRequest {
            model: &self.config.model,
            messages,
            temperature: (!is_reasoning_model).then_some(0.0),
            top_p: (!is_reasoning_model).then_some(0.1),
            max_tokens: (!is_reasoning_model).then_some(max_output_tokens),
            max_completion_tokens: is_reasoning_model.then_some(max_output_tokens),
        };

        let mut request = self.client.post(self.chat_url()).json(&payload);

        if let Some(api_key) = &self.config.api_key {
            request = if self.config.ai_provider == "azure-openai" {
                request.header("api-key", api_key)
            } else {
                request.bearer_auth(api_key)
            };
        }

        if let Some(session_id) = &self.session_id {
            request = request.header("x-opencode-session", session_id);
        }

        for (key, value) in &self.config.api_custom_headers {
            request = request.header(key, value);
        }

        let response = request.send().await.map_err(|error| {
            if error.is_timeout() {
                anyhow::anyhow!(
                    "request to {} timed out after {}s - raise AIC_HTTP_TIMEOUT or reduce the staged diff",
                    self.config.ai_provider,
                    self.config.http_timeout
                )
            } else {
                anyhow::Error::new(error).context("failed to call AI provider")
            }
        })?;
        let status = response.status();
        let body = response.text().await?;

        if !status.is_success() {
            return Err(normalize_provider_error(
                &self.config.ai_provider,
                &self.config.model,
                Some(status.as_u16()),
                &body,
            )
            .into());
        }

        let response: ChatResponse = serde_json::from_str(&body)
            .with_context(|| format!("failed to parse AI response: {body}"))?;
        let choice = response
            .choices
            .first()
            .ok_or(crate::errors::AicError::EmptyMessage)?;
        let content = choice
            .message
            .content
            .as_deref()
            .map(sanitize_model_output)
            .filter(|content| !content.is_empty())
            .ok_or(crate::errors::AicError::EmptyMessage)?;

        Ok(Generation {
            text: content,
            truncated: choice
                .finish_reason
                .as_deref()
                .is_some_and(is_truncation_reason),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_chat_completions_path() {
        let config = Config {
            api_url: Some("http://localhost:9000/v1".to_owned()),
            ..Config::default()
        };
        let engine = OpenAiCompatEngine::new(config).unwrap();
        assert_eq!(
            engine.chat_url(),
            "http://localhost:9000/v1/chat/completions"
        );
    }

    #[test]
    fn opencode_go_url_gets_a_session_id_under_the_openai_provider_id() {
        let config = Config {
            ai_provider: "openai".to_owned(),
            api_url: Some("https://opencode.ai/zen/go/v1".to_owned()),
            ..Config::default()
        };
        let engine = OpenAiCompatEngine::new(config).unwrap();
        assert!(engine.session_id.is_some());
    }

    #[test]
    fn opencode_zen_url_gets_no_session_id() {
        let config = Config {
            ai_provider: "openai".to_owned(),
            api_url: Some("https://opencode.ai/zen/v1".to_owned()),
            ..Config::default()
        };
        let engine = OpenAiCompatEngine::new(config).unwrap();
        assert!(engine.session_id.is_none());
    }

    #[test]
    fn unrelated_urls_get_no_session_id() {
        let config = Config {
            ai_provider: "openai".to_owned(),
            api_url: Some("https://api.openai.com/v1".to_owned()),
            ..Config::default()
        };
        let engine = OpenAiCompatEngine::new(config).unwrap();
        assert!(engine.session_id.is_none());
    }
}
