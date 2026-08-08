use anyhow::Result;

use crate::{
    ai::engine_from_config,
    config::Config,
    git::CommitInfo,
    prompt::{
        PullRequestDraft, build_pr_chunk_summary_messages, build_pr_messages,
        build_pr_synthesis_messages, parse_pull_request_response,
    },
    token::{count_messages, split_diff},
};

use super::{GenerationProgress, ProgressFn, report};

const TOKEN_ADJUSTMENT: usize = 20;

#[allow(clippy::too_many_arguments)]
pub async fn generate_pull_request(
    config: &Config,
    diff: &str,
    context: &str,
    base_ref: &str,
    branch_name: Option<&str>,
    ticket: Option<&str>,
    commits: &[CommitInfo],
    changed_files: &[String],
    progress: Option<ProgressFn<'_>>,
) -> Result<PullRequestDraft> {
    let prompt_tokens = count_messages(&build_pr_messages(
        config,
        "",
        context,
        base_ref,
        branch_name,
        ticket,
        commits,
        changed_files,
    )?);
    let max_request_tokens = config
        .tokens_max_input
        .saturating_sub(config.tokens_max_output)
        .saturating_sub(prompt_tokens)
        .saturating_sub(TOKEN_ADJUSTMENT);

    report(progress, GenerationProgress::Splitting);
    let chunks = split_diff(diff, max_request_tokens.max(1))?;
    let engine = engine_from_config(config)?;

    if chunks.len() == 1 {
        report(
            progress,
            GenerationProgress::Chunk {
                current: 1,
                total: 1,
            },
        );
        let messages = build_pr_messages(
            config,
            &chunks[0],
            context,
            base_ref,
            branch_name,
            ticket,
            commits,
            changed_files,
        )?;
        let response = engine.generate_commit_message(&messages).await?;
        return parse_pull_request_response(&response);
    }

    let mut partial_summaries = Vec::with_capacity(chunks.len());
    for (index, chunk) in chunks.iter().enumerate() {
        report(
            progress,
            GenerationProgress::Chunk {
                current: index + 1,
                total: chunks.len(),
            },
        );
        let messages = build_pr_chunk_summary_messages(
            config,
            chunk,
            context,
            base_ref,
            branch_name,
            ticket,
            commits,
            changed_files,
            index + 1,
            chunks.len(),
        )?;
        partial_summaries.push(engine.generate_commit_message(&messages).await?);
    }

    report(progress, GenerationProgress::Synthesizing);
    let synthesis_messages = build_pr_synthesis_messages(
        config,
        &partial_summaries,
        context,
        base_ref,
        branch_name,
        ticket,
        commits,
        changed_files,
    )?;
    let response = engine.generate_commit_message(&synthesis_messages).await?;
    parse_pull_request_response(&response)
}
