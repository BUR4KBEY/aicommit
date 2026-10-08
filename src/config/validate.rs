use anyhow::{Result, bail};

use crate::errors::AicError;

use super::{Config, model::supported_providers};

pub fn validate_config(config: &Config) -> Result<()> {
    if !supported_providers().contains(&config.ai_provider.as_str())
        && config.ai_provider.as_str() != "test"
    {
        bail!(AicError::InvalidConfigValue {
            key: "AIC_AI_PROVIDER".to_owned(),
            message: format!("supported values: {}", supported_providers().join(", ")),
        });
    }

    if config.ai_provider == "azure-openai" && config.api_url.is_none() {
        bail!(AicError::InvalidConfigValue {
            key: "AIC_API_URL".to_owned(),
            message: "required for Azure OpenAI; use https://<resource>.openai.azure.com/openai/v1"
                .to_owned(),
        });
    }

    if !config.message_template_placeholder.starts_with('$') {
        bail!(AicError::InvalidConfigValue {
            key: "AIC_MESSAGE_TEMPLATE_PLACEHOLDER".to_owned(),
            message: "must start with '$'".to_owned(),
        });
    }

    if config.tokens_max_input <= config.tokens_max_output {
        bail!(AicError::InvalidConfigValue {
            key: "AIC_TOKENS_MAX_INPUT".to_owned(),
            message: "must be greater than AIC_TOKENS_MAX_OUTPUT".to_owned(),
        });
    }

    if config.split_max < 2 {
        bail!(AicError::InvalidConfigValue {
            key: "AIC_SPLIT_MAX".to_owned(),
            message: "must be at least 2".to_owned(),
        });
    }

    Ok(())
}
