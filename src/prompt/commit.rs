use std::{collections::BTreeSet, fs, path::Path};

use anyhow::{Context, Result};

use crate::{
    ai::ChatMessage,
    config::{Config, provider_uses_compact_prompts},
};

const DEFAULT_SYSTEM_PROMPT: &str = include_str!("../../prompts/commit-system.md");
const COMPACT_SYSTEM_PROMPT: &str = include_str!("../../prompts/commit-system-apple.md");
const PROSE_EXTENSIONS: &[&str] = &["md", "mdx", "markdown", "txt", "rst", "adoc"];
const PROSE_COLLAPSE_MIN_LINES: usize = 40;
const PROSE_KEEP_LINES: usize = 20;
const SHORT_GITMOJI_HELP: &str = "If an emoji is useful, use only one GitMoji prefix: 🐛 fix, ✨ feature, 📝 docs, 🚀 deploy, ✅ tests, ♻️ refactor, ⬆️ dependencies, 🔧 config, 🌐 localization, or 💡 comments.";
const FULL_GITMOJI_HELP: &str = "If an emoji is useful, use one GitMoji prefix that best matches the whole change. Prefer the official intent of each emoji; never stack multiple emojis.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitPlanGroup {
    pub title: String,
    pub rationale: String,
    pub files: Vec<String>,
}

pub fn build_messages(
    config: &Config,
    diff: &str,
    full_gitmoji_spec: bool,
    context: &str,
    staged_files: &[String],
) -> Result<Vec<ChatMessage>> {
    let mut messages = initial_messages(config, full_gitmoji_spec, context, staged_files)?;
    messages.push(ChatMessage::user(diff));
    Ok(messages)
}

pub fn initial_messages(
    config: &Config,
    full_gitmoji_spec: bool,
    context: &str,
    staged_files: &[String],
) -> Result<Vec<ChatMessage>> {
    Ok(vec![
        ChatMessage::system(system_prompt(
            config,
            full_gitmoji_spec,
            context,
            staged_files,
        )?),
        ChatMessage::user(example_diff()),
        ChatMessage::assistant(example_commit(config)),
    ])
}

pub fn system_prompt(
    config: &Config,
    full_gitmoji_spec: bool,
    context: &str,
    staged_files: &[String],
) -> Result<String> {
    let convention = if config.emoji {
        if full_gitmoji_spec {
            FULL_GITMOJI_HELP
        } else {
            SHORT_GITMOJI_HELP
        }
    } else {
        "Use conventional commit keywords only: fix, feat, build, chore, ci, docs, style, refactor, perf, or test."
    };

    let body_instruction = if config.description {
        "After the subject, add one blank line, then 2-4 tight bullet points. Each bullet should explain a meaningful change or why it matters. Do not repeat the subject."
    } else {
        "Return only the subject line. Do not add a body, bullet list, markdown, or explanation."
    };

    let line_mode_instruction = if config.one_line_commit {
        "Use exactly one concise subject line."
    } else {
        "Use a subject plus body when body output is enabled. Keep the body scannable and useful in GitHub's commit view."
    };

    let scope_instruction = if config.omit_scope {
        "Do not include a scope; use '<type>: <subject>' when using conventional commits."
            .to_owned()
    } else {
        let base = "Use at most one scope, and only when it clarifies the single overall change.";
        let hints = detect_scope_hints(staged_files);
        if hints.is_empty() {
            base.to_owned()
        } else {
            format!(
                "{base} Likely scopes based on changed files: {}.",
                hints.join(", ")
            )
        }
    };

    let context_instruction = if context.trim().is_empty() {
        String::new()
    } else {
        format!(
            "Additional user context: <context>{}</context>. Use it when relevant.",
            context.trim()
        )
    };

    let template = prompt_template(config)?;
    Ok(template
        .replace("{{commit_convention}}", convention)
        .replace("{{body_instruction}}", body_instruction)
        .replace("{{line_mode_instruction}}", line_mode_instruction)
        .replace("{{scope_instruction}}", &scope_instruction)
        .replace("{{style_examples}}", &style_examples(config))
        .replace("{{language}}", &config.language)
        .replace("{{context_instruction}}", &context_instruction))
}

pub fn detect_scope_hints(files: &[String]) -> Vec<String> {
    let mut scopes = BTreeSet::new();

    for file in files {
        let path = Path::new(file);
        let components: Vec<_> = path.iter().filter_map(|c| c.to_str()).collect();

        let scope = match components.as_slice() {
            ["Cargo.toml"] | ["Cargo.lock"] => Some("deps"),
            ["docs", ..] | ["README.md"] | ["CLAUDE.md"] | ["AGENTS.md"] => Some("docs"),
            ["tests", ..] => Some("test"),
            ["prompts", ..] => Some("prompt"),
            [".github", ..] => Some("ci"),
            ["src", "ai", ..] => Some("ai"),
            ["src", "commands", ..] => Some("cli"),
            ["src", "map", ..] => Some("map"),
            ["src", file] => {
                let stem = Path::new(file)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("");
                match stem {
                    "config" => Some("config"),
                    "git" => Some("git"),
                    "prompt" => Some("prompt"),
                    "ui" => Some("ui"),
                    "token" => Some("token"),
                    "generator" => Some("generator"),
                    "errors" => Some("errors"),
                    _ => None,
                }
            }
            _ => None,
        };

        if let Some(s) = scope {
            scopes.insert(s.to_owned());
        }
    }

    scopes.into_iter().take(5).collect()
}

fn style_examples(config: &Config) -> String {
    let prompt_subject = if config.emoji {
        "✨ feat(prompt): make commit generation prompt-driven and resilient"
    } else {
        "feat(prompt): make commit generation prompt-driven and resilient"
    };
    let diff_subject = if config.emoji {
        "🐛 fix(diff): prevent oversized staged changes from aborting commits"
    } else {
        "fix(diff): prevent oversized staged changes from aborting commits"
    };

    format!(
        "{prompt_subject}\n\n- Move the default system prompt into a reusable template\n- Teach generation to synthesize chunked diffs into one polished message\n- Document how to tune prompt behavior without rebuilding\n\n{diff_subject}\n\n- Split oversized diff lines instead of failing the whole generation flow\n- Raise the default input budget for newer OpenAI models"
    )
}

fn prompt_template(config: &Config) -> Result<String> {
    match &config.prompt_file {
        Some(path) => fs::read_to_string(path)
            .with_context(|| format!("failed to read prompt template from {path}")),
        None if provider_uses_compact_prompts(&config.ai_provider) => {
            Ok(COMPACT_SYSTEM_PROMPT.to_owned())
        }
        None => Ok(DEFAULT_SYSTEM_PROMPT.to_owned()),
    }
}

/// Small models summarise whatever text dominates the diff, so a new blog post
/// or README swamps the real change. Lead with a per-file outline and trim long
/// added prose files to their opening lines (enough for a title and summary).
pub fn compact_diff_for_small_model(diff: &str) -> String {
    let sections = file_sections(diff);
    if sections.is_empty() {
        return diff.to_owned();
    }

    let mut outline = String::from("Staged files:\n");
    let mut body = String::with_capacity(diff.len());
    if let Some(preamble) = diff.get(..sections[0].0) {
        body.push_str(preamble);
    }

    for (_, section) in sections {
        let path = section
            .lines()
            .next()
            .and_then(|line| line.strip_prefix("diff --git a/"))
            .and_then(|rest| rest.split(" b/").next())
            .unwrap_or("unknown");
        let (header, hunks) =
            section.split_at(section.find("\n@@").map_or(section.len(), |i| i + 1));
        let is_new = header.contains("\nnew file mode");
        let added = hunks
            .lines()
            .filter(|line| line.starts_with('+') && !line.starts_with("+++"))
            .count();
        let removed = hunks
            .lines()
            .filter(|line| line.starts_with('-') && !line.starts_with("---"))
            .count();
        outline.push_str(&format!(
            "- {} {path} (+{added} -{removed})\n",
            if is_new { "added" } else { "modified" }
        ));

        if !(is_new && is_prose_file(path) && added > PROSE_COLLAPSE_MIN_LINES) {
            body.push_str(section);
            continue;
        }

        body.push_str(header);
        let mut kept = 0;
        for line in hunks.lines() {
            if line.starts_with('+') {
                if kept == PROSE_KEEP_LINES {
                    continue;
                }
                kept += 1;
            }
            body.push_str(line);
            body.push('\n');
        }
        body.push_str(&format!(
            "[... {} more added lines of prose omitted ...]\n",
            added - kept
        ));
    }

    format!("{outline}\n{body}")
}

fn file_sections(diff: &str) -> Vec<(usize, &str)> {
    let starts: Vec<usize> = diff
        .match_indices("diff --git ")
        .map(|(index, _)| index)
        .filter(|&index| index == 0 || diff.as_bytes()[index - 1] == b'\n')
        .collect();
    starts
        .iter()
        .enumerate()
        .map(|(i, &start)| {
            let end = starts.get(i + 1).copied().unwrap_or(diff.len());
            (start, &diff[start..end])
        })
        .collect()
}

fn is_prose_file(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| PROSE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

fn example_diff() -> String {
    r#"diff --git a/src/server.rs b/src/server.rs
--- a/src/server.rs
+++ b/src/server.rs
@@ -1,5 +1,5 @@
-let port = 7799;
+let port = std::env::var("PORT").unwrap_or_else(|_| "7799".into());"#
        .to_owned()
}

fn example_commit(config: &Config) -> String {
    let prefix = if config.emoji { "✨ " } else { "" };
    if config.omit_scope {
        format!("{prefix}feat: read server port from environment")
    } else {
        format!("{prefix}feat(server): read port from environment")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_uses_context_and_aic_config() {
        let config = Config {
            emoji: true,
            ..Config::default()
        };
        let prompt = system_prompt(&config, false, "issue 123", &[]).unwrap();
        assert!(prompt.contains("issue 123"));
        assert!(prompt.contains("GitMoji"));
    }

    #[test]
    fn scope_hints_detects_known_directories() {
        let files = vec![
            "src/ai/openai_compat.rs".to_owned(),
            "src/ai/mod.rs".to_owned(),
        ];
        assert_eq!(detect_scope_hints(&files), vec!["ai"]);
    }

    #[test]
    fn scope_hints_detects_multiple_scopes() {
        let files = vec![
            "src/commands/commit.rs".to_owned(),
            "src/git.rs".to_owned(),
            "Cargo.toml".to_owned(),
        ];
        let hints = detect_scope_hints(&files);
        assert_eq!(hints, vec!["cli", "deps", "git"]);
    }

    #[test]
    fn scope_hints_caps_at_five() {
        let files = vec![
            "src/ai/mod.rs".to_owned(),
            "src/commands/commit.rs".to_owned(),
            "src/config.rs".to_owned(),
            "src/git.rs".to_owned(),
            "src/token.rs".to_owned(),
            "src/ui.rs".to_owned(),
            "docs/roadmap.md".to_owned(),
        ];
        assert_eq!(detect_scope_hints(&files).len(), 5);
    }

    #[test]
    fn scope_hints_empty_for_no_files() {
        assert!(detect_scope_hints(&[]).is_empty());
    }

    #[test]
    fn scope_hints_appear_in_prompt_when_not_omitted() {
        let config = Config::default();
        assert!(!config.omit_scope);
        let files = vec!["src/git.rs".to_owned()];
        let prompt = system_prompt(&config, false, "", &files).unwrap();
        assert!(prompt.contains("Likely scopes based on changed files: git."));
    }

    #[test]
    fn scope_hints_absent_when_omit_scope() {
        let config = Config {
            omit_scope: true,
            ..Config::default()
        };
        let files = vec!["src/git.rs".to_owned()];
        let prompt = system_prompt(&config, false, "", &files).unwrap();
        assert!(!prompt.contains("Likely scopes"));
    }

    #[test]
    fn compact_diff_outlines_files_and_trims_long_added_prose() {
        let post: String = (1..=60).map(|n| format!("+line {n}\n")).collect();
        let diff = format!(
            "diff --git a/scripts/cover.js b/scripts/cover.js\nindex 1..2 100644\n--- a/scripts/cover.js\n+++ b/scripts/cover.js\n@@ -1 +1,2 @@\n-old\n+new\n+more\ndiff --git a/posts/new.mdx b/posts/new.mdx\nnew file mode 100644\nindex 0..3\n--- /dev/null\n+++ b/posts/new.mdx\n@@ -0,0 +1,60 @@\n{post}"
        );

        let compact = compact_diff_for_small_model(&diff);

        assert!(compact.starts_with(
            "Staged files:\n- modified scripts/cover.js (+2 -1)\n- added posts/new.mdx (+60 -0)\n\n"
        ));
        assert!(compact.contains("+new\n+more\n"));
        assert!(compact.contains("+line 20\n[... 40 more added lines of prose omitted ...]"));
        assert!(!compact.contains("+line 21\n"));
    }

    #[test]
    fn compact_diff_keeps_short_prose_and_code_files_intact() {
        let diff = "diff --git a/README.md b/README.md\nnew file mode 100644\n--- /dev/null\n+++ b/README.md\n@@ -0,0 +1,2 @@\n+# Title\n+Body\n";
        let compact = compact_diff_for_small_model(diff);
        assert!(compact.ends_with(diff));
        assert_eq!(
            compact_diff_for_small_model("metadata only"),
            "metadata only"
        );
    }

    #[test]
    fn apple_provider_uses_compact_prompt_without_style_examples() {
        let config = Config {
            ai_provider: "apple".to_owned(),
            ..Config::default()
        };
        let prompt = system_prompt(&config, false, "", &[]).unwrap();
        assert!(prompt.contains("Text inside an added file"));
        assert!(!prompt.contains("prevent oversized staged changes"));
        assert!(!prompt.contains("{{"));

        let default = system_prompt(&Config::default(), false, "", &[]).unwrap();
        assert!(default.contains("prevent oversized staged changes"));
    }
}
