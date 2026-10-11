use std::env;

use anyhow::Result;

use super::{
    Config, ConfigOverrides, ConfigPaths,
    model::{default_model_for_provider, is_local_cli_provider, provider_max_tokens_input},
    validate::validate_config,
};

mod apply;

pub use apply::{apply_file, apply_process_env, apply_toml_item, apply_value, optional_string};

pub fn load_from_with_overrides(
    paths: &ConfigPaths,
    overrides: &ConfigOverrides,
) -> Result<Config> {
    // Model stays empty until a layer sets AIC_MODEL, so an unset model
    // follows the resolved provider instead of the openai default.
    let mut config = Config {
        model: String::new(),
        ..Config::default()
    };
    apply_file(&mut config, &paths.global)?;
    apply_process_env(&mut config)?;
    if config.model.is_empty() {
        config.model = default_model_for_provider(&config.ai_provider).to_owned();
    }

    if let Some(provider) = overrides.provider.as_deref() {
        let previous_provider = config.ai_provider.clone();
        let previous_model = config.model.clone();
        apply_value(&mut config, "AIC_AI_PROVIDER", provider)?;
        if is_local_cli_provider(&config.ai_provider)
            || previous_model == default_model_for_provider(&previous_provider)
        {
            config.model = default_model_for_provider(&config.ai_provider).to_owned();
        }
    }

    if let Some(model) = overrides.model.as_deref() {
        config.model = model.to_owned();
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
    use std::path::PathBuf;

    use tempfile::TempDir;

    use crate::config::{
        Config, ConfigOverrides, ConfigPaths, model::provider_needs_api_key,
        write::set_global_config,
    };

    fn load_with(global: PathBuf, provider: Option<&str>, model: Option<&str>) -> Config {
        let overrides = ConfigOverrides {
            provider: provider.map(str::to_owned),
            model: model.map(str::to_owned),
        };
        Config::load_from_with_overrides(&ConfigPaths { global }, &overrides).unwrap()
    }

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

        let config = load_with(global, Some("ollama"), None);

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

        let config = load_with(global, Some("codex"), None);

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

        let config = load_with(global, Some("copilot"), None);

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

        let config = load_with(global, Some("anthropic"), None);

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

        let config = load_with(global, Some("groq"), None);

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

        let config = load_with(global, Some("apple"), None);

        assert_eq!(config.ai_provider, "apple");
        assert_eq!(config.tokens_max_input, 4_000);
    }

    #[test]
    fn model_override_wins_over_file_model_and_provider_override() {
        let temp = TempDir::new().unwrap();
        let global = temp.path().join(".aicommit");
        std::fs::write(
            &global,
            "AIC_AI_PROVIDER = \"openai\"\nAIC_MODEL = \"gpt-5.4\"\n",
        )
        .unwrap();

        let config = load_with(global.clone(), None, Some("chosen"));
        assert_eq!(config.ai_provider, "openai");
        assert_eq!(config.model, "chosen");

        let config = load_with(global, Some("anthropic"), Some("chosen"));
        assert_eq!(config.ai_provider, "anthropic");
        assert_eq!(config.model, "chosen");
    }

    #[test]
    fn provider_without_model_uses_provider_default_model() {
        let temp = TempDir::new().unwrap();
        let global = temp.path().join(".aicommit");
        std::fs::write(&global, "AIC_AI_PROVIDER = \"opencode-go\"\n").unwrap();

        let config = load_with(global, None, None);

        assert_eq!(config.ai_provider, "opencode-go");
        assert_eq!(config.model, "glm-5.3-flash");
    }

    #[test]
    fn explicit_model_matching_openai_default_is_kept_for_other_provider() {
        let temp = TempDir::new().unwrap();
        let global = temp.path().join(".aicommit");
        std::fs::write(
            &global,
            "AIC_AI_PROVIDER = \"opencode-go\"\nAIC_MODEL = \"gpt-5.4-mini\"\n",
        )
        .unwrap();

        let config = load_with(global, None, None);

        assert_eq!(config.model, "gpt-5.4-mini");
    }
}
