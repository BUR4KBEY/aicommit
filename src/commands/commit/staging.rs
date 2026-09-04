use std::io::{IsTerminal, stdin, stdout};

use anyhow::{Result, bail};

use crate::{errors::AicError, git, ui};

const CONTINUE_STAGE_OPTION: &str = "Continue";
const UNSTAGE_FILES_OPTION: &str = "Unstage files";
const STAGE_ALL_FILES_OPTION: &str = "Stage all";
const CHOOSE_FILES_OPTION: &str = "Choose files";
const ABORT_STAGE_OPTION: &str = "Abort";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StagingState {
    UseExisting,
    AutoStageAll,
    PromptForSelection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StageSelectionAction {
    StageAll,
    ChooseFiles,
    Abort,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExistingStageAction {
    Continue,
    UnstageFiles,
    Abort,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StagingPlan {
    AddFiles(Vec<String>),
    RemoveFiles(Vec<String>),
    NoOp,
    Continue,
    Abort,
}

/// What the staging preflight already rendered, so callers can avoid
/// repeating it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct StagingReport {
    /// True when the final staged set was just listed on screen.
    pub(crate) listed_files: bool,
}

pub async fn ensure_staged_files(
    skip_confirmation: bool,
    allow_unstage_preflight: bool,
) -> Result<StagingReport> {
    loop {
        let staged = git::staged_files()?;
        let changed = git::changed_files()?;

        match resolve_staging_state(&staged, &changed, skip_confirmation)? {
            StagingState::UseExisting => 'existing_preflight: loop {
                if !should_prompt_for_existing_stage_selection(
                    skip_confirmation,
                    &staged,
                    allow_unstage_preflight,
                ) {
                    return Ok(StagingReport::default());
                }

                let action = prompt_for_existing_stage_selection(&staged)?;
                let selected_files = if action == ExistingStageAction::UnstageFiles {
                    ui::multiselect("Select files to unstage", staged.clone())?
                } else {
                    Vec::new()
                };

                match build_existing_staging_plan(action, selected_files) {
                    StagingPlan::RemoveFiles(files) => {
                        git::unstage_files(&files)?;
                        break 'existing_preflight;
                    }
                    StagingPlan::NoOp => {
                        ui::session_step("No files were selected; keeping the current staged set");
                        continue 'existing_preflight;
                    }
                    StagingPlan::Continue => return Ok(StagingReport { listed_files: true }),
                    StagingPlan::Abort => bail!("commit aborted"),
                    StagingPlan::AddFiles(_) => bail!("invalid existing staging plan"),
                }
            },
            StagingState::AutoStageAll => {
                let files = build_staging_plan(StageSelectionAction::StageAll, changed, vec![])?;
                apply_staging_plan(files)?;
                return Ok(StagingReport::default());
            }
            StagingState::PromptForSelection => {
                let action = prompt_for_stage_selection(&changed)?;
                let selected_files = if action == StageSelectionAction::ChooseFiles {
                    ui::multiselect("Select files to stage", changed.clone())?
                } else {
                    Vec::new()
                };

                apply_staging_plan(build_staging_plan(action, changed, selected_files)?)?;
                return Ok(StagingReport::default());
            }
        }
    }
}

fn apply_staging_plan(plan: StagingPlan) -> Result<()> {
    match plan {
        StagingPlan::AddFiles(files) => {
            git::add_files(&files)?;
            Ok(())
        }
        StagingPlan::RemoveFiles(files) => {
            git::unstage_files(&files)?;
            Ok(())
        }
        StagingPlan::NoOp => Ok(()),
        StagingPlan::Continue => Ok(()),
        StagingPlan::Abort => bail!("commit aborted"),
    }
}

fn resolve_staging_state(
    staged: &[String],
    changed: &[String],
    skip_confirmation: bool,
) -> Result<StagingState> {
    if changed.is_empty() && staged.is_empty() {
        bail!(AicError::NoChanges);
    }

    if !staged.is_empty() {
        Ok(StagingState::UseExisting)
    } else if skip_confirmation {
        Ok(StagingState::AutoStageAll)
    } else {
        Ok(StagingState::PromptForSelection)
    }
}

fn should_prompt_for_existing_stage_selection(
    skip_confirmation: bool,
    staged_files: &[String],
    allow_unstage_preflight: bool,
) -> bool {
    should_prompt_for_existing_stage_selection_with_terminals(
        skip_confirmation,
        !staged_files.is_empty(),
        allow_unstage_preflight,
        stdin().is_terminal(),
        stdout().is_terminal(),
    )
}

fn should_prompt_for_existing_stage_selection_with_terminals(
    skip_confirmation: bool,
    has_staged_files: bool,
    allow_unstage_preflight: bool,
    stdin_is_terminal: bool,
    stdout_is_terminal: bool,
) -> bool {
    !skip_confirmation
        && has_staged_files
        && allow_unstage_preflight
        && stdin_is_terminal
        && stdout_is_terminal
}

fn prompt_for_stage_selection(changed_files: &[String]) -> Result<StageSelectionAction> {
    ui::session_step("No files are staged yet");
    ui::file_list_step("Changed files", changed_files);

    let selection = ui::select(
        "No files are staged. What would you like to do?",
        stage_selection_options(),
    )?;

    match selection.as_str() {
        STAGE_ALL_FILES_OPTION => Ok(StageSelectionAction::StageAll),
        CHOOSE_FILES_OPTION => Ok(StageSelectionAction::ChooseFiles),
        ABORT_STAGE_OPTION => Ok(StageSelectionAction::Abort),
        _ => bail!("invalid staging selection"),
    }
}

fn prompt_for_existing_stage_selection(staged_files: &[String]) -> Result<ExistingStageAction> {
    ui::session_step("These files are already staged");
    ui::file_list_step("Staged changes", staged_files);

    let selection = ui::select(
        "What would you like to do with these staged files?",
        existing_stage_selection_options(),
    )?;

    match selection.as_str() {
        CONTINUE_STAGE_OPTION => Ok(ExistingStageAction::Continue),
        UNSTAGE_FILES_OPTION => Ok(ExistingStageAction::UnstageFiles),
        ABORT_STAGE_OPTION => Ok(ExistingStageAction::Abort),
        _ => bail!("invalid existing staging selection"),
    }
}

pub(crate) fn stage_selection_options() -> Vec<String> {
    vec![
        STAGE_ALL_FILES_OPTION.to_owned(),
        CHOOSE_FILES_OPTION.to_owned(),
        ABORT_STAGE_OPTION.to_owned(),
    ]
}

pub(crate) fn existing_stage_selection_options() -> Vec<String> {
    vec![
        CONTINUE_STAGE_OPTION.to_owned(),
        UNSTAGE_FILES_OPTION.to_owned(),
        ABORT_STAGE_OPTION.to_owned(),
    ]
}

fn build_staging_plan(
    action: StageSelectionAction,
    changed: Vec<String>,
    selected_files: Vec<String>,
) -> Result<StagingPlan> {
    match action {
        StageSelectionAction::StageAll => Ok(StagingPlan::AddFiles(changed)),
        StageSelectionAction::ChooseFiles => {
            if selected_files.is_empty() {
                bail!("no files selected");
            }
            Ok(StagingPlan::AddFiles(selected_files))
        }
        StageSelectionAction::Abort => Ok(StagingPlan::Abort),
    }
}

fn build_existing_staging_plan(
    action: ExistingStageAction,
    selected_files: Vec<String>,
) -> StagingPlan {
    match action {
        ExistingStageAction::Continue => StagingPlan::Continue,
        ExistingStageAction::UnstageFiles => {
            if selected_files.is_empty() {
                StagingPlan::NoOp
            } else {
                StagingPlan::RemoveFiles(selected_files)
            }
        }
        ExistingStageAction::Abort => StagingPlan::Abort,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staging_state_reports_no_changes_when_repo_is_clean() {
        let error = resolve_staging_state(&[], &[], false).unwrap_err();
        assert_eq!(error.to_string(), "no changes detected");
    }

    #[test]
    fn staging_state_bypasses_prompt_when_files_are_already_staged() {
        let staged = vec!["src/main.rs".to_owned()];
        let changed = vec!["src/main.rs".to_owned(), "README.md".to_owned()];

        assert_eq!(
            resolve_staging_state(&staged, &changed, false).unwrap(),
            StagingState::UseExisting
        );
    }

    #[test]
    fn staging_state_prompts_when_only_unstaged_files_exist() {
        let changed = vec!["src/main.rs".to_owned(), "README.md".to_owned()];

        assert_eq!(
            resolve_staging_state(&[], &changed, false).unwrap(),
            StagingState::PromptForSelection
        );
    }

    #[test]
    fn staging_state_auto_stages_all_with_yes_when_only_unstaged_files_exist() {
        let changed = vec!["src/main.rs".to_owned(), "README.md".to_owned()];

        assert_eq!(
            resolve_staging_state(&[], &changed, true).unwrap(),
            StagingState::AutoStageAll
        );
    }

    #[test]
    fn staging_plan_stages_all_changed_files() {
        let changed = vec!["src/main.rs".to_owned(), "README.md".to_owned()];

        assert_eq!(
            build_staging_plan(StageSelectionAction::StageAll, changed.clone(), vec![]).unwrap(),
            StagingPlan::AddFiles(changed)
        );
    }

    #[test]
    fn staging_plan_stages_only_selected_files() {
        let selected = vec!["README.md".to_owned()];

        assert_eq!(
            build_staging_plan(
                StageSelectionAction::ChooseFiles,
                vec!["src/main.rs".to_owned(), "README.md".to_owned()],
                selected.clone(),
            )
            .unwrap(),
            StagingPlan::AddFiles(selected)
        );
    }

    #[test]
    fn staging_plan_rejects_empty_file_selection() {
        let error = build_staging_plan(
            StageSelectionAction::ChooseFiles,
            vec!["src/main.rs".to_owned()],
            vec![],
        )
        .unwrap_err();

        assert_eq!(error.to_string(), "no files selected");
    }

    #[test]
    fn staging_plan_aborts_when_user_cancels() {
        assert_eq!(
            build_staging_plan(
                StageSelectionAction::Abort,
                vec!["src/main.rs".to_owned()],
                vec!["src/main.rs".to_owned()],
            )
            .unwrap(),
            StagingPlan::Abort
        );
    }

    #[test]
    fn stage_selection_options_use_explicit_action_labels() {
        assert_eq!(
            stage_selection_options(),
            vec![
                "Stage all".to_owned(),
                "Choose files".to_owned(),
                "Abort".to_owned()
            ]
        );
    }

    #[test]
    fn existing_stage_selection_options_include_unstage_action() {
        assert_eq!(
            existing_stage_selection_options(),
            vec![
                "Continue".to_owned(),
                "Unstage files".to_owned(),
                "Abort".to_owned()
            ]
        );
    }

    #[test]
    fn existing_staging_plan_unstages_selected_files() {
        let selected = vec!["src/main.rs".to_owned()];
        assert_eq!(
            build_existing_staging_plan(ExistingStageAction::UnstageFiles, selected.clone()),
            StagingPlan::RemoveFiles(selected)
        );
    }

    #[test]
    fn existing_staging_plan_treats_empty_unstage_selection_as_no_op() {
        assert_eq!(
            build_existing_staging_plan(ExistingStageAction::UnstageFiles, vec![]),
            StagingPlan::NoOp
        );
    }

    #[test]
    fn existing_staging_plan_continues_when_requested() {
        assert_eq!(
            build_existing_staging_plan(ExistingStageAction::Continue, vec![]),
            StagingPlan::Continue
        );
    }

    #[test]
    fn existing_staging_prompt_is_skipped_for_yes_mode() {
        assert!(!should_prompt_for_existing_stage_selection_with_terminals(
            true, true, true, true, true
        ));
        assert!(should_prompt_for_existing_stage_selection_with_terminals(
            false, true, true, true, true
        ));
        assert!(!should_prompt_for_existing_stage_selection_with_terminals(
            false, false, true, true, true
        ));
        assert!(!should_prompt_for_existing_stage_selection_with_terminals(
            false, true, true, false, true
        ));
        assert!(!should_prompt_for_existing_stage_selection_with_terminals(
            false, true, true, true, false
        ));
        assert!(!should_prompt_for_existing_stage_selection_with_terminals(
            false, true, false, true, true
        ));
    }
}
