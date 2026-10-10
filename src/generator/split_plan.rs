use std::collections::BTreeSet;

use anyhow::{Result, bail};
use serde::Deserialize;

use crate::{
    ai::{AiEngine, ChatMessage, Generation, GenerationRequest, engine_from_config},
    config::Config,
    prompt::{
        SplitPlanGroup, build_split_chunk_summary_messages, build_split_plan_messages,
        build_split_synthesis_messages, sanitize_model_output,
    },
    token::{count_messages, split_diff},
};

use super::{GenerationProgress, ProgressFn, report};

const TOKEN_ADJUSTMENT: usize = 20;

/// Split plans carry several titles, rationales, and file lists, so they run
/// longer than a single commit message. `AIC_TOKENS_MAX_OUTPUT` (500 by
/// default) truncates them mid-JSON, so plan calls keep their own floor.
const SPLIT_PLAN_MIN_OUTPUT_TOKENS: usize = 4096;

/// Multiplier for the single retry issued when the provider reports it ran
/// out of output budget mid-plan.
const SPLIT_PLAN_RETRY_FACTOR: usize = 4;

/// Output cap for one plan call. Never below the structured-artifact floor and
/// never beyond what the input window can spare; the bumped retry drops to the
/// same ceiling when the window is too small to grow into.
fn split_plan_output_cap(config: &Config, prompt_tokens: usize, bumped: bool) -> usize {
    let headroom = config.tokens_max_input.saturating_sub(prompt_tokens).max(1);
    let base = config
        .tokens_max_output
        .max(SPLIT_PLAN_MIN_OUTPUT_TOKENS)
        .min(headroom);
    if bumped {
        base.saturating_mul(SPLIT_PLAN_RETRY_FACTOR).min(headroom)
    } else {
        base
    }
}

/// Ask for the plan. When the provider says it stopped on the output cap, ask
/// once more with a bigger cap before the caller degrades to a single commit;
/// anything else that fails to parse is a model bug a bigger cap cannot fix.
async fn request_split_plan(
    engine: &dyn AiEngine,
    messages: &[ChatMessage],
    staged_files: &[String],
    config: &Config,
    prompt_tokens: usize,
) -> Result<Vec<SplitPlanGroup>> {
    let cap = split_plan_output_cap(config, prompt_tokens, false);
    let response = request_plan(engine, messages, cap).await?;
    if !response.truncated {
        return parse_split_plan_response(&response.text, staged_files);
    }

    let retry_cap = split_plan_output_cap(config, prompt_tokens, true);
    if retry_cap > cap {
        let retry = request_plan(engine, messages, retry_cap).await?;
        if !retry.truncated {
            return parse_split_plan_response(&retry.text, staged_files);
        }
        return Err(truncated_plan_error(retry_cap, config));
    }

    Err(truncated_plan_error(cap, config))
}

async fn request_plan(
    engine: &dyn AiEngine,
    messages: &[ChatMessage],
    cap: usize,
) -> Result<Generation> {
    engine
        .generate_with_options(
            messages,
            &GenerationRequest {
                max_output_tokens: Some(cap),
            },
        )
        .await
}

/// Concrete cause plus the fix, so a degraded split never surfaces as a bare
/// `EOF while parsing a string`.
fn truncated_plan_error(cap: usize, config: &Config) -> anyhow::Error {
    anyhow::anyhow!(
        "the split plan response was truncated at the {cap}-token plan cap; \
         raise AIC_TOKENS_MAX_OUTPUT (currently {}) or AIC_TOKENS_MAX_INPUT ({}) to fit a longer plan",
        config.tokens_max_output,
        config.tokens_max_input
    )
}

#[derive(Debug, Deserialize)]
struct SplitPlanResponse {
    groups: Vec<SplitPlanResponseGroup>,
}

#[derive(Debug, Deserialize)]
struct SplitPlanResponseGroup {
    title: String,
    rationale: String,
    files: Vec<String>,
}

pub async fn generate_split_plan(
    config: &Config,
    diff: &str,
    context: &str,
    staged_files: &[String],
    progress: Option<ProgressFn<'_>>,
) -> Result<Vec<SplitPlanGroup>> {
    let prompt_tokens = count_messages(&build_split_plan_messages(
        config,
        "",
        context,
        staged_files,
    )?);
    let max_request_tokens = config
        .tokens_max_input
        .saturating_sub(split_plan_output_cap(config, prompt_tokens, false))
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
        let messages = build_split_plan_messages(config, &chunks[0], context, staged_files)?;
        return request_split_plan(
            engine.as_ref(),
            &messages,
            staged_files,
            config,
            prompt_tokens,
        )
        .await;
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
        let messages = build_split_chunk_summary_messages(
            config,
            chunk,
            context,
            staged_files,
            index + 1,
            chunks.len(),
        )?;
        partial_summaries.push(engine.generate_commit_message(&messages).await?);
    }

    report(progress, GenerationProgress::Synthesizing);
    let synthesis_messages =
        build_split_synthesis_messages(config, &partial_summaries, context, staged_files)?;
    request_split_plan(
        engine.as_ref(),
        &synthesis_messages,
        staged_files,
        config,
        prompt_tokens,
    )
    .await
}

fn parse_split_plan_response(input: &str, staged_files: &[String]) -> Result<Vec<SplitPlanGroup>> {
    let normalized = sanitize_model_output(input);
    let json = extract_json_payload(&normalized);
    let response: SplitPlanResponse = serde_json::from_str(&json)
        .map_err(|error| anyhow::anyhow!("failed to parse split plan JSON: {error}"))?;
    validate_split_plan(response.groups, staged_files)
}

fn extract_json_payload(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.starts_with("```") {
        let lines = trimmed.lines().collect::<Vec<_>>();
        if lines.len() >= 3 {
            return lines[1..lines.len() - 1].join("\n").trim().to_owned();
        }
    }
    trimmed.to_owned()
}

fn validate_split_plan(
    groups: Vec<SplitPlanResponseGroup>,
    staged_files: &[String],
) -> Result<Vec<SplitPlanGroup>> {
    if groups.len() < 2 {
        bail!("split plan must contain at least 2 groups");
    }
    if groups.len() > 4 {
        bail!("split plan must contain at most 4 groups");
    }

    let staged_set = staged_files.iter().cloned().collect::<BTreeSet<_>>();
    let mut seen = BTreeSet::new();
    let mut validated = Vec::with_capacity(groups.len());

    for group in groups {
        if group.files.is_empty() {
            bail!("split plan groups must not be empty");
        }

        let title = group.title.trim().to_owned();
        let rationale = group.rationale.trim().to_owned();
        if title.is_empty() {
            bail!("split plan groups must include a title");
        }
        if rationale.is_empty() {
            bail!("split plan groups must include a rationale");
        }

        let mut files = Vec::with_capacity(group.files.len());
        for file in group.files {
            let trimmed = file.trim().to_owned();
            if trimmed.is_empty() {
                bail!("split plan contained an empty file path");
            }
            if !staged_set.contains(&trimmed) {
                bail!("split plan referenced unknown file '{trimmed}'");
            }
            if !seen.insert(trimmed.clone()) {
                bail!("split plan referenced '{trimmed}' more than once");
            }
            files.push(trimmed);
        }

        validated.push(SplitPlanGroup {
            title,
            rationale,
            files,
        });
    }

    if seen != staged_set {
        bail!("split plan did not assign every staged file exactly once");
    }

    Ok(validated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_plan_rejects_single_group() {
        let error = parse_split_plan_response(
            r#"{"groups":[{"title":"one","rationale":"one change","files":["src/lib.rs"]}]}"#,
            &["src/lib.rs".to_owned()],
        )
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "split plan must contain at least 2 groups"
        );
    }

    #[test]
    fn split_plan_rejects_unknown_files() {
        let error = parse_split_plan_response(
            r#"{"groups":[
                {"title":"one","rationale":"one change","files":["src/lib.rs"]},
                {"title":"two","rationale":"two change","files":["src/main.rs"]}
            ]}"#,
            &["src/lib.rs".to_owned()],
        )
        .unwrap_err();

        assert!(error.to_string().contains("unknown file"));
    }

    #[test]
    fn split_plan_accepts_valid_groups() {
        let groups = parse_split_plan_response(
            r#"{"groups":[
                {"title":"cli","rationale":"cli changes","files":["src/cli.rs"]},
                {"title":"docs","rationale":"docs changes","files":["README.md"]}
            ]}"#,
            &["src/cli.rs".to_owned(), "README.md".to_owned()],
        )
        .unwrap();

        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].title, "cli");
        assert_eq!(groups[1].files, vec!["README.md".to_owned()]);
    }
}
