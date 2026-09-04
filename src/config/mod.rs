use std::{collections::BTreeMap, env, path::PathBuf};

use anyhow::{Context, Result};
use directories::BaseDirs;
use serde::{Deserialize, Serialize};

mod load;
mod model;
mod parse;
mod validate;
mod write;

pub use load::{apply_file, apply_process_env, apply_toml_item, apply_value, optional_string};
pub use model::{
    default_api_url_for_provider, default_model_for_provider, enabled_providers,
    is_local_cli_provider, model_list, provider_needs_api_key, supported_providers,
};
pub use validate::validate_config;
pub use write::{set_global_config, write_global_config};

pub const GLOBAL_CONFIG_FILE: &str = ".aicommit";
pub const MODEL_CACHE_FILE: &str = ".aicommit-models.json";
pub const STATUS_MESSAGES_FILE: &str = ".aicommit-status.toml";
pub const REPO_IGNORE_FILE: &str = ".aicommitignore";
pub const DEFAULT_MAX_TOKENS_INPUT: usize = 128_000;
pub const DEFAULT_MAX_TOKENS_OUTPUT: usize = 500;
pub const DEFAULT_HTTP_TIMEOUT_SECS: usize = 120;

pub const CONFIG_KEYS: &[&str] = &[
    "AIC_AI_PROVIDER",
    "AIC_API_KEY",
    "AIC_API_URL",
    "AIC_API_CUSTOM_HEADERS",
    "AIC_PROXY",
    "AIC_TOKENS_MAX_INPUT",
    "AIC_TOKENS_MAX_OUTPUT",
    "AIC_HTTP_TIMEOUT",
    "AIC_DESCRIPTION",
    "AIC_EMOJI",
    "AIC_MODEL",
    "AIC_LANGUAGE",
    "AIC_MESSAGE_TEMPLATE_PLACEHOLDER",
    "AIC_PROMPT_FILE",
    "AIC_ONE_LINE_COMMIT",
    "AIC_OMIT_SCOPE",
    "AIC_GITPUSH",
    "AIC_REMOTE_ICON_STYLE",
    "AIC_HOOK_AUTO_UNCOMMENT",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Config {
    pub ai_provider: String,
    pub api_key: Option<String>,
    pub api_url: Option<String>,
    pub api_custom_headers: BTreeMap<String, String>,
    pub proxy: Option<String>,
    pub tokens_max_input: usize,
    pub tokens_max_output: usize,
    pub http_timeout: usize,
    pub description: bool,
    pub emoji: bool,
    pub model: String,
    pub language: String,
    pub message_template_placeholder: String,
    pub prompt_file: Option<String>,
    pub one_line_commit: bool,
    pub omit_scope: bool,
    pub gitpush: bool,
    pub remote_icon_style: String,
    pub hook_auto_uncomment: bool,
}

#[derive(Debug, Clone)]
pub struct ConfigPaths {
    pub global: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            ai_provider: "openai".to_owned(),
            api_key: None,
            api_url: None,
            api_custom_headers: BTreeMap::new(),
            proxy: None,
            tokens_max_input: DEFAULT_MAX_TOKENS_INPUT,
            tokens_max_output: DEFAULT_MAX_TOKENS_OUTPUT,
            http_timeout: DEFAULT_HTTP_TIMEOUT_SECS,
            description: true,
            emoji: true,
            model: default_model_for_provider("openai").to_owned(),
            language: "en".to_owned(),
            message_template_placeholder: "$msg".to_owned(),
            prompt_file: None,
            one_line_commit: false,
            omit_scope: false,
            gitpush: true,
            remote_icon_style: "auto".to_owned(),
            hook_auto_uncomment: false,
        }
    }
}

impl ConfigPaths {
    pub fn discover() -> Result<Self> {
        let home = BaseDirs::new()
            .map(|base| base.home_dir().to_path_buf())
            .or_else(|| env::var_os("HOME").map(PathBuf::from))
            .context("could not determine home directory")?;

        Ok(Self {
            global: home.join(GLOBAL_CONFIG_FILE),
        })
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        Self::load_from_with_provider_override(&ConfigPaths::discover()?, None)
    }

    pub fn load_with_provider_override(provider_override: Option<&str>) -> Result<Self> {
        Self::load_from_with_provider_override(&ConfigPaths::discover()?, provider_override)
    }

    pub fn load_from(paths: &ConfigPaths) -> Result<Self> {
        Self::load_from_with_provider_override(paths, None)
    }

    pub fn load_from_with_provider_override(
        paths: &ConfigPaths,
        provider_override: Option<&str>,
    ) -> Result<Self> {
        load::load_from_with_provider_override(paths, provider_override)
    }

    pub fn as_key_values(&self) -> Vec<(&'static str, String)> {
        vec![
            ("AIC_AI_PROVIDER", self.ai_provider.clone()),
            ("AIC_API_KEY", self.api_key.clone().unwrap_or_default()),
            ("AIC_API_URL", self.api_url.clone().unwrap_or_default()),
            (
                "AIC_API_CUSTOM_HEADERS",
                serde_json::to_string(&self.api_custom_headers).unwrap_or_default(),
            ),
            ("AIC_PROXY", self.proxy.clone().unwrap_or_default()),
            ("AIC_TOKENS_MAX_INPUT", self.tokens_max_input.to_string()),
            ("AIC_TOKENS_MAX_OUTPUT", self.tokens_max_output.to_string()),
            ("AIC_HTTP_TIMEOUT", self.http_timeout.to_string()),
            ("AIC_DESCRIPTION", self.description.to_string()),
            ("AIC_EMOJI", self.emoji.to_string()),
            ("AIC_MODEL", self.model.clone()),
            ("AIC_LANGUAGE", self.language.clone()),
            (
                "AIC_MESSAGE_TEMPLATE_PLACEHOLDER",
                self.message_template_placeholder.clone(),
            ),
            (
                "AIC_PROMPT_FILE",
                self.prompt_file.clone().unwrap_or_default(),
            ),
            ("AIC_ONE_LINE_COMMIT", self.one_line_commit.to_string()),
            ("AIC_OMIT_SCOPE", self.omit_scope.to_string()),
            ("AIC_GITPUSH", self.gitpush.to_string()),
            ("AIC_REMOTE_ICON_STYLE", self.remote_icon_style.clone()),
            (
                "AIC_HOOK_AUTO_UNCOMMENT",
                self.hook_auto_uncomment.to_string(),
            ),
        ]
    }

    pub fn get_key(&self, key: &str) -> Option<String> {
        self.as_key_values()
            .into_iter()
            .find_map(|(candidate, value)| (candidate == key).then_some(value))
    }

    pub fn provider_needs_api_key(&self) -> bool {
        provider_needs_api_key(&self.ai_provider)
    }
}

pub fn global_model_cache_path() -> Result<PathBuf> {
    let home = BaseDirs::new()
        .map(|base| base.home_dir().to_path_buf())
        .or_else(|| env::var_os("HOME").map(PathBuf::from))
        .context("could not determine home directory")?;
    Ok(home.join(MODEL_CACHE_FILE))
}

pub fn config_description(key: &str) -> Option<&'static str> {
    crate::cli_text::config_description(key)
}
