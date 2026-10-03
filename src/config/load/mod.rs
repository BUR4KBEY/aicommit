use std::env;

use anyhow::Result;

use super::{
    Config, ConfigPaths,
    model::{default_model_for_provider, is_local_cli_provider, provider_max_tokens_input},
    validate::validate_config,
};

mod apply;

pub use apply::{apply_file, apply_process_env, apply_toml_item, apply_value, optional_string};

pub fn load_from_with_provider_override(
    paths: &ConfigPaths,
    provider_override: Option<&str>,
) -> Result<Config> {
    let mut config = Config::default();
    apply_file(&mut config, &paths.global)?;
    apply_process_env(&mut config)?;

    if let Some(provider) = provider_override {
        let previous_provider = config.ai_provider.clone();
        let previous_model = config.model.clone();
        apply_value(&mut config, "AIC_AI_PROVIDER", provider)?;
        if is_local_cli_provider(&config.ai_provider)
            || previous_model == default_model_for_provider(&previous_provider)
        {
            config.model = default_model_for_provider(&config.ai_provider).to_owned();
        }
    }

    if let Some(limit) = provider_max_tokens_input(&config.ai_provider) {
        config.tokens_max_input = config.tokens_max_input.min(limit);
    }

    if config.proxy.is_none() {
        config.proxy = env::var("HTTPS_PROXY")
            .ok()
            .or_else(|| env::var("HTTP_PROXY").ok());
    }

    validate_config(&config)?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use crate::config::{
        Config, ConfigPaths, model::provider_needs_api_key, write::set_global_config,
    };

    #[test]
    fn load_from_ignores_neighbor_dotenv_file() {
        let temp = TempDir::new().unwrap();
        let global = temp.path().join(".aicommit");
        let dotenv_path = temp.path().join(".env");
        std::fs::write(
            &global,
            "AIC_API_KEY = \"global\"\nAIC_MODEL = \"gpt-5.4-mini\"\n",
        )
        .unwrap();
        std::fs::write(
            &dotenv_path,
            "AIC_API_KEY=local\nsid=1:abc; _cfuvid=value; uid=123\nAIC_MODEL=gpt-5.4\n",
        )
        .unwrap();

        let config = Config::load_from(&ConfigPaths { global }).unwrap();

        assert_eq!(config.api_key.as_deref(), Some("global"));
        assert_eq!(config.model, "gpt-5.4-mini");
    }

    #[test]
    fn normalizes_claudecode_provider_alias() {
        let temp = TempDir::new().unwrap();
        let global = temp.path().join(".aicommit");
        std::fs::write(&global, "AIC_AI_PROVIDER = \"claudecode\"\n").unwrap();

        let config = Config::load_from(&ConfigPaths { global }).unwrap();

        assert_eq!(config.ai_provider, "claude-code");
    }

    #[test]
    fn accepts_local_cli_providers() {
        let temp = TempDir::new().unwrap();
        let global = temp.path().join(".aicommit");
        std::fs::write(
            &global,
            "AIC_AI_PROVIDER = \"codex\"\nAIC_MODEL = \"default\"\n",
        )
        .unwrap();

        let config = Config::load_from(&ConfigPaths { global }).unwrap();

        assert_eq!(config.ai_provider, "codex");
    }

    #[test]
    fn accepts_copilot_as_local_cli_provider() {
        let temp = TempDir::new().unwrap();
        let global = temp.path().join(".aicommit");
        std::fs::write(
            &global,
            "AIC_AI_PROVIDER = \"copilot\"\nAIC_MODEL = \"default\"\n",
        )
        .unwrap();

        let config = Config::load_from(&ConfigPaths { global }).unwrap();

        assert_eq!(config.ai_provider, "copilot");
        assert_eq!(config.model, "default");
    }

    #[test]
    fn accepts_new_remote_providers() {
        let temp = TempDir::new().unwrap();
        let global = temp.path().join(".aicommit");
        std::fs::write(
            &global,
            "AIC_AI_PROVIDER = \"anthropic\"\nAIC_MODEL = \"claude-sonnet-4-20250514\"\n",
        )
        .unwrap();

        let config = Config::load_from(&ConfigPaths { global }).unwrap();

        assert_eq!(config.ai_provider, "anthropic");
    }

    #[test]
    fn local_cli_providers_do_not_need_api_keys() {
        let config = Config {
            ai_provider: "claude-code".to_owned(),
            ..Config::default()
        };

        assert!(!config.provider_needs_api_key());
        assert!(!provider_needs_api_key("codex"));
        assert!(!provider_needs_api_key("copilot"));
    }

    #[test]
    fn provider_override_switches_ollama_to_provider_default() {
        let temp = TempDir::new().unwrap();
        let global = temp.path().join(".aicommit");
        std::fs::write(
            &global,
            "AIC_AI_PROVIDER = \"openai\"\nAIC_MODEL = \"gpt-5.4-mini\"\n",
        )
        .unwrap();

        let config =
            Config::load_from_with_provider_override(&ConfigPaths { global }, Some("ollama"))
                .unwrap();

        assert_eq!(config.ai_provider, "ollama");
        assert_eq!(config.model, "llama3.2");
    }

    #[test]
    fn provider_override_switches_local_cli_model_to_default() {
        let temp = TempDir::new().unwrap();
        let global = temp.path().join(".aicommit");
        std::fs::write(
            &global,
            "AIC_AI_PROVIDER = \"openai\"\nAIC_MODEL = \"gpt-5.4\"\n",
        )
        .unwrap();

        let config =
            Config::load_from_with_provider_override(&ConfigPaths { global }, Some("codex"))
                .unwrap();

        assert_eq!(config.ai_provider, "codex");
        assert_eq!(config.model, "default");
    }

    #[test]
    fn provider_override_switches_copilot_model_to_default() {
        let temp = TempDir::new().unwrap();
        let global = temp.path().join(".aicommit");
        std::fs::write(
            &global,
            "AIC_AI_PROVIDER = \"openai\"\nAIC_MODEL = \"gpt-5.4\"\n",
        )
        .unwrap();

        let config =
            Config::load_from_with_provider_override(&ConfigPaths { global }, Some("copilot"))
                .unwrap();

        assert_eq!(config.ai_provider, "copilot");
        assert_eq!(config.model, "default");
    }

    #[test]
    fn provider_override_switches_remote_default_model_to_provider_default() {
        let temp = TempDir::new().unwrap();
        let global = temp.path().join(".aicommit");
        std::fs::write(
            &global,
            "AIC_AI_PROVIDER = \"openai\"\nAIC_MODEL = \"gpt-5.4-mini\"\n",
        )
        .unwrap();

        let config =
            Config::load_from_with_provider_override(&ConfigPaths { global }, Some("anthropic"))
                .unwrap();

        assert_eq!(config.ai_provider, "anthropic");
        assert_eq!(config.model, "claude-sonnet-4-20250514");
    }

    #[test]
    fn provider_override_preserves_explicit_remote_model() {
        let temp = TempDir::new().unwrap();
        let global = temp.path().join(".aicommit");
        std::fs::write(
            &global,
            "AIC_AI_PROVIDER = \"openai\"\nAIC_MODEL = \"gpt-5.4\"\n",
        )
        .unwrap();

        let config =
            Config::load_from_with_provider_override(&ConfigPaths { global }, Some("groq"))
                .unwrap();

        assert_eq!(config.ai_provider, "groq");
        assert_eq!(config.model, "gpt-5.4");
    }

    #[test]
    fn setting_local_provider_without_model_uses_default_model() {
        let temp = TempDir::new().unwrap();
        let global = temp.path().join(".aicommit");

        let config = set_global_config(
            &[("AIC_AI_PROVIDER".to_owned(), "claudecode".to_owned())],
            &global,
        )
        .unwrap();

        assert_eq!(config.ai_provider, "claude-code");
        assert_eq!(config.model, "default");
    }

    #[test]
    fn apple_provider_caps_token_input_and_accepts_fm_alias() {
        let temp = TempDir::new().unwrap();
        let global = temp.path().join(".aicommit");
        std::fs::write(&global, "AIC_AI_PROVIDER = \"fm\"\n").unwrap();

        let config = Config::load_from(&ConfigPaths { global }).unwrap();

        assert_eq!(config.ai_provider, "apple");
        assert_eq!(config.tokens_max_input, 6_000);
    }

    #[test]
    fn apple_provider_override_keeps_lower_explicit_token_input() {
        let temp = TempDir::new().unwrap();
        let global = temp.path().join(".aicommit");
        std::fs::write(
            &global,
            "AIC_AI_PROVIDER = \"openai\"\nAIC_TOKENS_MAX_INPUT = 4000\n",
        )
        .unwrap();

        let config =
            Config::load_from_with_provider_override(&ConfigPaths { global }, Some("apple"))
                .unwrap();

        assert_eq!(config.ai_provider, "apple");
        assert_eq!(config.tokens_max_input, 4_000);
    }
}
