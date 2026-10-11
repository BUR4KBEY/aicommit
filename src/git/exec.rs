use std::{
    ffi::OsStr,
    path::Path,
    process::{Command, Output},
};

use anyhow::{Result, bail};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitOutput {
    pub stdout: String,
    pub stderr: String,
}

pub fn run_git<I, S>(args: I) -> Result<GitOutput>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    run_git_in(std::env::current_dir()?, args)
}

pub fn run_git_in<I, S>(cwd: impl AsRef<Path>, args: I) -> Result<GitOutput>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = Command::new("git").args(args).current_dir(cwd).output()?;
    command_output(output)
}

pub(crate) fn run_git_dynamic_in(cwd: impl AsRef<Path>, args: Vec<String>) -> Result<GitOutput> {
    let output = Command::new("git").args(args).current_dir(cwd).output()?;
    command_output(output)
}

fn command_output(output: Output) -> Result<GitOutput> {
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if !output.status.success() {
        let detail = if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            format!("git exited with {}", output.status)
        };
        bail!("{detail}");
    }
    Ok(GitOutput { stdout, stderr })
}
