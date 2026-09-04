use std::{
    io::Write,
    path::PathBuf,
    process::{Command, Output, Stdio},
};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;

use crate::{
    ai::{AiEngine, ChatMessage},
    config::Config,
    prompt::sanitize_model_output,
};

use super::path::resolve_program_path;

#[derive(Debug, Clone)]
pub struct CommandEngine {
    pub(super) config: Config,
    pub(super) program: String,
    pub(super) args: Vec<String>,
    pub(super) cwd: PathBuf,
}

impl CommandEngine {
    fn render_prompt(messages: &[ChatMessage]) -> String {
        let mut prompt = String::from(
            "Return only the assistant reply for the final user message. Do not add commentary about your process.\n",
        );

        for message in messages {
            prompt.push_str("\n<message role=\"");
            prompt.push_str(&message.role);
            prompt.push_str("\">\n");
            prompt.push_str(&message.content);
            if !message.content.ends_with('\n') {
                prompt.push('\n');
            }
            prompt.push_str("</message>\n");
        }

        prompt
    }

    fn resolved_program(&self) -> Option<PathBuf> {
        resolve_program_path(&self.program)
    }
}

enum CommandIoError {
    Spawn(std::io::Error),
    Prompt(std::io::Error),
    Output(std::io::Error),
}

fn run_command(
    program: PathBuf,
    args: Vec<String>,
    cwd: PathBuf,
    prompt: String,
) -> Result<Output, CommandIoError> {
    let mut child = Command::new(&program)
        .args(&args)
        .current_dir(&cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(CommandIoError::Spawn)?;

    // Feed stdin from its own thread while wait_with_output drains stdout and
    // stderr; writing inline deadlocks once prompt and output exceed the pipe
    // buffers (~64KiB), which large diffs always do.
    let writer = child.stdin.take().map(|mut stdin| {
        std::thread::spawn(move || -> std::io::Result<()> { stdin.write_all(prompt.as_bytes()) })
    });

    let output = child.wait_with_output().map_err(CommandIoError::Output)?;

    if let Some(writer) = writer {
        // A child that exits before reading all of stdin breaks the pipe; its
        // own output/exit status carries the real story in that case.
        if let Ok(Err(error)) = writer.join()
            && error.kind() != std::io::ErrorKind::BrokenPipe
        {
            return Err(CommandIoError::Prompt(error));
        }
    }

    Ok(output)
}

#[async_trait]
impl AiEngine for CommandEngine {
    async fn generate_commit_message(&self, messages: &[ChatMessage]) -> Result<String> {
        let prompt = Self::render_prompt(messages);
        let program = self
            .resolved_program()
            .unwrap_or_else(|| PathBuf::from(&self.program));
        let args = self.args.clone();
        let cwd = self.cwd.clone();

        let result = tokio::task::spawn_blocking(move || run_command(program, args, cwd, prompt))
            .await
            .context("provider subprocess task failed")?;

        let output = result.map_err(|error| match error {
            CommandIoError::Spawn(error) => match error.kind() {
                std::io::ErrorKind::NotFound => anyhow::anyhow!(
                    "{} provider requires `{}` on PATH",
                    self.provider_label(),
                    self.program
                ),
                _ => anyhow::anyhow!(
                    "failed to start {} provider via `{}`: {error}",
                    self.provider_label(),
                    self.binary_hint()
                ),
            },
            CommandIoError::Prompt(error) => anyhow::anyhow!(
                "failed to write prompt to `{}`: {error}",
                self.binary_hint()
            ),
            CommandIoError::Output(error) => anyhow::anyhow!(
                "failed to read output from {} provider via `{}`: {error}",
                self.provider_label(),
                self.binary_hint()
            ),
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            let detail = if stderr.is_empty() {
                format!("exit status {}", output.status)
            } else {
                stderr
            };
            bail!(
                "{} provider failed via `{}`: {}",
                self.provider_label(),
                self.binary_hint(),
                detail
            );
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let content = sanitize_model_output(&stdout);
        if content.is_empty() {
            bail!(
                "{} provider returned an empty response via `{}`",
                self.provider_label(),
                self.binary_hint()
            );
        }

        Ok(content)
    }
}

#[cfg(test)]
mod tests {
    use std::{io::Write, path::Path};

    use tempfile::TempDir;

    use super::*;

    struct TestCommand {
        program: String,
        args: Vec<String>,
    }

    fn test_messages() -> Vec<ChatMessage> {
        vec![ChatMessage::user("diff --git a/src/lib.rs b/src/lib.rs")]
    }

    #[cfg(windows)]
    fn escape_cmd_echo(line: &str) -> String {
        line.replace('^', "^^")
            .replace('%', "%%")
            .replace('&', "^&")
            .replace('|', "^|")
            .replace('<', "^<")
            .replace('>', "^>")
            .replace('(', "^(")
            .replace(')', "^)")
    }

    fn install_test_command(
        dir: &Path,
        name: &str,
        stdout: &str,
        stderr: &str,
        exit_code: i32,
    ) -> TestCommand {
        #[cfg(unix)]
        let path = dir.join(name);
        #[cfg(windows)]
        let path = dir.join(format!("{name}.cmd"));

        #[cfg(unix)]
        let script = {
            let mut script = String::from("#!/bin/sh\ncat >/dev/null\n");
            if !stdout.is_empty() {
                script.push_str("cat <<'AIC_STDOUT'\n");
                script.push_str(stdout);
                if !stdout.ends_with('\n') {
                    script.push('\n');
                }
                script.push_str("AIC_STDOUT\n");
            }
            if !stderr.is_empty() {
                script.push_str("cat <<'AIC_STDERR' >&2\n");
                script.push_str(stderr);
                if !stderr.ends_with('\n') {
                    script.push('\n');
                }
                script.push_str("AIC_STDERR\n");
            }
            script.push_str(&format!("exit {exit_code}\n"));
            script
        };

        #[cfg(windows)]
        let script = {
            let mut script = String::from("@echo off\r\nmore >NUL\r\n");
            for line in stdout.lines() {
                script.push_str("echo(");
                script.push_str(&escape_cmd_echo(line));
                script.push_str("\r\n");
            }
            if stdout.ends_with('\n') {
                script.push_str("echo(\r\n");
            }
            for line in stderr.lines() {
                script.push_str(">&2 echo(");
                script.push_str(&escape_cmd_echo(line));
                script.push_str("\r\n");
            }
            if stderr.ends_with('\n') {
                script.push_str(">&2 echo(\r\n");
            }
            script.push_str(&format!("exit /b {exit_code}\r\n"));
            script
        };

        let temp_path = dir.join(format!(".{name}.tmp"));
        let mut file = std::fs::File::create(&temp_path).unwrap();
        file.write_all(script.as_bytes()).unwrap();
        file.sync_all().unwrap();
        drop(file);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = std::fs::metadata(&temp_path).unwrap().permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&temp_path, permissions).unwrap();
        }

        std::fs::rename(&temp_path, &path).unwrap();

        #[cfg(unix)]
        {
            TestCommand {
                program: "/bin/sh".to_owned(),
                args: vec![path.to_string_lossy().to_string()],
            }
        }

        #[cfg(windows)]
        {
            TestCommand {
                program: path.to_string_lossy().to_string(),
                args: Vec::new(),
            }
        }
    }

    #[tokio::test]
    async fn command_engine_strips_reasoning_tags() {
        let temp = TempDir::new().unwrap();
        let command = install_test_command(
            temp.path(),
            "claude-test",
            "<think>hidden</think>\nfeat: add cli\n",
            "",
            0,
        );
        let engine = CommandEngine::with_command(
            Config {
                ai_provider: "claude-code".to_owned(),
                model: "default".to_owned(),
                ..Config::default()
            },
            command.program,
            command.args,
            std::env::temp_dir(),
        );

        let response = engine
            .generate_commit_message(&test_messages())
            .await
            .unwrap();

        assert_eq!(response, "feat: add cli");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn command_engine_survives_large_prompt_and_chatty_child() {
        use std::os::unix::fs::PermissionsExt;

        // Regression test for the stdin/stdout pipe deadlock: the child floods
        // stderr well past the pipe buffer before reading any of stdin, and the
        // prompt itself is far larger than the stdin pipe buffer.
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("chatty");
        let script = "#!/bin/sh\n\
            i=0\n\
            while [ $i -lt 3000 ]; do\n\
            \techo \"stderr noise long enough to overflow the pipe buffer before stdin is read\" >&2\n\
            \ti=$((i+1))\n\
            done\n\
            cat >/dev/null\n\
            echo \"feat: survive large prompts\"\n";
        std::fs::write(&path, script).unwrap();
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&path, permissions).unwrap();

        let engine = CommandEngine::with_command(
            Config {
                ai_provider: "claude-code".to_owned(),
                model: "default".to_owned(),
                ..Config::default()
            },
            "/bin/sh".to_owned(),
            vec![path.to_string_lossy().to_string()],
            std::env::temp_dir(),
        );

        let large_diff = "+ a reasonably long changed line of diff content\n".repeat(10_000);
        let messages = vec![ChatMessage::user(large_diff)];

        let response = engine.generate_commit_message(&messages).await.unwrap();
        assert_eq!(response, "feat: survive large prompts");
    }

    #[tokio::test]
    async fn command_engine_reports_missing_binary() {
        let engine = CommandEngine::with_command(
            Config {
                ai_provider: "codex".to_owned(),
                model: "default".to_owned(),
                ..Config::default()
            },
            "__missing_binary__",
            ["exec"],
            std::env::temp_dir(),
        );

        let error = engine
            .generate_commit_message(&test_messages())
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("codex provider requires `__missing_binary__` on PATH"));
    }

    #[tokio::test]
    async fn command_engine_reports_non_zero_exit() {
        let temp = TempDir::new().unwrap();
        let command = install_test_command(temp.path(), "claude-fail", "", "boom", 9);
        let engine = CommandEngine::with_command(
            Config {
                ai_provider: "claude-code".to_owned(),
                model: "default".to_owned(),
                ..Config::default()
            },
            command.program,
            command.args,
            std::env::temp_dir(),
        );

        let error = engine
            .generate_commit_message(&test_messages())
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("claude-code provider failed"));
        assert!(error.contains("boom"));
    }

    #[tokio::test]
    async fn copilot_command_engine_reports_missing_binary() {
        let engine = CommandEngine::with_command(
            Config {
                ai_provider: "copilot".to_owned(),
                model: "default".to_owned(),
                ..Config::default()
            },
            "__missing_binary__",
            ["-s", "--no-ask-user"],
            std::env::temp_dir(),
        );

        let error = engine
            .generate_commit_message(&test_messages())
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("copilot provider requires `__missing_binary__` on PATH"));
    }

    #[tokio::test]
    async fn copilot_command_engine_reports_non_zero_exit() {
        let temp = TempDir::new().unwrap();
        let command = install_test_command(temp.path(), "copilot-fail", "", "boom", 9);
        let engine = CommandEngine::with_command(
            Config {
                ai_provider: "copilot".to_owned(),
                model: "default".to_owned(),
                ..Config::default()
            },
            command.program,
            command.args,
            std::env::temp_dir(),
        );

        let error = engine
            .generate_commit_message(&test_messages())
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("copilot provider failed"));
        assert!(error.contains("boom"));
    }
}
