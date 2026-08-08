use std::{fs, path::Path};

use anyhow::Result;
use toml_edit::DocumentMut;

use super::{
    Config,
    load::{apply_file, apply_value},
    model::default_model_for_provider,
    validate::validate_config,
};

pub fn set_global_config(key_values: &[(String, String)], global_path: &Path) -> Result<Config> {
    let mut config = Config::default();
    apply_file(&mut config, global_path)?;

    for (key, value) in key_values {
        apply_value(&mut config, key, value)?;
    }

    let provider_was_set = key_values.iter().any(|(key, _)| key == "AIC_AI_PROVIDER");
    let model_was_set = key_values.iter().any(|(key, _)| key == "AIC_MODEL");
    if provider_was_set && !model_was_set {
        config.model = default_model_for_provider(&config.ai_provider).to_owned();
    }

    validate_config(&config)?;
    write_global_config(&config, global_path)?;
    Ok(config)
}

pub fn write_global_config(config: &Config, global_path: &Path) -> Result<()> {
    if let Some(parent) = global_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut doc = DocumentMut::new();
    for (key, value) in config.as_key_values() {
        if value.is_empty() {
            continue;
        }

        doc[key] = match key {
            "AIC_TOKENS_MAX_INPUT" | "AIC_TOKENS_MAX_OUTPUT" | "AIC_HTTP_TIMEOUT" => {
                value.parse::<i64>()?.into()
            }
            "AIC_DESCRIPTION"
            | "AIC_EMOJI"
            | "AIC_ONE_LINE_COMMIT"
            | "AIC_OMIT_SCOPE"
            | "AIC_GITPUSH"
            | "AIC_HOOK_AUTO_UNCOMMENT" => value.parse::<bool>()?.into(),
            _ => value.into(),
        };
    }

    fs::write(global_path, doc.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn set_global_config_writes_defaults_and_values() {
        let temp = TempDir::new().unwrap();
        let global = temp.path().join(".aicommit");

        let config = set_global_config(
            &[
                ("AIC_API_KEY".to_owned(), "key".to_owned()),
                ("AIC_EMOJI".to_owned(), "true".to_owned()),
            ],
            &global,
        )
        .unwrap();

        assert_eq!(config.api_key.as_deref(), Some("key"));
        assert!(config.emoji);
        let content = std::fs::read_to_string(global).unwrap();
        assert!(content.contains("AIC_API_KEY"));
        assert!(content.contains("AIC_EMOJI"));
    }

    #[test]
    fn setting_remote_provider_without_model_uses_provider_default_model() {
        let temp = TempDir::new().unwrap();
        let global = temp.path().join(".aicommit");

        let config = set_global_config(
            &[("AIC_AI_PROVIDER".to_owned(), "groq".to_owned())],
            &global,
        )
        .unwrap();

        assert_eq!(config.ai_provider, "groq");
        assert_eq!(config.model, "llama-3.1-8b-instant");
    }
}
