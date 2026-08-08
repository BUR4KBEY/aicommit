use thiserror::Error;

#[derive(Debug, Error)]
pub enum AicError {
    #[error("unsupported config key: {0}")]
    UnsupportedConfigKey(String),

    #[error("invalid value for {key}: {message}")]
    InvalidConfigValue { key: String, message: String },

    #[error("no changes detected")]
    NoChanges,

    #[error("not a git repository")]
    NotGitRepository,

    #[error("API key is missing for provider '{0}'")]
    MissingApiKey(String),

    #[error("model '{model}' is not available for provider '{provider}'")]
    ModelNotFound { provider: String, model: String },

    #[error("authentication failed for provider '{0}'")]
    Authentication(String),

    #[error("rate limit exceeded for provider '{0}'")]
    RateLimited(String),

    #[error("insufficient credits or quota for provider '{0}'")]
    InsufficientCredits(String),

    #[error("service unavailable for provider '{0}'")]
    ServiceUnavailable(String),

    #[error("AI provider returned an empty response")]
    EmptyMessage,

    #[error(
        "diff is too large for the configured token limits - raise AIC_TOKENS_MAX_INPUT or exclude bulky files via .aicommitignore"
    )]
    TooManyTokens,
}

pub fn normalize_provider_error(
    provider: &str,
    model: &str,
    status: Option<u16>,
    body: &str,
) -> AicError {
    let lower = body.to_lowercase();

    match status {
        Some(401) => return AicError::Authentication(provider.to_owned()),
        Some(402) => return AicError::InsufficientCredits(provider.to_owned()),
        Some(404) if mentions_model_problem(&lower) => {
            return AicError::ModelNotFound {
                provider: provider.to_owned(),
                model: model.to_owned(),
            };
        }
        Some(413) => return AicError::TooManyTokens,
        Some(429) => return AicError::RateLimited(provider.to_owned()),
        Some(500..=599) => return AicError::ServiceUnavailable(provider.to_owned()),
        _ => {}
    }

    if mentions_context_overflow(&lower) {
        AicError::TooManyTokens
    } else if mentions_model_problem(&lower) {
        AicError::ModelNotFound {
            provider: provider.to_owned(),
            model: model.to_owned(),
        }
    } else if lower.contains("api key")
        || lower.contains("apikey")
        || lower.contains("authentication")
        || lower.contains("unauthorized")
    {
        AicError::Authentication(provider.to_owned())
    } else if lower.contains("rate limit") || lower.contains("too many requests") {
        AicError::RateLimited(provider.to_owned())
    } else if lower.contains("credit")
        || lower.contains("quota")
        || lower.contains("billing")
        || lower.contains("payment")
    {
        AicError::InsufficientCredits(provider.to_owned())
    } else {
        AicError::ServiceUnavailable(format!("{provider}: {body}"))
    }
}

fn mentions_context_overflow(lower: &str) -> bool {
    lower.contains("prompt is too long")
        || lower.contains("prompt too long")
        || lower.contains("context length")
        || lower.contains("context window")
        || lower.contains("maximum context")
        || lower.contains("too many tokens")
        || lower.contains("request too large")
        || lower.contains("input is too long")
}

fn mentions_model_problem(lower: &str) -> bool {
    // Context-size 400s often read "invalid_request_error ... model context
    // window"; rule those out before treating "model" + "invalid" as a
    // missing-model signal.
    !mentions_context_overflow(lower)
        && lower.contains("model")
        && (lower.contains("not found")
            || lower.contains("does not exist")
            || lower.contains("invalid")
            || lower.contains("pull"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_overflow_maps_to_too_many_tokens() {
        let error = normalize_provider_error(
            "openai",
            "gpt-4o",
            Some(400),
            r#"{"error":{"type":"invalid_request_error","message":"This model's maximum context length is 128000 tokens."}}"#,
        );
        assert!(matches!(error, AicError::TooManyTokens));
    }

    #[test]
    fn payload_too_large_status_maps_to_too_many_tokens() {
        let error = normalize_provider_error("anthropic", "claude", Some(413), "Payload Too Large");
        assert!(matches!(error, AicError::TooManyTokens));
    }

    #[test]
    fn context_window_body_is_not_reported_as_missing_model() {
        let error = normalize_provider_error(
            "openai",
            "gpt-4o",
            Some(400),
            "invalid_request_error: the request exceeds the model context window",
        );
        assert!(matches!(error, AicError::TooManyTokens));
    }

    #[test]
    fn missing_model_is_still_detected() {
        let error = normalize_provider_error(
            "ollama",
            "llama3",
            Some(404),
            "model 'llama3' not found, try pulling it first",
        );
        assert!(matches!(error, AicError::ModelNotFound { .. }));
    }
}
