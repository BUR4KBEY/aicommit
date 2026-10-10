use anyhow::{Result, bail};

use crate::{cli::SplitMode, config::Config, generator, git, prompt::SplitPlanGroup, ui};

use super::{
    helpers::append_commit_history,
    push::{PushPlan, build_push_plan, execute_push_plan},
    split::{SplitCommitDraft, generate_split_commit_drafts},
};

/// Result of attempting the non-interactive split path. `SingleCommit`
/// means the caller must fall through to the normal one-message flow.
pub(crate) enum AutoSplitOutcome {
    /// Split was not requested or a guardrail tripped (run single-commit).
    SingleCommit,
    /// Dry run: plan rendered, nothing committed.
    DryRun(Vec<SplitPlanGroup>),
    /// Commits created in plan order with their short hashes.
    Committed(Vec<AutoSplitCommit>),
}

pub(crate) struct AutoSplitCommit {
    pub(crate) draft: SplitCommitDraft,
    pub(crate) hash: String,
}

/// Non-interactive split: plan, drafts, then either render (`dry_run`) or
/// commit each group in order. Never prompts. Any guardrail trip returns
/// `SingleCommit` so the caller falls back to the one-message flow.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_auto_split(
    config: &Config,
    diff: &str,
    extra_args: &[String],
    context: &str,
    full_gitmoji_spec: bool,
    split: SplitMode,
    skip_confirmation: bool,
    dry_run: bool,
    amend: bool,
    staged_files: &[String],
) -> Result<AutoSplitOutcome> {
    if split != SplitMode::Auto || amend {
        return Ok(AutoSplitOutcome::SingleCommit);
    }
    if !skip_confirmation && !dry_run {
        bail!(
            "--split auto requires -y (or -d for dry-run) because group selection is interactive"
        );
    }
    if staged_files.len() < 2 {
        return Ok(AutoSplitOutcome::SingleCommit);
    }

    let partially_staged = git::partially_staged_files(staged_files)?;
    if !partially_staged.is_empty() {
        ui::warn(format!(
            "split flow is unavailable because these files also have unstaged changes: {}",
            partially_staged.join(", ")
        ));
        return Ok(AutoSplitOutcome::SingleCommit);
    }

    let spinner = ui::StatusSpinner::start(
        "Analyzing staged changes for split groups",
        ui::StatusPool::Waiting,
    );
    let progress =
        |event: generator::GenerationProgress| spinner.on_generation_progress(event, "split plan");
    let suggested =
        generator::generate_split_plan(config, diff, context, staged_files, Some(&progress)).await;
    spinner.finish_and_clear();

    let suggested = match suggested {
        Ok(groups) => groups,
        Err(error) => {
            ui::warn(format!(
                "could not build a split plan; continuing with one commit: {error}"
            ));
            return Ok(AutoSplitOutcome::SingleCommit);
        }
    };

    let Some(groups) = accept_auto_groups(config, &suggested, staged_files) else {
        return Ok(AutoSplitOutcome::SingleCommit);
    };

    // No partial commits: a group without a message fails the whole split, so
    // agents get one stderr line naming the group, the stage, and the attempts.
    let drafts =
        generate_split_commit_drafts(config, &groups, context, full_gitmoji_spec, extra_args)
            .await
            .map_err(anyhow::Error::new)?;

    if dry_run {
        render_auto_split_plan(&drafts);
        return Ok(AutoSplitOutcome::DryRun(groups));
    }

    let push_plan = build_push_plan(
        config.gitpush,
        true,
        &git::remote_metadata()?,
        &config.remote_icon_style,
    )?;
    if !matches!(push_plan, PushPlan::Skip | PushPlan::AutoPush(_)) {
        // build_push_plan with skip=true only yields Skip/AutoPush (or bails
        // on multi-remote). Fail loudly if that ever changes instead of
        // prompting for a remote mid-split.
        bail!(
            "machine mode cannot choose a push remote; set AIC_GITPUSH=false or configure exactly one remote"
        );
    }

    let mut commits = Vec::with_capacity(drafts.len());
    for (index, draft) in drafts.iter().enumerate() {
        // The model plan assigns every staged file exactly once (validated
        // in generate_split_plan and re-checked in accept_auto_groups), so
        // clear/add per group never double- or never-commits a file. Drafts
        // already carry the message template, like the interactive flow.
        git::clear_index()?;
        git::add_files(&draft.group.files)?;
        git::commit(
            &draft.message,
            &super::filtered_extra_args(config, extra_args),
        )
        .map_err(|error| {
            if index == 0 {
                error
            } else {
                anyhow::anyhow!(
                    "split commit {} failed after {} earlier split commits were created: {error}",
                    index + 1,
                    index
                )
            }
        })?;
        let hash = git::head_short_hash()?;
        ui::section(format!(
            "Split commit {}/{} created",
            index + 1,
            drafts.len()
        ));
        ui::headline(draft.message.lines().next().unwrap_or(&draft.message));
        append_commit_history(config, &draft.message, &draft.group.files);
        commits.push(AutoSplitCommit {
            draft: draft.clone(),
            hash,
        });
    }

    execute_push_plan(push_plan, config, true).await?;
    Ok(AutoSplitOutcome::Committed(commits))
}

/// Guardrailed acceptance: exactly-once file coverage, at least 2 groups,
/// at most `config.split_max` groups. Rejections warn on stderr and return
/// `None` (caller falls back to single-commit).
fn accept_auto_groups(
    config: &Config,
    groups: &[SplitPlanGroup],
    staged_files: &[String],
) -> Option<Vec<SplitPlanGroup>> {
    if groups.len() < 2 {
        ui::warn("split plan proposed fewer than 2 groups; continuing with one commit");
        return None;
    }
    if groups.len() > config.split_max {
        ui::warn(format!(
            "split plan proposed {} groups (max {}); continuing with one commit",
            groups.len(),
            config.split_max
        ));
        return None;
    }
    // Defensive: generate_split_plan already validates exact-once coverage,
    // but never commit a partial/overlapping plan without prompting.
    let mut seen: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for group in groups {
        for file in &group.files {
            if !seen.insert(file.as_str()) {
                ui::warn(format!(
                    "split plan referenced '{file}' more than once; continuing with one commit"
                ));
                return None;
            }
        }
    }
    let staged: std::collections::BTreeSet<&str> =
        staged_files.iter().map(String::as_str).collect();
    if seen != staged {
        ui::warn(
            "split plan did not assign every staged file exactly once; continuing with one commit",
        );
        return None;
    }
    Some(groups.to_vec())
}

fn render_auto_split_plan(drafts: &[SplitCommitDraft]) {
    ui::blank_line();
    ui::section(format!("Split plan ({})", drafts.len()));
    for (index, draft) in drafts.iter().enumerate() {
        ui::blank_line();
        ui::headline(format!("Commit {}: {}", index + 1, draft.group.title));
        ui::secondary(&draft.group.rationale);
        ui::file_metadata(&draft.group.files);
        for line in ui::summarize_files(&draft.group.files, 4, 3) {
            ui::bullet(line);
        }
        ui::primary_card("Commit message", &draft.message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(title: &str, files: &[&str]) -> SplitPlanGroup {
        SplitPlanGroup {
            title: title.to_owned(),
            rationale: "test".to_owned(),
            files: files.iter().map(ToString::to_string).collect(),
        }
    }

    #[test]
    fn rejects_single_group_plan() {
        let config = Config::default();
        let staged = vec!["a.txt".to_owned()];
        assert!(accept_auto_groups(&config, &[group("one", &["a.txt"])], &staged).is_none());
    }

    #[test]
    fn rejects_plan_exceeding_split_max() {
        let config = Config {
            split_max: 2,
            ..Config::default()
        };
        let staged = vec!["a.txt".to_owned(), "b.txt".to_owned(), "c.txt".to_owned()];
        let groups = vec![
            group("one", &["a.txt"]),
            group("two", &["b.txt"]),
            group("three", &["c.txt"]),
        ];
        assert!(accept_auto_groups(&config, &groups, &staged).is_none());
    }

    #[test]
    fn rejects_plan_with_double_committed_file() {
        let config = Config::default();
        let staged = vec!["a.txt".to_owned(), "b.txt".to_owned()];
        let groups = vec![group("one", &["a.txt", "b.txt"]), group("two", &["a.txt"])];
        assert!(accept_auto_groups(&config, &groups, &staged).is_none());
    }

    #[test]
    fn rejects_plan_missing_a_staged_file() {
        let config = Config::default();
        let staged = vec!["a.txt".to_owned(), "b.txt".to_owned()];
        let groups = vec![group("one", &["a.txt"]), group("two", &["c.txt"])];
        assert!(accept_auto_groups(&config, &groups, &staged).is_none());
    }

    #[test]
    fn accepts_exact_coverage_within_max() {
        let config = Config::default();
        let staged = vec!["a.txt".to_owned(), "b.txt".to_owned()];
        let groups = vec![group("one", &["a.txt"]), group("two", &["b.txt"])];
        assert_eq!(accept_auto_groups(&config, &groups, &staged), Some(groups));
    }
}
