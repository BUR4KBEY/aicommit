use anyhow::{Result, bail};

use crate::{config::Config, generator, git, prompt::SplitPlanGroup, ui};

use super::super::{
    apply_message_template, filtered_extra_args,
    helpers::append_commit_history,
    push::{build_push_plan, execute_push_plan},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SplitCommitDraft {
    pub(crate) group: SplitPlanGroup,
    pub(crate) message: String,
}

pub(crate) async fn generate_split_commit_drafts(
    config: &Config,
    groups: &[SplitPlanGroup],
    context: &str,
    full_gitmoji_spec: bool,
    extra_args: &[String],
) -> Result<Vec<SplitCommitDraft>> {
    let mut drafts = Vec::with_capacity(groups.len());

    for (index, group) in groups.iter().enumerate() {
        let group_diff = git::staged_diff(&group.files)?;
        let draft_label = format!("commit {}/{}", index + 1, groups.len());
        let spinner = ui::StatusSpinner::start(
            format!("Drafting {}: {}", draft_label, group.title),
            ui::StatusPool::Waiting,
        );
        let progress = |event: generator::GenerationProgress| {
            spinner.on_generation_progress(event, &draft_label)
        };
        let mut message = generator::generate_commit_message(
            config,
            &group_diff,
            full_gitmoji_spec,
            context,
            &group.files,
            Some(&progress),
        )
        .await?;
        spinner.finish_and_clear();
        message = apply_message_template(config, extra_args, &message);
        drafts.push(SplitCommitDraft {
            group: group.clone(),
            message,
        });
    }

    Ok(drafts)
}

pub(super) fn render_split_commit_preview(drafts: &[SplitCommitDraft]) {
    ui::blank_line();
    ui::section(format!("Split commit preview ({})", drafts.len()));
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

pub(super) fn edit_split_commit_message(drafts: &mut [SplitCommitDraft]) -> Result<()> {
    let labels = drafts
        .iter()
        .enumerate()
        .map(|(index, draft)| format!("Commit {}: {}", index + 1, draft.group.title))
        .collect::<Vec<_>>();
    let selected = ui::select("Which commit message would you like to edit?", labels)?;
    let Some(index) = selected
        .split(':')
        .next()
        .and_then(|prefix| prefix.trim_start_matches("Commit ").parse::<usize>().ok())
        .and_then(|n| n.checked_sub(1))
    else {
        bail!("invalid split commit selection");
    };
    let edited = ui::editor("Edit split commit message", &drafts[index].message)?;
    let edited = edited.trim();
    if edited.is_empty() {
        bail!("split commit message cannot be empty");
    }
    drafts[index].message = edited.to_owned();
    Ok(())
}

pub(crate) async fn create_split_commits(
    config: &Config,
    drafts: &[SplitCommitDraft],
    extra_args: &[String],
) -> Result<()> {
    let push_plan = build_push_plan(
        config.gitpush,
        false,
        &git::remote_metadata()?,
        &config.remote_icon_style,
    )?;

    for (index, draft) in drafts.iter().enumerate() {
        git::clear_index()?;
        git::add_files(&draft.group.files)?;
        git::commit(&draft.message, &filtered_extra_args(config, extra_args)).map_err(|error| {
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
        ui::section(format!(
            "Split commit {}/{} created",
            index + 1,
            drafts.len()
        ));
        ui::headline(draft.message.lines().next().unwrap_or(&draft.message));

        append_commit_history(config, &draft.message, &draft.group.files);
    }

    execute_push_plan(push_plan, config, false)
        .await
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use std::{
        ffi::OsStr,
        path::{Path, PathBuf},
        process::Command,
        sync::MutexGuard,
    };

    use tempfile::TempDir;

    use super::*;
    use crate::{config::Config, git::cwd_test_lock};

    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn split_drafts_use_group_specific_diffs() {
        let _cwd = hold_cwd_for_test();
        let repo = init_repo();
        std::fs::create_dir_all(repo.path().join("src")).unwrap();
        std::fs::write(repo.path().join("README.md"), "docs\n").unwrap();
        std::fs::write(repo.path().join("src/lib.rs"), "pub fn value() {}\n").unwrap();
        run_git_test(repo.path(), ["add", "README.md", "src/lib.rs"]);

        let _dir = CurrentDirGuard::enter(repo.path());

        let drafts = generate_split_commit_drafts(
            &Config {
                ai_provider: "test".to_owned(),
                gitpush: false,
                ..Config::default()
            },
            &[
                SplitPlanGroup {
                    title: "Docs".to_owned(),
                    rationale: "docs".to_owned(),
                    files: vec!["README.md".to_owned()],
                },
                SplitPlanGroup {
                    title: "Library".to_owned(),
                    rationale: "lib".to_owned(),
                    files: vec!["src/lib.rs".to_owned()],
                },
            ],
            "",
            false,
            &[],
        )
        .await
        .unwrap();

        assert_eq!(drafts[0].message, "docs: update readme");
        assert_eq!(drafts[1].message, "feat: update library");
    }

    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn create_split_commits_creates_multiple_commits() {
        let _cwd = hold_cwd_for_test();
        let repo = init_repo();
        std::fs::create_dir_all(repo.path().join("src")).unwrap();
        std::fs::write(repo.path().join("README.md"), "docs\n").unwrap();
        std::fs::write(repo.path().join("src/lib.rs"), "pub fn value() {}\n").unwrap();
        run_git_test(repo.path(), ["add", "README.md", "src/lib.rs"]);

        let _dir = CurrentDirGuard::enter(repo.path());

        create_split_commits(
            &Config {
                ai_provider: "test".to_owned(),
                gitpush: false,
                ..Config::default()
            },
            &[
                SplitCommitDraft {
                    group: SplitPlanGroup {
                        title: "Docs".to_owned(),
                        rationale: "docs".to_owned(),
                        files: vec!["README.md".to_owned()],
                    },
                    message: "docs: update readme".to_owned(),
                },
                SplitCommitDraft {
                    group: SplitPlanGroup {
                        title: "Library".to_owned(),
                        rationale: "lib".to_owned(),
                        files: vec!["src/lib.rs".to_owned()],
                    },
                    message: "feat: update library".to_owned(),
                },
            ],
            &[],
        )
        .await
        .unwrap();

        let subjects = git_stdout(repo.path(), ["log", "--format=%s", "-2"]);
        let lines = subjects.lines().collect::<Vec<_>>();
        assert_eq!(lines, vec!["feat: update library", "docs: update readme"]);
    }

    fn init_repo() -> TempDir {
        let temp = TempDir::new().unwrap();
        run_git_test(temp.path(), ["init", "-b", "main"]);
        run_git_test(temp.path(), ["config", "user.email", "test@example.com"]);
        run_git_test(temp.path(), ["config", "user.name", "Test User"]);
        temp
    }

    fn git_stdout<I, S>(cwd: &Path, args: I) -> String
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let output = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(output.status.success());
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    fn run_git_test<I, S>(cwd: &Path, args: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let status = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .status()
            .unwrap();
        assert!(status.success());
    }

    fn hold_cwd_for_test() -> MutexGuard<'static, ()> {
        cwd_test_lock()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    struct CurrentDirGuard {
        previous: PathBuf,
    }

    impl CurrentDirGuard {
        fn enter(path: &Path) -> Self {
            let previous = std::env::current_dir().unwrap();
            std::env::set_current_dir(path).unwrap();
            Self { previous }
        }
    }

    impl Drop for CurrentDirGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.previous);
        }
    }
}
