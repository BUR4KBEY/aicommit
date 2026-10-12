use thiserror::Error;

#[derive(Debug, Error)]
pub enum AicError {
    #[error("unsupported config key: {0}")]
    UnsupportedConfigKey(String),

    #[error("invalid value for {key}: {message}")]
    InvalidConfigValue { key: String, message: String },

    #[error("no changes detected")]
    NoChanges,

    #[error("interactive mode requires a TTY")]
    NotTty,

    #[error("commit aborted")]
    Aborted,

    #[error("commit was created locally, but the push failed: {0}")]
    CommitCreatedPushFailed(String),

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

    #[error("provider rejected the request as invalid: {0}")]
    InvalidRequest(String),

    #[error("AI provider returned an empty response")]
    EmptyMessage,

    #[error(
        "diff is too large for the configured token limits - raise AIC_TOKENS_MAX_INPUT or exclude bulky files via .aicommitignore"
    )]
    TooManyTokens,
}

impl AicError {
    /// True when the provider refused this exact request, so sending it again
    /// unchanged fails the same way. Callers must not retry these.
    pub fn rejects_request(&self) -> bool {
        matches!(
            self,
            AicError::MissingApiKey(_)
                | AicError::ModelNotFound { .. }
                | AicError::Authentication(_)
                | AicError::InsufficientCredits(_)
                | AicError::InvalidRequest(_)
                | AicError::TooManyTokens
                | AicError::UnsupportedConfigKey(_)
                | AicError::InvalidConfigValue { .. }
        )
    }
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
    } else if status.is_some_and(is_client_request_error) {
        AicError::InvalidRequest(format!("{provider}: {body}"))
    } else {
        AicError::ServiceUnavailable(format!("{provider}: {body}"))
    }
}

/// 4xx means the request itself is wrong. 408 (timeout) and 409 (conflict)
/// can clear on their own, so they stay retryable.
fn is_client_request_error(status: u16) -> bool {
    (400..=499).contains(&status) && status != 408 && status != 409
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

    #[test]
    fn bad_request_is_an_invalid_request_not_service_unavailable() {
        let error = normalize_provider_error(
            "anthropic",
            "claude-haiku-5-5",
            Some(400),
            r#"{"type":"error","error":{"type":"invalid_request_error","message":"messages.2: user messages must have non-empty content"}}"#,
        );
        assert!(matches!(error, AicError::InvalidRequest(_)));
        assert!(error.rejects_request());
    }

    #[test]
    fn transient_statuses_stay_retryable() {
        for status in [408, 409, 500, 503] {
            let error = normalize_provider_error("anthropic", "claude", Some(status), "oops");
            assert!(
                matches!(error, AicError::ServiceUnavailable(_)),
                "status {status}"
            );
            assert!(!error.rejects_request(), "status {status}");
        }
    }
}
