use anyhow::{Result, bail};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{
    ai::{anthropic::AnthropicEngine, command::CommandEngine, openai_compat::OpenAiCompatEngine},
    config::Config,
};

pub mod anthropic;
pub mod command;
pub mod openai_compat;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_owned(),
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_owned(),
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_owned(),
            content: content.into(),
        }
    }
}

#[async_trait]
pub trait AiEngine: Send + Sync {
    /// Send chat messages and return the model's text response.
    /// Used for commit generation, review, and any other text completion task.
    async fn generate_commit_message(&self, messages: &[ChatMessage]) -> Result<String>;
}

pub fn engine_from_config(config: &Config) -> Result<Box<dyn AiEngine>> {
    match config.ai_provider.as_str() {
        "openai" | "azure-openai" | "groq" | "ollama" => {
            Ok(Box::new(OpenAiCompatEngine::new(config.clone())?))
        }
        "anthropic" => Ok(Box::new(AnthropicEngine::new(config.clone())?)),
        "claude-code" | "codex" | "copilot" | "apple" => {
            Ok(Box::new(CommandEngine::new(config.clone())?))
        }
        "test" => Ok(Box::new(TestEngine)),
        unsupported => {
            bail!(
                "provider '{unsupported}' is not supported; use openai, azure-openai, anthropic, groq, ollama, claude-code, codex, copilot, or apple"
            )
        }
    }
}

struct TestEngine;

#[async_trait]
impl AiEngine for TestEngine {
    async fn generate_commit_message(&self, messages: &[ChatMessage]) -> Result<String> {
        let system = messages
            .iter()
            .find(|message| message.role == "system")
            .map(|message| message.content.as_str())
            .unwrap_or_default();
        let user = messages
            .iter()
            .rev()
            .find(|message| message.role == "user")
            .map(|message| message.content.as_str())
            .unwrap_or_default();

        if system.contains("Return valid JSON only.") {
            let files = extract_split_files(user);
            return Ok(test_split_plan_response(&files));
        }

        if system.contains("Git recovery guidance") {
            if user.contains("commit_created: true") {
                return Ok(
                    "The commit is local, but the branch needs to be rebased before you push.\n\n1. `git pull --rebase`\n2. `git push`"
                        .to_owned(),
                );
            }

            return Ok(
                "The branch needs to be synced before `aic` can continue.\n\n1. `git pull --rebase`\n2. `aic`"
                    .to_owned(),
            );
        }

        if user.contains("README.md") {
            return Ok("docs: update readme".to_owned());
        }
        if user.contains("src/lib.rs") {
            return Ok("feat: update library".to_owned());
        }

        Ok("feat: add generated commit message".to_owned())
    }
}

fn extract_split_files(input: &str) -> Vec<String> {
    let mut files = Vec::new();
    let mut in_files = false;

    for line in input.lines() {
        if line.trim() == "Staged files:" {
            in_files = true;
            continue;
        }

        if !in_files {
            continue;
        }

        if let Some(file) = line.trim().strip_prefix("- ") {
            files.push(file.to_owned());
        } else if !line.trim().is_empty() {
            break;
        }
    }

    files
}

fn test_split_plan_response(files: &[String]) -> String {
    if files.len() < 2 {
        return r#"{"groups":[{"title":"one","rationale":"single change","files":["src.txt"]}]}"#
            .to_owned();
    }

    let midpoint = 1.min(files.len() - 1);
    let first = serde_json::to_string(&files[..midpoint]).unwrap_or_else(|_| "[]".to_owned());
    let second = serde_json::to_string(&files[midpoint..]).unwrap_or_else(|_| "[]".to_owned());
    format!(
        r#"{{"groups":[{{"title":"Primary change","rationale":"Keep the first concern separate","files":{first}}},{{"title":"Follow-up change","rationale":"Keep the remaining concern separate","files":{second}}}]}}"#
    )
}
