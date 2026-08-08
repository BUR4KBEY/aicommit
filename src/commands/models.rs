use std::{
    collections::BTreeMap,
    fs,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Result, bail};
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::{
    config::{
        Config, default_api_url_for_provider, global_model_cache_path, is_local_cli_provider,
        model_list, supported_providers,
    },
    ui,
};

const CACHE_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);

#[derive(Debug, Default, Serialize, Deserialize)]
struct ModelCache {
    timestamp: u64,
    models: BTreeMap<String, Vec<String>>,
}

pub async fn run(provider_override: Option<String>, refresh: bool) -> Result<()> {
    let config = Config::load_with_provider_override(provider_override.as_deref())?;
    let provider = config.ai_provider.clone();
    if !supported_providers().contains(&provider.as_str()) {
        bail!(
            "unsupported provider '{provider}'; supported values: {}",
            supported_providers().join(", ")
        );
    }

    if is_local_cli_provider(&provider) {
        ui::section(format!("Available models for {provider}"));
        ui::info(format!("❯ {}", config.model));
        let binary = match provider.as_str() {
            "claude-code" => "`claude`",
            "codex" => "`codex exec`",
            "copilot" => "`copilot`",
            _ => "`local CLI`",
        };
        ui::secondary(format!(
            "Uses the installed {binary} CLI from PATH with its existing authentication."
        ));
        return Ok(());
    }

    let cache_path = global_model_cache_path()?;
    let mut cache = read_cache(&cache_path).unwrap_or_default();

    let models = if !refresh && cache_is_fresh(&cache) {
        cache.models.get(&provider).cloned()
    } else {
        None
    };

    let models = match models {
        Some(models) => models,
        None => {
            let fetched = fetch_models(&provider, &config).await.unwrap_or_else(|_| {
                model_list(&provider)
                    .iter()
                    .map(|model| model.to_string())
                    .collect()
            });
            cache.timestamp = current_timestamp();
            cache.models.insert(provider.clone(), fetched.clone());
            write_cache(&cache_path, &cache)?;
            fetched
        }
    };

    ui::section(format!("Available models for {provider}"));
    if models.is_empty() {
        ui::secondary("no models found");
    } else {
        for model in models {
            if model == config.model {
                ui::info(format!("❯ {model}"));
            } else {
                ui::secondary(model);
            }
        }
    }
    Ok(())
}

async fn fetch_models(provider: &str, config: &Config) -> Result<Vec<String>> {
    match provider {
        "openai" | "groq" | "ollama" => {
            let fallback =
                match provider {
                    "groq" => default_api_url_for_provider("groq")
                        .unwrap_or("https://api.groq.com/openai/v1"),
                    "ollama" => default_api_url_for_provider("ollama")
                        .unwrap_or("http://localhost:11434/v1"),
                    _ => "https://api.openai.com/v1",
                };
            let base = config.api_url.as_deref().unwrap_or(fallback);
            fetch_openai_models(
                provider,
                &format!("{}/models", base.trim_end_matches('/')),
                config,
            )
            .await
        }
        "azure-openai" => {
            if let Some(base) = config.api_url.as_deref() {
                fetch_openai_models(
                    provider,
                    &format!("{}/models", base.trim_end_matches('/')),
                    config,
                )
                .await
            } else {
                Ok(model_list(provider)
                    .iter()
                    .map(|model| model.to_string())
                    .collect())
            }
        }
        "anthropic" => {
            let base = config
                .api_url
                .as_deref()
                .or_else(|| default_api_url_for_provider("anthropic"))
                .unwrap_or("https://api.anthropic.com/v1");
            fetch_anthropic_models(&format!("{}/models", base.trim_end_matches('/')), config).await
        }
        _ => Ok(model_list(provider)
            .iter()
            .map(|model| model.to_string())
            .collect()),
    }
}

async fn fetch_openai_models(provider: &str, url: &str, config: &Config) -> Result<Vec<String>> {
    #[derive(Debug, Deserialize)]
    struct Response {
        data: Vec<Model>,
    }

    #[derive(Debug, Deserialize)]
    struct Model {
        id: String,
    }

    let mut request = Client::new().get(url);
    if let Some(api_key) = &config.api_key {
        request = if provider == "azure-openai" {
            request.header("api-key", api_key)
        } else {
            request.bearer_auth(api_key)
        };
    }
    let response = request
        .send()
        .await?
        .error_for_status()?
        .json::<Response>()
        .await?;
    let mut models = response
        .data
        .into_iter()
        .map(|model| model.id)
        .filter(|model| match provider {
            "groq" | "ollama" => true,
            _ => {
                model.starts_with("gpt-")
                    || model.starts_with("o1")
                    || model.starts_with("o3")
                    || model.starts_with("o4")
            }
        })
        .collect::<Vec<_>>();
    models.sort();
    Ok(models)
}

async fn fetch_anthropic_models(url: &str, config: &Config) -> Result<Vec<String>> {
    #[derive(Debug, Deserialize)]
    struct Response {
        data: Vec<Model>,
    }

    #[derive(Debug, Deserialize)]
    struct Model {
        id: String,
    }

    let mut request = Client::new()
        .get(url)
        .header("anthropic-version", "2023-06-01");
    if let Some(api_key) = &config.api_key {
        request = request.header("x-api-key", api_key);
    }
    for (key, value) in &config.api_custom_headers {
        request = request.header(key, value);
    }

    let response = request
        .send()
        .await?
        .error_for_status()?
        .json::<Response>()
        .await?;
    let mut models = response
        .data
        .into_iter()
        .map(|model| model.id)
        .collect::<Vec<_>>();
    models.sort();
    Ok(models)
}

fn read_cache(path: &std::path::Path) -> Result<ModelCache> {
    let content = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&content)?)
}

fn write_cache(path: &std::path::Path, cache: &ModelCache) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(cache)?)?;
    Ok(())
}

fn cache_is_fresh(cache: &ModelCache) -> bool {
    current_timestamp().saturating_sub(cache.timestamp) < CACHE_TTL.as_secs()
}

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{header, method, path},
    };

    #[tokio::test]
    async fn fetches_groq_models_via_openai_compatible_endpoint() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/openai/v1/models"))
            .and(header("authorization", "Bearer key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [
                    { "id": "llama-3.3-70b-versatile" },
                    { "id": "llama-3.1-8b-instant" }
                ]
            })))
            .mount(&server)
            .await;

        let config = Config {
            ai_provider: "groq".to_owned(),
            api_key: Some("key".to_owned()),
            api_url: Some(format!("{}/openai/v1", server.uri())),
            model: "llama-3.1-8b-instant".to_owned(),
            ..Config::default()
        };

        let models = fetch_models("groq", &config).await.unwrap();

        assert_eq!(
            models,
            vec![
                "llama-3.1-8b-instant".to_owned(),
                "llama-3.3-70b-versatile".to_owned(),
            ]
        );
    }

    #[tokio::test]
    async fn fetches_ollama_models_via_openai_compatible_endpoint_without_api_key() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [
                    { "id": "llama3.2" },
                    { "id": "qwen3-coder" }
                ]
            })))
            .mount(&server)
            .await;

        let config = Config {
            ai_provider: "ollama".to_owned(),
            api_url: Some(format!("{}/v1", server.uri())),
            model: "llama3.2".to_owned(),
            ..Config::default()
        };

        let models = fetch_models("ollama", &config).await.unwrap();

        assert_eq!(
            models,
            vec!["llama3.2".to_owned(), "qwen3-coder".to_owned()]
        );
    }

    #[tokio::test]
    async fn fetches_anthropic_models_via_models_endpoint() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .and(header("x-api-key", "key"))
            .and(header("anthropic-version", "2023-06-01"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [
                    { "id": "claude-opus-4-20250514" },
                    { "id": "claude-sonnet-4-20250514" }
                ]
            })))
            .mount(&server)
            .await;

        let config = Config {
            ai_provider: "anthropic".to_owned(),
            api_key: Some("key".to_owned()),
            api_url: Some(format!("{}/v1", server.uri())),
            model: "claude-sonnet-4-20250514".to_owned(),
            ..Config::default()
        };

        let models = fetch_models("anthropic", &config).await.unwrap();

        assert_eq!(
            models,
            vec![
                "claude-opus-4-20250514".to_owned(),
                "claude-sonnet-4-20250514".to_owned(),
            ]
        );
    }
}
