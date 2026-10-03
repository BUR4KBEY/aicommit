use std::{env, path::PathBuf};

use anyhow::{Result, bail};

use crate::{config::Config, git};

mod execution;
mod path;

pub use execution::CommandEngine;

const APPLE_RESPOND_ARGS: &[&str] = &[
    "respond",
    "--no-stream",
    "--greedy",
    "--guardrails",
    "permissive-content-transformations",
];

const COPILOT_EXCLUDED_TOOLS: &str = "bash,read_bash,write_bash,stop_bash,list_bash,create,edit,apply_patch,web_fetch,task,read_agent,list_agents,ask_user";

impl CommandEngine {
    pub fn new(config: Config) -> Result<Self> {
        let cwd = git::repo_root().or_else(|_| env::current_dir())?;
        match config.ai_provider.as_str() {
            "claude-code" => Ok(Self::with_command(config, "claude", ["-p"], cwd)),
            "codex" => Ok(Self::with_command(config, "codex", ["exec"], cwd)),
            "copilot" => Ok(Self::with_command(
                config,
                "copilot",
                [
                    "-s",
                    "--no-ask-user",
                    "--excluded-tools",
                    COPILOT_EXCLUDED_TOOLS,
                ],
                cwd,
            )),
            "apple" => Ok(Self::with_command(config, "fm", APPLE_RESPOND_ARGS, cwd)
                .with_instructions_flag("-i")),
            unsupported => bail!("provider '{unsupported}' is not supported by the command engine"),
        }
    }

    pub(crate) fn with_command<S, I>(config: Config, program: S, args: I, cwd: PathBuf) -> Self
    where
        S: Into<String>,
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        Self {
            config,
            program: program.into(),
            args: args
                .into_iter()
                .map(|arg| arg.as_ref().to_owned())
                .collect(),
            cwd,
            instructions_flag: None,
        }
    }

    /// Pass system prompts and earlier turns through `flag` instead of stdin,
    /// leaving only the final user message on stdin. Suits CLIs that take
    /// separate instructions and are too small to untangle a flattened transcript.
    pub(crate) fn with_instructions_flag(mut self, flag: impl Into<String>) -> Self {
        self.instructions_flag = Some(flag.into());
        self
    }

    fn provider_label(&self) -> &'static str {
        match self.config.ai_provider.as_str() {
            "claude-code" => "claude-code",
            "codex" => "codex",
            "copilot" => "copilot",
            "apple" => "apple",
            _ => "command provider",
        }
    }

    fn binary_hint(&self) -> String {
        if self.args.is_empty() {
            self.program.clone()
        } else {
            format!("{} {}", self.program, self.args.join(" "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copilot_provider_uses_standalone_cli_in_text_only_mode() {
        let engine = CommandEngine::new(Config {
            ai_provider: "copilot".to_owned(),
            model: "default".to_owned(),
            ..Config::default()
        })
        .unwrap();

        assert_eq!(engine.program, "copilot");
        assert_eq!(
            engine.args,
            vec![
                "-s".to_owned(),
                "--no-ask-user".to_owned(),
                "--excluded-tools".to_owned(),
                COPILOT_EXCLUDED_TOOLS.to_owned(),
            ]
        );
    }

    #[test]
    fn apple_provider_uses_fm_respond_with_instructions_flag() {
        let engine = CommandEngine::new(Config {
            ai_provider: "apple".to_owned(),
            model: "default".to_owned(),
            ..Config::default()
        })
        .unwrap();

        assert_eq!(engine.program, "fm");
        assert_eq!(engine.args, APPLE_RESPOND_ARGS);
        assert_eq!(engine.instructions_flag.as_deref(), Some("-i"));
    }
}
