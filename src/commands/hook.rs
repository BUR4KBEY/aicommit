use std::{env, fs, path::PathBuf};

use anyhow::Result;

use crate::{
    commands::commit::staged_commit_input, config::Config, errors::AicError, generator, git, ui,
};

pub fn set() -> Result<()> {
    git::assert_git_repo()?;
    let binary = current_binary()?;
    let path = git::write_hook(&binary)?;
    ui::section("Hook installed");
    ui::secondary(format!("path: {}", path.display()));
    Ok(())
}

pub fn unset() -> Result<()> {
    git::assert_git_repo()?;
    let binary = current_binary()?;
    match git::remove_hook_if_owned(&binary)? {
        Some(path) => {
            ui::section("Hook removed");
            ui::secondary(format!("path: {}", path.display()));
        }
        None => ui::info("no aic hook was set"),
    }
    Ok(())
}

pub async fn run_hook(message_file: String, commit_source: Option<String>) -> Result<()> {
    if commit_source.is_some() {
        return Ok(());
    }

    let config = Config::load()?;
    if config.provider_needs_api_key() && config.api_key.is_none() {
        return Err(AicError::MissingApiKey(config.ai_provider).into());
    }

    let staged = git::staged_files()?;
    if staged.is_empty() {
        return Ok(());
    }

    let commit_input = staged_commit_input(&staged)?;
    if commit_input.content.trim().is_empty() {
        return Ok(());
    }

    // git invokes this from prepare-commit-msg with no UI of its own; a plain
    // stderr line keeps hook users informed without assuming a TTY.
    eprintln!(
        "aic: generating commit message via {}/{} ...",
        config.ai_provider, config.model
    );
    let message = generator::generate_commit_message(
        &config,
        &commit_input.content,
        false,
        "",
        &staged,
        None,
    )
    .await?;
    let existing = fs::read_to_string(&message_file)?;
    let content = if config.hook_auto_uncomment {
        format!("{message}\n\n{existing}")
    } else {
        format!(
            "# {message}\n\n# ---------- [aic] ---------- #\n# Remove the leading # above to use this generated message.\n# Close the editor without saving to cancel the commit.\n\n{existing}"
        )
    };
    fs::write(message_file, content)?;
    Ok(())
}

fn current_binary() -> Result<PathBuf> {
    env::current_exe().or_else(|_| {
        env::args_os()
            .next()
            .map(PathBuf::from)
            .ok_or_else(|| anyhow::anyhow!("could not determine current executable"))
    })
}
