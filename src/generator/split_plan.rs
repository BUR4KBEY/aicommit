use std::collections::BTreeSet;

use anyhow::{Result, bail};
use serde::Deserialize;

use crate::{
    ai::engine_from_config,
    config::Config,
    prompt::{
        SplitPlanGroup, build_split_chunk_summary_messages, build_split_plan_messages,
        build_split_synthesis_messages, sanitize_model_output,
    },
    token::{count_messages, split_diff},
};

use super::{GenerationProgress, ProgressFn, report};

const TOKEN_ADJUSTMENT: usize = 20;

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
        let messages = build_split_plan_messages(config, &chunks[0], context, staged_files)?;
        let response = engine.generate_commit_message(&messages).await?;
        return parse_split_plan_response(&response, staged_files);
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
    let response = engine.generate_commit_message(&synthesis_messages).await?;
    parse_split_plan_response(&response, staged_files)
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
