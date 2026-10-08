use anyhow::Result;

use crate::{
    config::{
        Config, ConfigPaths, default_model_for_provider, enabled_providers, provider_needs_api_key,
        set_global_config,
    },
    ui,
};

pub async fn run() -> Result<()> {
    // `aic setup` is interactive-only: keep the dedicated wording but exit 2
    // with the same non-interactive hint printed first.
    if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        eprintln!("{}", crate::exit::NO_TTY_HINT);
        anyhow::bail!("aic setup is interactive-only; rerun in a terminal with a TTY on stdin");
    }
    ui::section("Setup");
    let providers = enabled_providers()
        .iter()
        .map(|provider| provider.to_string())
        .collect::<Vec<_>>();
    let provider = ui::select("Select your AI provider", providers)?;

    let mut key_values = vec![
        ("AIC_AI_PROVIDER".to_owned(), provider.clone()),
        (
            "AIC_MODEL".to_owned(),
            default_model_for_provider(&provider).to_owned(),
        ),
    ];

    if provider_needs_api_key(&provider) {
        let api_key = ui::text("Enter your API key", None)?;
        key_values.push(("AIC_API_KEY".to_owned(), api_key));
    }

    if provider == "azure-openai" {
        let api_url = ui::text(
            "Azure OpenAI endpoint",
            Some("https://<resource>.openai.azure.com/openai/v1"),
        )?;
        key_values.push(("AIC_API_URL".to_owned(), api_url));
    }

    let model = ui::text("Model", Some(default_model_for_provider(&provider)))?;
    key_values.retain(|(key, _)| key != "AIC_MODEL");
    key_values.push(("AIC_MODEL".to_owned(), model));

    let paths = ConfigPaths::discover()?;
    set_global_config(&key_values, &paths.global)?;
    let config = Config::load_from(&paths)?;
    ui::blank_line();
    ui::section("Setup complete");
    ui::metadata_row(&[
        format!("provider: {}", config.ai_provider),
        format!("model: {}", config.model),
    ]);
    Ok(())
}
