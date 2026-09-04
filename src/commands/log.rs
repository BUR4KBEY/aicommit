use anyhow::{Result, bail};

use crate::{config::Config, errors::AicError, generator, git, ui};

pub async fn run(
    count: usize,
    skip_confirmation: bool,
    provider_override: Option<String>,
) -> Result<()> {
    git::assert_git_repo()?;
    let config = Config::load_with_provider_override(provider_override.as_deref())?;

    if config.provider_needs_api_key() && config.api_key.is_none() {
        bail!(AicError::MissingApiKey(config.ai_provider));
    }

    git::assert_clean_worktree()?;
    git::assert_no_merges(count)?;

    let commits = git::last_n_commits(count)?;
    if commits.is_empty() {
        bail!("no commits found");
    }

    ui::section(format!("Rewriting {}", commit_count_label(commits.len())));

    let mut new_messages = Vec::with_capacity(commits.len());
    for commit in &commits {
        let diff = git::commit_diff(&commit.hash)?;
        let files = git::commit_files(&commit.hash)?;

        let spinner = ui::StatusSpinner::start(
            format!("Generating message for {}", &commit.hash[..8]),
            ui::StatusPool::Waiting,
        );
        let hash_label = format!("message for {}", &commit.hash[..8]);
        let progress = |event: generator::GenerationProgress| {
            spinner.on_generation_progress(event, &hash_label)
        };
        let message =
            generator::generate_commit_message(&config, &diff, false, "", &files, Some(&progress))
                .await?;
        spinner.finish_and_clear();
        new_messages.push(message);
    }

    ui::section("Proposed changes");
    for (commit, new_msg) in commits.iter().zip(new_messages.iter()) {
        ui::blank_line();
        let old_subject = &commit.subject;
        let new_subject = new_msg.lines().next().unwrap_or("");
        // `secondary` indents 2 spaces; pad the arrow line so the new subject
        // lines up under the old one (2 indent + 8 hash + 2 gap).
        ui::secondary(format!("{}  {old_subject}", &commit.hash[..8]));
        ui::info(format!("{:10}→ {new_subject}", ""));
    }

    if !skip_confirmation {
        ui::blank_line();
        if !ui::confirm("Rewrite these commit messages?", false)? {
            bail!("rewrite aborted");
        }
    }

    let spinner = ui::spinner("Rewriting commits");
    git::reword_commits(commits.len(), &new_messages)?;
    spinner.finish_and_clear();

    ui::success(format!("Rewrote {}", commit_count_label(commits.len())));
    Ok(())
}

fn commit_count_label(count: usize) -> String {
    match count {
        1 => "1 commit message".to_owned(),
        value => format!("{value} commit messages"),
    }
}
