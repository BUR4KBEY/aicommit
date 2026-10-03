const SUPPORTED_PROVIDERS: &[&str] = &[
    "openai",
    "azure-openai",
    "anthropic",
    "groq",
    "ollama",
    "claude-code",
    "codex",
    "copilot",
    "apple",
];
const LOCAL_CLI_PROVIDERS: &[&str] = &["claude-code", "codex", "copilot", "apple"];

// Apple's on-device Foundation Model has an 8,192-token context window that
// covers instructions, prompt, and response. aic counts with cl100k, which
// runs ~10-20% under Apple's tokenizer on diffs, so leave generous headroom.
const APPLE_MAX_TOKENS_INPUT: usize = 6_000;

pub fn default_model_for_provider(provider: &str) -> &'static str {
    match provider {
        "claude-code" | "codex" | "copilot" | "apple" => "default",
        "anthropic" => "claude-sonnet-4-20250514",
        "groq" => "llama-3.1-8b-instant",
        "ollama" => "llama3.2",
        "azure-openai" => "gpt-5.4-mini",
        _ => "gpt-5.4-mini",
    }
}

pub fn default_api_url_for_provider(provider: &str) -> Option<&'static str> {
    match provider {
        "anthropic" => Some("https://api.anthropic.com/v1"),
        "groq" => Some("https://api.groq.com/openai/v1"),
        "ollama" => Some("http://localhost:11434/v1"),
        _ => None,
    }
}

pub fn supported_providers() -> &'static [&'static str] {
    SUPPORTED_PROVIDERS
}

pub fn enabled_providers() -> &'static [&'static str] {
    supported_providers()
}

pub fn model_list(provider: &str) -> &'static [&'static str] {
    match provider {
        "claude-code" | "codex" | "copilot" | "apple" => &["default"],
        "anthropic" => &[
            "claude-sonnet-4-20250514",
            "claude-opus-4-20250514",
            "claude-3-7-sonnet-latest",
            "claude-3-5-haiku-latest",
        ],
        "groq" => &[
            "llama-3.1-8b-instant",
            "llama-3.3-70b-versatile",
            "openai/gpt-oss-120b",
        ],
        "ollama" => &["llama3.2", "qwen3-coder", "gpt-oss:20b"],
        "azure-openai" => &["gpt-5.4-mini", "gpt-5.4", "gpt-5.4-nano"],
        _ => &["gpt-5.4-mini", "gpt-5.4", "gpt-5.4-nano"],
    }
}

pub fn is_local_cli_provider(provider: &str) -> bool {
    LOCAL_CLI_PROVIDERS.contains(&provider)
}

/// Hard context ceiling for providers whose model cannot accept more input,
/// regardless of what AIC_TOKENS_MAX_INPUT is set to.
pub fn provider_max_tokens_input(provider: &str) -> Option<usize> {
    match provider {
        "apple" => Some(APPLE_MAX_TOKENS_INPUT),
        _ => None,
    }
}

pub fn provider_needs_api_key(provider: &str) -> bool {
    !matches!(provider, "test" | "ollama") && !is_local_cli_provider(provider)
}

#[cfg(test)]
mod tests {
    use super::{
        is_local_cli_provider, provider_max_tokens_input, provider_needs_api_key,
        supported_providers,
    };

    #[test]
    fn ollama_does_not_need_api_key() {
        assert!(!provider_needs_api_key("ollama"));
    }

    #[test]
    fn copilot_is_a_supported_local_cli_provider() {
        assert!(supported_providers().contains(&"copilot"));
        assert!(is_local_cli_provider("copilot"));
        assert!(!provider_needs_api_key("copilot"));
    }

    #[test]
    fn apple_is_a_capped_local_cli_provider() {
        assert!(supported_providers().contains(&"apple"));
        assert!(is_local_cli_provider("apple"));
        assert!(!provider_needs_api_key("apple"));
        assert_eq!(provider_max_tokens_input("apple"), Some(6_000));
        assert_eq!(provider_max_tokens_input("openai"), None);
    }
}
