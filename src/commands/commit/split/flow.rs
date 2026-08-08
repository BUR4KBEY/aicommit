use anyhow::{Result, bail};

use crate::{config::Config, generator, git, ui};

use super::{
    drafts::{
        create_split_commits, edit_split_commit_message, generate_split_commit_drafts,
        render_split_commit_preview,
    },
    groups::choose_split_groups,
};

const CREATE_ONE_COMMIT_OPTION: &str = "Create one commit";
const SPLIT_INTO_MULTIPLE_COMMITS_OPTION: &str = "Split into multiple commits";
const ABORT_OPTION: &str = "Abort";
const CREATE_COMMITS_OPTION: &str = "Create commits";
const REGENERATE_ALL_MESSAGES_OPTION: &str = "Regenerate messages";
const EDIT_A_MESSAGE_OPTION: &str = "Edit a message";
const ACCEPT_OPTION: &str = "Accept";
const REGENERATE_OPTION: &str = "Regenerate";
const EDIT_OPTION: &str = "Edit";

pub(crate) fn should_offer_split(
    staged_file_count: usize,
    skip_confirmation: bool,
    dry_run: bool,
    amend: bool,
) -> bool {
    staged_file_count >= 2 && !skip_confirmation && !dry_run && !amend
}

pub(crate) async fn maybe_execute_split_flow(
    config: &Config,
    diff: &str,
    extra_args: &[String],
    context: &str,
    full_gitmoji_spec: bool,
    staged_files: &[String],
) -> Result<bool> {
    if staged_files.len() < 2 {
        return Ok(false);
    }

    let partially_staged = git::partially_staged_files(staged_files)?;
    if !partially_staged.is_empty() {
        ui::warn(format!(
            "split flow is unavailable because these files also have unstaged changes: {}",
            partially_staged.join(", ")
        ));
        return Ok(false);
    }

    let selection = ui::select(
        "How would you like to commit these staged changes?",
        vec![
            CREATE_ONE_COMMIT_OPTION.to_owned(),
            SPLIT_INTO_MULTIPLE_COMMITS_OPTION.to_owned(),
            ABORT_OPTION.to_owned(),
        ],
    )?;

    match selection.as_str() {
        CREATE_ONE_COMMIT_OPTION => return Ok(false),
        ABORT_OPTION => bail!("commit aborted"),
        SPLIT_INTO_MULTIPLE_COMMITS_OPTION => {}
        _ => bail!("invalid split selection"),
    }

    let spinner = ui::StatusSpinner::start(
        "Analyzing staged changes for split groups",
        ui::StatusPool::Waiting,
    );
    let progress =
        |event: generator::GenerationProgress| spinner.on_generation_progress(event, "split plan");
    let suggested_groups =
        generator::generate_split_plan(config, diff, context, staged_files, Some(&progress)).await;
    spinner.finish_and_clear();

    let suggested_groups = match suggested_groups {
        Ok(groups) => groups,
        Err(error) => {
            ui::warn(format!(
                "could not build a split plan; continuing with one commit: {error}"
            ));
            return Ok(false);
        }
    };

    let groups = choose_split_groups(&suggested_groups, staged_files)?;
    let Some(groups) = groups else {
        return Ok(false);
    };

    let mut drafts =
        generate_split_commit_drafts(config, &groups, context, full_gitmoji_spec, extra_args)
            .await?;

    loop {
        render_split_commit_preview(&drafts);
        let selection = ui::select(
            "What would you like to do with these split commits?",
            vec![
                CREATE_COMMITS_OPTION.to_owned(),
                REGENERATE_ALL_MESSAGES_OPTION.to_owned(),
                EDIT_A_MESSAGE_OPTION.to_owned(),
                ABORT_OPTION.to_owned(),
            ],
        )?;

        match selection.as_str() {
            CREATE_COMMITS_OPTION => {
                create_split_commits(config, &drafts, extra_args).await?;
                return Ok(true);
            }
            REGENERATE_ALL_MESSAGES_OPTION => {
                drafts = generate_split_commit_drafts(
                    config,
                    &groups,
                    context,
                    full_gitmoji_spec,
                    extra_args,
                )
                .await?;
            }
            EDIT_A_MESSAGE_OPTION => edit_split_commit_message(&mut drafts)?,
            ABORT_OPTION => bail!("commit aborted"),
            _ => bail!("invalid split preview selection"),
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn generate_confirm_and_commit(
    config: &Config,
    diff: &str,
    extra_args: &[String],
    context: &str,
    full_gitmoji_spec: bool,
    skip_confirmation: bool,
    dry_run: bool,
    staged_files: &[String],
) -> Result<()> {
    loop {
        ui::session_step(format!(
            "Sending to {}/{}",
            config.ai_provider, config.model
        ));
        let spinner =
            ui::StatusSpinner::start("Generating commit message", ui::StatusPool::Waiting);
        let progress = |event: generator::GenerationProgress| {
            spinner.on_generation_progress(event, "commit message")
        };
        let commit_message = generator::generate_commit_message(
            config,
            diff,
            full_gitmoji_spec,
            context,
            staged_files,
            Some(&progress),
        )
        .await;
        spinner.finish_and_clear();

        let commit_message =
            super::super::apply_message_template(config, extra_args, &commit_message?);

        ui::blank_line();
        ui::primary_card("Generated commit", &commit_message);

        if dry_run {
            return Ok(());
        }

        let action = if skip_confirmation {
            ACCEPT_OPTION.to_owned()
        } else {
            ui::select(
                "What would you like to do with this message?",
                commit_confirmation_options(),
            )?
        };

        match action.as_str() {
            ACCEPT_OPTION => {
                return super::super::push::commit_and_maybe_push(
                    config,
                    &commit_message,
                    extra_args,
                    staged_files,
                    skip_confirmation,
                )
                .await;
            }
            EDIT_OPTION => {
                let edited = ui::text("Edit commit message", Some(&commit_message))?;
                return super::super::push::commit_and_maybe_push(
                    config,
                    &edited,
                    extra_args,
                    staged_files,
                    skip_confirmation,
                )
                .await;
            }
            REGENERATE_OPTION => continue,
            _ => bail!("commit aborted"),
        }
    }
}

pub(crate) fn commit_confirmation_options() -> Vec<String> {
    vec![
        ACCEPT_OPTION.to_owned(),
        REGENERATE_OPTION.to_owned(),
        EDIT_OPTION.to_owned(),
        ABORT_OPTION.to_owned(),
    ]
}

#[cfg(test)]
mod tests {
    use super::{commit_confirmation_options, should_offer_split};

    #[test]
    fn split_prompt_is_only_offered_for_interactive_multi_file_commits() {
        assert!(should_offer_split(2, false, false, false));
        assert!(!should_offer_split(1, false, false, false));
        assert!(!should_offer_split(2, true, false, false));
        assert!(!should_offer_split(2, false, true, false));
        assert!(!should_offer_split(2, false, false, true));
    }

    #[test]
    fn commit_confirmation_options_use_transcript_actions() {
        assert_eq!(
            commit_confirmation_options(),
            vec![
                "Accept".to_owned(),
                "Regenerate".to_owned(),
                "Edit".to_owned(),
                "Abort".to_owned()
            ]
        );
    }
}
