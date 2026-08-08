use anyhow::{Result, bail};

use crate::{config::Config, errors::AicError, git, prompt::detect_scope_hints, ui};

use self::{
    git_sync::enforce_pre_commit_sync_guard,
    helpers::{CommitInput, CommitInputSource, amend_commit_input, enrich_context_with_branch},
    split::{generate_confirm_and_commit, maybe_execute_split_flow, should_offer_split},
};

mod git_sync;
mod helpers;
mod push;
mod split;
mod staging;

pub async fn run(
    extra_args: Vec<String>,
    context: String,
    full_gitmoji_spec: bool,
    skip_confirmation: bool,
    dry_run: bool,
    amend: bool,
    provider_override: Option<String>,
) -> Result<()> {
    git::assert_git_repo()?;
    let config = Config::load_with_provider_override(provider_override.as_deref())?;

    if config.provider_needs_api_key() && config.api_key.is_none() {
        bail!(AicError::MissingApiKey(config.ai_provider));
    }

    enforce_pre_commit_sync_guard(&config, dry_run).await?;

    ui::section(if amend {
        "Amend session"
    } else {
        "Commit session"
    });

    let (files, commit_input, files_already_listed) = if amend {
        let files = git::last_commit_files()?;
        if files.is_empty() {
            bail!("no files in the last commit to amend");
        }
        let commit_input = amend_commit_input(&files)?;
        (files, commit_input, false)
    } else {
        let staging = ensure_staged_files(skip_confirmation, true).await?;
        let staged = git::staged_files()?;
        if staged.is_empty() {
            bail!(AicError::NoChanges);
        }
        let commit_input = staged_commit_input(&staged)?;
        (staged, commit_input, staging.listed_files)
    };

    if commit_input.content.trim().is_empty() {
        bail!("no commit context available after applying ignore and binary filters");
    }

    render_commit_session(
        &config,
        &files,
        &commit_input,
        amend,
        &context,
        files_already_listed,
    );

    let mut effective_args = extra_args;
    if amend && !effective_args.iter().any(|a| a == "--amend") {
        effective_args.push("--amend".to_owned());
    }

    let context = enrich_context_with_branch(&context);

    if commit_input.source == CommitInputSource::Diff
        && should_offer_split(files.len(), skip_confirmation, dry_run, amend)
        && maybe_execute_split_flow(
            &config,
            &commit_input.content,
            &effective_args,
            &context,
            full_gitmoji_spec,
            &files,
        )
        .await?
    {
        return Ok(());
    }

    generate_confirm_and_commit(
        &config,
        &commit_input.content,
        &effective_args,
        &context,
        full_gitmoji_spec,
        skip_confirmation,
        dry_run,
        &files,
    )
    .await
}

fn render_commit_session(
    config: &Config,
    files: &[String],
    commit_input: &CommitInput,
    amend: bool,
    context: &str,
    files_already_listed: bool,
) {
    match commit_input.source {
        CommitInputSource::Diff => {
            let change_target = if amend {
                "last commit diff"
            } else {
                "staged diff"
            };
            ui::session_step(format!(
                "Reading {change_target} ({}, {} lines)",
                ui::file_count_label(files.len()),
                commit_input.content.lines().count()
            ));
        }
        CommitInputSource::Metadata => {
            let change_target = if amend {
                "last commit change metadata"
            } else {
                "staged change metadata"
            };
            ui::session_step(format!(
                "Reading {change_target} ({})",
                ui::file_count_label(files.len())
            ));
            ui::secondary(
                "No readable diff content was available, so aic is using file paths and Git change types instead of file contents.",
            );
        }
    }

    let mut context_items = Vec::new();
    if let Some(branch) = git::current_branch() {
        context_items.push(branch);
    }
    context_items.push(format!("{}/{}", config.ai_provider, config.model));
    if let Some(ticket) = git::ticket_from_branch() {
        context_items.push(format!("ticket: {ticket}"));
    }
    let scope_hints = detect_scope_hints(files);
    if !scope_hints.is_empty() {
        context_items.push(format!("scopes: {}", scope_hints.join(", ")));
    }
    if !context.trim().is_empty() {
        context_items.push("extra context provided".to_owned());
    }
    ui::metadata_row(&context_items);
    if !files_already_listed {
        ui::file_list(
            if amend {
                "Last commit changes"
            } else {
                "Staged changes"
            },
            files,
        );
    }
}

pub(crate) use helpers::{apply_message_template, filtered_extra_args, staged_commit_input};
pub(crate) use staging::ensure_staged_files;
