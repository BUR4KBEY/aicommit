use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::{Client, Proxy};
use serde::{Deserialize, Serialize};

use crate::{
    ai::{AiEngine, ChatMessage},
    config::{Config, default_api_url_for_provider},
    errors::normalize_provider_error,
    prompt::sanitize_model_output,
    token::count_messages,
};

const CONNECT_TIMEOUT_SECS: u64 = 10;

#[derive(Debug, Clone)]
pub struct OpenAiCompatEngine {
    config: Config,
    client: Client,
    base_url: String,
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
            _ => config
                .api_url
                .clone()
                .unwrap_or_else(|| "https://api.openai.com/v1".to_owned()),
        };

        Ok(Self {
            config,
            client,
            base_url,
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
        let request_tokens = count_messages(messages);
        if request_tokens
            > self
                .config
                .tokens_max_input
                .saturating_sub(self.config.tokens_max_output)
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
            max_tokens: (!is_reasoning_model).then_some(self.config.tokens_max_output),
            max_completion_tokens: is_reasoning_model.then_some(self.config.tokens_max_output),
        };

        let mut request = self.client.post(self.chat_url()).json(&payload);

        if let Some(api_key) = &self.config.api_key {
            request = if self.config.ai_provider == "azure-openai" {
                request.header("api-key", api_key)
            } else {
                request.bearer_auth(api_key)
            };
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
        let content = response
            .choices
            .first()
            .and_then(|choice| choice.message.content.as_deref())
            .map(sanitize_model_output)
            .filter(|content| !content.is_empty())
            .ok_or(crate::errors::AicError::EmptyMessage)?;

        Ok(content)
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
}
