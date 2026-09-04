use anyhow::{Context, Result};

use crate::errors::AicError;

use super::super::{CONFIG_KEYS, Config};

pub fn apply_file(config: &mut Config, path: &std::path::Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }

    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read config file {}", path.display()))?;
    let doc = content
        .parse::<toml_edit::DocumentMut>()
        .with_context(|| format!("failed to parse config file {}", path.display()))?;

    for key in CONFIG_KEYS {
        if let Some(item) = doc.get(key)
            && !item.is_none()
        {
            apply_toml_item(config, key, item)?;
        }
    }
    Ok(())
}

pub fn apply_process_env(config: &mut Config) -> Result<()> {
    for key in CONFIG_KEYS {
        if let Ok(value) = std::env::var(key) {
            apply_value(config, key, &value)?;
        }
    }
    Ok(())
}

pub fn apply_toml_item(config: &mut Config, key: &str, item: &toml_edit::Item) -> Result<()> {
    let value = item
        .as_value()
        .map(crate::config::parse::toml_value_to_string)
        .unwrap_or_else(|| item.to_string());
    apply_value(config, key, &value)
}

pub fn apply_value(config: &mut Config, key: &str, value: &str) -> Result<()> {
    if !CONFIG_KEYS.contains(&key) {
        return Err(AicError::UnsupportedConfigKey(key.to_owned()).into());
    }

    match key {
        "AIC_AI_PROVIDER" => config.ai_provider = crate::config::parse::normalize_provider(value),
        "AIC_API_KEY" => config.api_key = optional_string(value),
        "AIC_API_URL" => config.api_url = optional_string(value),
        "AIC_API_CUSTOM_HEADERS" => {
            config.api_custom_headers = crate::config::parse::parse_headers(value)?
        }
        "AIC_PROXY" => config.proxy = optional_string(value),
        "AIC_TOKENS_MAX_INPUT" => {
            config.tokens_max_input = crate::config::parse::parse_usize(key, value)?
        }
        "AIC_TOKENS_MAX_OUTPUT" => {
            config.tokens_max_output = crate::config::parse::parse_usize(key, value)?
        }
        "AIC_HTTP_TIMEOUT" => config.http_timeout = crate::config::parse::parse_usize(key, value)?,
        "AIC_DESCRIPTION" => config.description = crate::config::parse::parse_bool(key, value)?,
        "AIC_EMOJI" => config.emoji = crate::config::parse::parse_bool(key, value)?,
        "AIC_MODEL" => config.model = value.to_owned(),
        "AIC_LANGUAGE" => config.language = value.to_owned(),
        "AIC_MESSAGE_TEMPLATE_PLACEHOLDER" => {
            config.message_template_placeholder = value.to_owned()
        }
        "AIC_PROMPT_FILE" => config.prompt_file = optional_string(value),
        "AIC_ONE_LINE_COMMIT" => {
            config.one_line_commit = crate::config::parse::parse_bool(key, value)?
        }
        "AIC_OMIT_SCOPE" => config.omit_scope = crate::config::parse::parse_bool(key, value)?,
        "AIC_GITPUSH" => config.gitpush = crate::config::parse::parse_bool(key, value)?,
        "AIC_REMOTE_ICON_STYLE" => {
            config.remote_icon_style = crate::config::parse::normalize_remote_icon_style(value)?
        }
        "AIC_HOOK_AUTO_UNCOMMENT" => {
            config.hook_auto_uncomment = crate::config::parse::parse_bool(key, value)?
        }
        _ => unreachable!("all config keys are handled"),
    }
    Ok(())
}

pub fn optional_string(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed == "null" || trimmed == "undefined" {
        None
    } else {
        Some(trimmed.to_owned())
    }
}
