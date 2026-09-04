use anyhow::{Result, bail};

use crate::{
    ai::engine_from_config,
    config::Config,
    errors::AicError,
    git, history_store,
    prompt::{build_review_messages, detect_scope_hints, review_system_prompt},
    token::{count_messages, count_tokens, split_diff},
    ui,
};

const TOKEN_ADJUSTMENT: usize = 20;

pub async fn run(context: String, provider_override: Option<String>) -> Result<()> {
    git::assert_git_repo()?;
    let config = Config::load_with_provider_override(provider_override.as_deref())?;

    if config.provider_needs_api_key() && config.api_key.is_none() {
        bail!(AicError::MissingApiKey(config.ai_provider));
    }

    ui::section("Review session");
    super::commit::ensure_staged_files(false, false).await?;
    let staged = git::staged_files()?;
    if staged.is_empty() {
        bail!(AicError::NoChanges);
    }

    let diff = git::staged_diff(&staged)?;
    if diff.trim().is_empty() {
        bail!("no diff content available after applying ignore and binary filters");
    }

    ui::session_step(format!(
        "Reading staged diff ({}, {} lines)",
        ui::file_count_label(staged.len()),
        diff.lines().count()
    ));
    let mut context_items = Vec::new();
    if let Some(branch) = git::current_branch() {
        context_items.push(branch);
    }
    context_items.push(format!("{}/{}", config.ai_provider, config.model));
    if let Some(ticket) = git::ticket_from_branch() {
        context_items.push(format!("ticket: {ticket}"));
    }
    let scopes = detect_scope_hints(&staged);
    if !scopes.is_empty() {
        context_items.push(format!("scopes: {}", scopes.join(", ")));
    }
    if !context.trim().is_empty() {
        context_items.push("extra context provided".to_owned());
    }
    ui::metadata_row(&context_items);
    ui::file_list("Staged changes", &staged);
    let spinner = ui::StatusSpinner::start("Analyzing changes", ui::StatusPool::Waiting);
    let progress = |event: crate::generator::GenerationProgress| {
        spinner.on_generation_progress(event, "review")
    };
    let review = generate_review(&config, &diff, &context, Some(&progress)).await;
    spinner.finish_and_clear();

    let review = review?;
    ui::markdown_card("AI review", &review);

    let history_saved = match history_store::append_entry(&history_store::HistoryEntry {
        timestamp: history_store::now_iso8601(),
        kind: "review".to_owned(),
        message: review.clone(),
        repo_path: git::repo_root()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        files: staged.clone(),
        provider: config.ai_provider.clone(),
        model: config.model.clone(),
    }) {
        Ok(()) => true,
        Err(e) => {
            ui::warn(format!("failed to save history: {e}"));
            false
        }
    };

    ui::section("Review complete");
    let mut completion_items = vec![format!("analyzed: {}", ui::file_count_label(staged.len()))];
    if history_saved {
        completion_items.push("history: saved".to_owned());
    }
    ui::metadata_row(&completion_items);
    ui::hint(
        "Next: update the staged changes and run `aic review` again, or run `aic` when you're ready to draft a commit.",
    );

    Ok(())
}

async fn generate_review(
    config: &Config,
    diff: &str,
    context: &str,
    progress: Option<crate::generator::ProgressFn<'_>>,
) -> Result<String> {
    use crate::generator::GenerationProgress;

    let report = |event: GenerationProgress| {
        if let Some(callback) = progress {
            callback(event);
        }
    };

    let system_prompt = review_system_prompt(config, context)?;
    let system_tokens = count_messages(&[crate::ai::ChatMessage::system(&system_prompt)]);
    let max_request_tokens = config
        .tokens_max_input
        .saturating_sub(config.tokens_max_output)
        .saturating_sub(system_tokens)
        .saturating_sub(TOKEN_ADJUSTMENT);

    report(GenerationProgress::Splitting);
    let chunks = split_diff(diff, max_request_tokens.max(1))?;
    let engine = engine_from_config(config)?;

    if chunks.len() == 1 {
        report(GenerationProgress::Chunk {
            current: 1,
            total: 1,
        });
        let messages = build_review_messages(config, &chunks[0], context)?;
        return engine.generate_commit_message(&messages).await;
    }

    let mut partial_reviews = Vec::with_capacity(chunks.len());
    for (i, chunk) in chunks.iter().enumerate() {
        report(GenerationProgress::Chunk {
            current: i + 1,
            total: chunks.len(),
        });
        let chunk_context = format!(
            "{context}\nThis is diff chunk {} of {}. List findings for this chunk only.",
            i + 1,
            chunks.len()
        );
        let messages = build_review_messages(config, chunk, &chunk_context)?;
        partial_reviews.push(engine.generate_commit_message(&messages).await?);
    }

    report(GenerationProgress::Synthesizing);
    let per_chunk_budget = max_request_tokens / partial_reviews.len().max(1);
    let synthesis_parts: Vec<String> = partial_reviews
        .iter()
        .enumerate()
        .map(|(i, review)| {
            let header = format!("--- Chunk {} ---\n", i + 1);
            let available = per_chunk_budget.saturating_sub(count_tokens(&header));
            let mut truncated = String::new();
            let mut used_tokens = 0usize;
            for line in review.lines() {
                let line_tokens = count_tokens(line) + 1;
                if used_tokens + line_tokens > available {
                    truncated.push_str("[...truncated]\n");
                    break;
                }
                truncated.push_str(line);
                truncated.push('\n');
                used_tokens += line_tokens;
            }
            format!("{header}{truncated}")
        })
        .collect();

    let synthesis_input = format!(
        "Partial review findings from a large staged diff:\n{}\n\nSynthesize into one unified review. Deduplicate, prioritize by severity, and present a cohesive summary.",
        synthesis_parts.join("\n\n")
    );
    let messages = build_review_messages(config, &synthesis_input, context)?;
    engine.generate_commit_message(&messages).await
}
