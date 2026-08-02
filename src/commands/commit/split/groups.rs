use anyhow::{bail, Result};

use crate::{prompt::SplitPlanGroup, ui};

const ABORT_OPTION: &str = "Abort";
const USE_SUGGESTED_GROUPS_OPTION: &str = "Use suggested groups";
const BUILD_GROUPS_MANUALLY_OPTION: &str = "Build groups manually";
const KEEP_ONE_COMMIT_OPTION: &str = "Keep one commit";

pub(super) fn choose_split_groups(
    suggested_groups: &[SplitPlanGroup],
    staged_files: &[String],
) -> Result<Option<Vec<SplitPlanGroup>>> {
    render_split_groups(suggested_groups, "Suggested split groups");
    let selection = ui::select(
        "How would you like to use these groups?",
        vec![
            USE_SUGGESTED_GROUPS_OPTION.to_owned(),
            BUILD_GROUPS_MANUALLY_OPTION.to_owned(),
            KEEP_ONE_COMMIT_OPTION.to_owned(),
            ABORT_OPTION.to_owned(),
        ],
    )?;

    match selection.as_str() {
        USE_SUGGESTED_GROUPS_OPTION => Ok(Some(suggested_groups.to_vec())),
        BUILD_GROUPS_MANUALLY_OPTION => {
            let manual = build_manual_split_groups(staged_files)?;
            render_split_groups(&manual, "Manual split groups");
            Ok(Some(manual))
        }
        KEEP_ONE_COMMIT_OPTION => Ok(None),
        ABORT_OPTION => bail!("commit aborted"),
        _ => bail!("invalid split grouping selection"),
    }
}

fn build_manual_split_groups(staged_files: &[String]) -> Result<Vec<SplitPlanGroup>> {
    let mut remaining = staged_files.to_vec();
    let mut groups = Vec::new();
    let mut index = 1;

    while remaining.len() > 1 {
        let selection = ui::multiselect(
            &format!("Select files for split commit {index}"),
            remaining.clone(),
        )?;
        if selection.is_empty() {
            bail!("no files selected");
        }
        if remaining.len() > 2 && selection.len() == remaining.len() {
            bail!("select fewer than all remaining files to create multiple commits");
        }

        remaining.retain(|file| !selection.contains(file));
        groups.push(SplitPlanGroup {
            title: format!("Commit {index}"),
            rationale: "Manually grouped files".to_owned(),
            files: selection,
        });
        index += 1;
    }

    if !remaining.is_empty() {
        groups.push(SplitPlanGroup {
            title: format!("Commit {index}"),
            rationale: "Remaining files".to_owned(),
            files: remaining,
        });
    }

    Ok(groups)
}

pub(super) fn render_split_groups(groups: &[SplitPlanGroup], title: &str) {
    ui::blank_line();
    ui::section(title);
    for (index, group) in groups.iter().enumerate() {
        ui::blank_line();
        ui::headline(format!("Split commit {}: {}", index + 1, group.title));
        ui::secondary(&group.rationale);
        ui::file_metadata(&group.files);
        for line in ui::summarize_files(&group.files, 4, 3) {
            ui::bullet(line);
        }
    }
}
