use anyhow::{Result, bail};

use crate::{
    config::Config,
    errors::AicError,
    generator, git,
    history_store::{self, HistoryEntry},
    ui,
};

pub async fn run(
    context: String,
    base: Option<String>,
    skip_confirmation: bool,
    provider_override: Option<String>,
) -> Result<()> {
    git::assert_git_repo()?;
    let config = Config::load_with_provider_override(provider_override.as_deref())?;

    if config.provider_needs_api_key() && config.api_key.is_none() {
        bail!(AicError::MissingApiKey(config.ai_provider));
    }

    let base_ref = git::resolve_base_ref(base.as_deref())?;
    let commits = git::commits_since(&base_ref)?;
    if commits.is_empty() {
        bail!("no commits found between {base_ref} and HEAD");
    }

    let diff = git::diff_since(&base_ref)?;
    if diff.trim().is_empty() {
        bail!("no diff found between {base_ref} and HEAD");
    }

    let changed_files = git::files_since(&base_ref)?;
    let branch_name = git::current_branch();
    let ticket = git::ticket_from_branch();

    ui::section("Pull request session");
    ui::session_step(format!(
        "Reading branch range against {base_ref} ({} commits, {}, {} lines)",
        commits.len(),
        ui::file_count_label(changed_files.len()),
        diff.lines().count()
    ));
    let mut session_items = vec![format!("base: {base_ref}")];
    if let Some(branch_name) = &branch_name {
        session_items.push(format!("branch: {branch_name}"));
    }
    if let Some(ticket) = &ticket {
        session_items.push(format!("ticket: {ticket}"));
    }
    if !context.trim().is_empty() {
        session_items.push("extra context provided".to_owned());
    }
    ui::metadata_row(&session_items);
    ui::metadata_row(&[
        format!("provider: {}", config.ai_provider),
        format!("model: {}", config.model),
    ]);
    ui::file_list("Changed files", &changed_files);

    loop {
        ui::session_step(format!(
            "Sending to {}/{}",
            config.ai_provider, config.model
        ));
        let spinner =
            ui::StatusSpinner::start("Generating pull request draft", ui::StatusPool::Waiting);
        let progress = |event: generator::GenerationProgress| {
            spinner.on_generation_progress(event, "pull request draft")
        };
        let draft = generator::generate_pull_request(
            &config,
            &diff,
            &context,
            &base_ref,
            branch_name.as_deref(),
            ticket.as_deref(),
            &commits,
            &changed_files,
            Some(&progress),
        )
        .await;
        spinner.finish_and_clear();

        let draft = draft?;
        if skip_confirmation {
            return finish(&config, &draft.title, &draft.body, &changed_files);
        }

        ui::blank_line();
        preview(&draft.title, &draft.body);

        let action = ui::select(
            "Use this pull request draft?",
            vec!["Use".to_owned(), "Regenerate".to_owned(), "Edit".to_owned()],
        )?;

        match action.as_str() {
            "Use" => return finish(&config, &draft.title, &draft.body, &changed_files),
            "Edit" => {
                let edited_title = ui::text("Edit PR title", Some(&draft.title))?;
                let edited_body = ui::editor("Edit PR description", &draft.body)?;
                return finish(
                    &config,
                    edited_title.trim(),
                    edited_body.trim(),
                    &changed_files,
                );
            }
            "Regenerate" => continue,
            _ => bail!("PR generation aborted"),
        }
    }
}

fn preview(title: &str, body: &str) {
    ui::blank_line();
    ui::primary_card("PR title", title);
    ui::blank_line();
    ui::markdown_card(
        "PR description",
        if body.is_empty() { "(empty)" } else { body },
    );
}

fn finish(config: &Config, title: &str, body: &str, changed_files: &[String]) -> Result<()> {
    if title.trim().is_empty() {
        bail!("PR title cannot be empty");
    }

    let message = pr_message(title, body);

    preview(title.trim(), body.trim());

    if let Err(error) = history_store::append_entry(&HistoryEntry {
        timestamp: history_store::now_iso8601(),
        kind: "pr".to_owned(),
        message,
        repo_path: git::repo_root()
            .map(|path| path.display().to_string())
            .unwrap_or_default(),
        files: changed_files.to_vec(),
        provider: config.ai_provider.clone(),
        model: config.model.clone(),
    }) {
        ui::warn(format!("failed to save history: {error}"));
    }

    ui::blank_line();
    ui::success("Generated pull request draft");
    Ok(())
}

fn pr_message(title: &str, body: &str) -> String {
    if body.trim().is_empty() {
        title.trim().to_owned()
    } else {
        format!("{}\n\n{}", title.trim(), body.trim())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pr_message_omits_blank_body() {
        assert_eq!(pr_message("feat: add pr", ""), "feat: add pr");
    }

    #[test]
    fn pr_message_includes_body() {
        assert_eq!(
            pr_message("feat: add pr", "## Summary\n- add it"),
            "feat: add pr\n\n## Summary\n- add it"
        );
    }
}
