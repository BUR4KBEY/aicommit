use anyhow::{Result, bail};

use crate::{
    cli::SplitMode,
    config::Config,
    errors::AicError,
    generator, git,
    output::{
        JsonCommit, JsonOutput, JsonSplitGroup, OutputMode, emit_json, json_error_output,
        split_message,
    },
};

use super::{
    apply_message_template,
    auto_split::{AutoSplitOutcome, run_auto_split},
    git_sync::enforce_pre_commit_sync_guard,
    helpers::{
        CommitInputSource, amend_commit_input, append_commit_history, enrich_context_with_branch,
    },
    push::{PushPlan, build_push_plan, execute_push_plan},
    staged_commit_input,
};
/// Machine entry point for `commit`: no prompts, no decoration.
/// JSON mode prints exactly one JSON line on success; errors print exactly one
/// JSON `error` object and return `Err`. Quiet mode prints the decisive line(s).
#[allow(clippy::too_many_arguments)]
pub(super) async fn run_machine(
    extra_args: Vec<String>,
    context: String,
    full_gitmoji_spec: bool,
    skip_confirmation: bool,
    dry_run: bool,
    amend: bool,
    provider_override: Option<String>,
    split: SplitMode,
    output: OutputMode,
) -> Result<()> {
    // Resolve provider/model for JSON errors. Config load itself can fail;
    // fall back to override/default then. Note: AIC_MODEL env / config file
    // can change the model; resolved in run_machine_inner on success paths.
    let fallback_provider = provider_override
        .clone()
        .unwrap_or_else(|| Config::default().ai_provider);
    let fallback_model = Config::default().model;

    let outcome = run_machine_inner(
        extra_args,
        &context,
        full_gitmoji_spec,
        skip_confirmation,
        dry_run,
        amend,
        provider_override.as_deref(),
        split,
        output,
    )
    .await;

    match outcome {
        Ok(JsonOutput {
            command,
            dry_run,
            provider,
            model,
            message,
            commits,
            split_plan,
            error: None,
        }) if output.is_json() => {
            emit_json(&JsonOutput {
                command,
                dry_run,
                provider,
                model,
                message,
                commits,
                split_plan,
                error: None,
            });
            Ok(())
        }
        Ok(JsonOutput {
            message,
            commits,
            error: None,
            ..
        }) => {
            // Quiet mode: message (dry-run) or `hash subject` per commit.
            if let Some(commits) = commits.filter(|c| !c.is_empty()) {
                for commit in &commits {
                    match &commit.hash {
                        Some(hash) => println!("{hash} {}", commit.subject),
                        None => println!("{}", commit.subject),
                    }
                }
            } else if let Some(message) = message {
                println!("{message}");
            }
            Ok(())
        }
        Ok(JsonOutput {
            command,
            dry_run,
            provider,
            model,
            error: Some(error),
            ..
        }) => {
            // Unreachable today: failures return Err below. Kept total so a
            // future partial-success path cannot leak prose to stdout.
            debug_assert!(false, "machine flow returned error payload as Ok");
            emit_json(&JsonOutput {
                command,
                dry_run,
                provider,
                model,
                message: None,
                commits: None,
                split_plan: None,
                error: Some(error),
            });
            bail!("machine flow failed");
        }
        Err(error) => {
            if output.is_json() {
                // Best-effort: re-resolve config for accurate provider/model
                // in the error envelope (cheap, no network).
                let (provider, model) =
                    match Config::load_with_provider_override(provider_override.as_deref()) {
                        Ok(config) => (config.ai_provider, config.model),
                        Err(_) => (fallback_provider, fallback_model),
                    };
                emit_json(&json_error_output(
                    "commit", dry_run, &provider, &model, &error,
                ));
            } else {
                eprintln!("Error: {error:#}");
            }
            Err(error)
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_machine_inner(
    mut extra_args: Vec<String>,
    context: &str,
    full_gitmoji_spec: bool,
    skip_confirmation: bool,
    dry_run: bool,
    amend: bool,
    provider_override: Option<&str>,
    split: SplitMode,
    output: OutputMode,
) -> Result<JsonOutput> {
    git::assert_git_repo()?;
    let config = Config::load_with_provider_override(provider_override)?;

    if config.provider_needs_api_key() && config.api_key.is_none() {
        bail!(AicError::MissingApiKey(config.ai_provider));
    }

    if amend && !extra_args.iter().any(|a| a == "--amend") {
        extra_args.push("--amend".to_owned());
    }
    let context = enrich_context_with_branch(context);

    if !skip_confirmation && !dry_run && output.is_json() {
        bail!("--json requires -y (or -d for dry-run) because commit confirmation is interactive");
    }
    // Quiet implies -y: no prompts, auto-stage/auto-push when unambiguous.
    let effective_skip = skip_confirmation || matches!(output, OutputMode::Quiet);

    enforce_pre_commit_sync_guard(&config, dry_run).await?;

    let (files, commit_input) = if amend {
        let files = git::last_commit_files()?;
        if files.is_empty() {
            bail!("no files in the last commit to amend");
        }
        let commit_input = amend_commit_input(&files)?;
        (files, commit_input)
    } else {
        machine_ensure_staged(effective_skip)?;
        let staged = git::staged_files()?;
        if staged.is_empty() {
            bail!(AicError::NoChanges);
        }
        let commit_input = staged_commit_input(&staged)?;
        (staged, commit_input)
    };

    if commit_input.content.trim().is_empty() {
        bail!("no commit context available after applying ignore and binary filters");
    }

    if commit_input.source == CommitInputSource::Diff && split == SplitMode::Auto && !amend {
        match run_auto_split(
            &config,
            &commit_input.content,
            &extra_args,
            &context,
            full_gitmoji_spec,
            split,
            effective_skip,
            dry_run,
            amend,
            &files,
        )
        .await?
        {
            AutoSplitOutcome::SingleCommit => {}
            AutoSplitOutcome::DryRun(groups) => {
                return Ok(JsonOutput {
                    command: "commit".to_owned(),
                    dry_run: true,
                    provider: config.ai_provider.clone(),
                    model: config.model.clone(),
                    message: None,
                    commits: Some(vec![]),
                    split_plan: Some(groups.iter().map(JsonSplitGroup::from).collect()),
                    error: None,
                });
            }
            AutoSplitOutcome::Committed(commits) => {
                let split_plan: Vec<JsonSplitGroup> = commits
                    .iter()
                    .map(|commit| JsonSplitGroup::from(&commit.draft.group))
                    .collect();
                let commits: Vec<JsonCommit> = commits
                    .into_iter()
                    .map(|commit| {
                        let (subject, body) = split_message(&commit.draft.message);
                        JsonCommit {
                            hash: Some(commit.hash),
                            subject,
                            body,
                            files: commit.draft.group.files,
                        }
                    })
                    .collect();
                return Ok(JsonOutput {
                    command: "commit".to_owned(),
                    dry_run: false,
                    provider: config.ai_provider.clone(),
                    model: config.model.clone(),
                    message: None,
                    commits: Some(commits),
                    split_plan: Some(split_plan),
                    error: None,
                });
            }
        }
    }

    let message = apply_message_template(
        &config,
        &extra_args,
        &generator::generate_commit_message(
            &config,
            &commit_input.content,
            full_gitmoji_spec,
            &context,
            &files,
            None,
        )
        .await?,
    );

    if dry_run {
        return Ok(JsonOutput {
            command: "commit".to_owned(),
            dry_run: true,
            provider: config.ai_provider.clone(),
            model: config.model.clone(),
            message: Some(message),
            commits: Some(vec![]),
            split_plan: None,
            error: None,
        });
    }

    let (hash, subject, body) = machine_commit_and_push(&config, &message, &extra_args).await?;

    append_commit_history(&config, &message, &files);

    Ok(JsonOutput {
        command: "commit".to_owned(),
        dry_run: false,
        provider: config.ai_provider.clone(),
        model: config.model.clone(),
        message: Some(message),
        commits: Some(vec![JsonCommit {
            hash: Some(hash),
            subject,
            body,
            files,
        }]),
        split_plan: None,
        error: None,
    })
}

/// Non-interactive staging: mirror `ensure_staged_files` for the `-y` path.
/// Errors instead of prompting when nothing is staged and `-y` is absent.
fn machine_ensure_staged(skip_confirmation: bool) -> Result<()> {
    let staged = git::staged_files()?;
    let changed = git::changed_files()?;

    if changed.is_empty() && staged.is_empty() {
        bail!(AicError::NoChanges);
    }
    if !staged.is_empty() {
        return Ok(());
    }
    if skip_confirmation {
        git::add_files(&changed)?;
        return Ok(());
    }
    bail!("no files staged; stage files or rerun with -y to stage all changed files");
}

/// Commit and push without prompts. Returns `(hash, subject, body)`.
/// Mirrors `push::commit_and_maybe_push` minus all UI; push plan
/// `ConfirmSingle`/`SelectRemote` cannot occur because machine flows force
/// `skip_confirmation`-style resolution (auto-push iff exactly one remote).
async fn machine_commit_and_push(
    config: &Config,
    message: &str,
    extra_args: &[String],
) -> Result<(String, String, Option<String>)> {
    use super::filtered_extra_args;

    let filtered = filtered_extra_args(config, extra_args);
    git::commit(message, &filtered)?;
    // Amend rewrites HEAD; hash must be read after commit in both cases.
    let hash = git::head_short_hash()?;
    let (subject, body) = split_message(message);

    // Push: reuse plan builder with forced skip_confirmation semantics so a
    // single remote auto-pushes and multiple remotes error (same as -y).
    let plan = build_push_plan(
        config.gitpush,
        true,
        &git::remote_metadata()?,
        &config.remote_icon_style,
    )?;
    match plan {
        PushPlan::Skip => {}
        PushPlan::AutoPush(remote) => {
            // execute_push_plan(AutoPush) performs no prompts.
            execute_push_plan(PushPlan::AutoPush(remote), config, true).await?;
        }
        // Unreachable: build_push_plan with skip=true only yields Skip/AutoPush
        // (or bails on multi-remote). Match explicitly to fail loudly if that
        // ever changes instead of silently prompting.
        PushPlan::ConfirmSingle { .. } | PushPlan::SelectRemote(_) => {
            bail!(
                "machine mode cannot choose a push remote; set AIC_GITPUSH=false or configure exactly one remote"
            );
        }
    }

    Ok((hash, subject, body))
}
