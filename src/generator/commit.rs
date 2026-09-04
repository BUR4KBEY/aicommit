use anyhow::Result;

use crate::{
    ai::engine_from_config,
    config::Config,
    prompt::{build_messages, initial_messages},
    token::{count_messages, split_diff},
};

use super::{GenerationProgress, ProgressFn, report};

const TOKEN_ADJUSTMENT: usize = 20;

fn chunk_summary_context(context: &str, current: usize, total: usize) -> String {
    format!(
        "{context}\nThis is diff chunk {current} of {total}. Summarize the change intent in one short phrase for later synthesis. Do not write a final commit message."
    )
}

pub async fn generate_commit_message(
    config: &Config,
    diff: &str,
    full_gitmoji_spec: bool,
    context: &str,
    staged_files: &[String],
    progress: Option<ProgressFn<'_>>,
) -> Result<String> {
    // Budget against the chunk-summary prompt variant - the largest context
    // this function sends. Measuring the bare context under-reserves and can
    // trip the engine's token guard on tightly-packed chunks.
    let prompt_tokens = count_messages(&initial_messages(
        config,
        full_gitmoji_spec,
        &chunk_summary_context(context, 9999, 9999),
        staged_files,
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
        let chat_messages =
            build_messages(config, &chunks[0], full_gitmoji_spec, context, staged_files)?;
        return engine.generate_commit_message(&chat_messages).await;
    }

    let mut summaries = Vec::with_capacity(chunks.len());
    for (index, chunk) in chunks.iter().enumerate() {
        report(
            progress,
            GenerationProgress::Chunk {
                current: index + 1,
                total: chunks.len(),
            },
        );
        let chunk_context = chunk_summary_context(context, index + 1, chunks.len());
        let chat_messages = build_messages(
            config,
            chunk,
            full_gitmoji_spec,
            &chunk_context,
            staged_files,
        )?;
        summaries.push(engine.generate_commit_message(&chat_messages).await?);
    }

    report(progress, GenerationProgress::Synthesizing);
    let synthesis_input = format!(
        "Partial summaries from a large staged diff:\n{}\n\nSynthesize these into exactly one final commit message.",
        summaries
            .iter()
            .enumerate()
            .map(|(index, summary)| format!("{}. {}", index + 1, summary))
            .collect::<Vec<_>>()
            .join("\n")
    );
    let synthesis_context = format!(
        "{context}\nYou are synthesizing partial summaries from one staged diff. Return exactly one final commit message, not one line per summary."
    );
    let chat_messages = build_messages(
        config,
        &synthesis_input,
        full_gitmoji_spec,
        &synthesis_context,
        staged_files,
    )?;
    engine.generate_commit_message(&chat_messages).await
}
