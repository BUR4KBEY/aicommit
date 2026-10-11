use anyhow::Result;

use crate::{ai::ChatMessage, config::Config};

const DEFAULT_SPLIT_PROMPT: &str = include_str!("../../prompts/split-system.md");
const SPLIT_SUMMARY_PROMPT: &str = "You are helping split one staged change set into multiple commits.\n\nOutput contract:\n- Return 2-5 terse Markdown bullet points.\n- Summarize distinct change concerns visible in this diff chunk only.\n- Mention concrete files or subsystems when they help separate concerns.\n- Do not return JSON, commit messages, headings, or prose paragraphs.\n\nLanguage:\nUse {{language}}.\n\n{{context_instruction}}";

pub fn build_split_plan_messages(
    config: &Config,
    diff: &str,
    context: &str,
    staged_files: &[String],
) -> Result<Vec<ChatMessage>> {
    Ok(vec![
        ChatMessage::system(split_system_prompt(config, context)?),
        ChatMessage::user(split_diff_input(diff, context, staged_files)),
    ])
}

pub fn build_split_chunk_summary_messages(
    config: &Config,
    diff: &str,
    context: &str,
    staged_files: &[String],
    chunk_index: usize,
    chunk_total: usize,
) -> Result<Vec<ChatMessage>> {
    let summary_context = if context.trim().is_empty() {
        format!(
            "This is diff chunk {} of {} for one staged change set.",
            chunk_index, chunk_total
        )
    } else {
        format!(
            "Additional user context: <context>{}</context>. This is diff chunk {} of {} for one staged change set.",
            context.trim(),
            chunk_index,
            chunk_total
        )
    };

    Ok(vec![
        ChatMessage::system(
            SPLIT_SUMMARY_PROMPT
                .replace("{{language}}", &config.language)
                .replace("{{context_instruction}}", &summary_context),
        ),
        ChatMessage::user(split_diff_input(diff, context, staged_files)),
    ])
}

pub fn build_split_synthesis_messages(
    config: &Config,
    partial_summaries: &[String],
    context: &str,
    staged_files: &[String],
) -> Result<Vec<ChatMessage>> {
    Ok(vec![
        ChatMessage::system(split_system_prompt(config, context)?),
        ChatMessage::user(split_summary_input(
            partial_summaries,
            context,
            staged_files,
        )),
    ])
}

pub fn split_system_prompt(config: &Config, context: &str) -> Result<String> {
    let context_instruction = if context.trim().is_empty() {
        String::new()
    } else {
        format!(
            "Additional user context: <context>{}</context>. Use it when relevant.",
            context.trim()
        )
    };

    Ok(DEFAULT_SPLIT_PROMPT
        .replace("{{language}}", &config.language)
        .replace("{{split_max}}", &config.split_max.to_string())
        .replace("{{context_instruction}}", &context_instruction))
}

fn split_diff_input(diff: &str, context: &str, staged_files: &[String]) -> String {
    let metadata = split_metadata(context, staged_files);
    format!("{metadata}\n\nStaged diff:\n```diff\n{diff}\n```")
}

fn split_summary_input(
    partial_summaries: &[String],
    context: &str,
    staged_files: &[String],
) -> String {
    let metadata = split_metadata(context, staged_files);
    let summaries = partial_summaries
        .iter()
        .enumerate()
        .map(|(index, summary)| format!("Chunk {}:\n{}", index + 1, summary.trim()))
        .collect::<Vec<_>>()
        .join("\n\n");

    format!("{metadata}\n\nPartial summaries from a large staged diff:\n{summaries}")
}

fn split_metadata(context: &str, staged_files: &[String]) -> String {
    let file_lines = if staged_files.is_empty() {
        "- none".to_owned()
    } else {
        staged_files
            .iter()
            .map(|file| format!("- {file}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let context_line = if context.trim().is_empty() {
        "none".to_owned()
    } else {
        context.trim().to_owned()
    };

    format!("User context: {context_line}\nStaged files:\n{file_lines}")
}
