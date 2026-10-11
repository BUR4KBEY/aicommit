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

/// Attempts per group message before the flow surfaces the failure. Small on
/// purpose: a group that keeps failing is a provider or config problem, not a
/// blip, and every retry costs a full plan-sized round trip.
pub(crate) const GROUP_MESSAGE_ATTEMPTS: u32 = 4;

/// One group could not be drafted. No commit exists yet, so the caller picks
/// between retrying that group, committing the drafts gathered before it, or
/// aborting the whole split.
#[derive(Debug)]
pub(crate) struct SplitDraftFailure {
    /// Drafts for the groups before the failure, in plan order.
    pub(crate) drafts: Vec<SplitCommitDraft>,
    /// Zero-based index of the group that failed.
    pub(crate) index: usize,
    pub(crate) total: usize,
    /// Which step of the group pipeline failed, e.g. "message generation".
    pub(crate) stage: &'static str,
    pub(crate) attempts: u32,
    pub(crate) source: anyhow::Error,
}

impl SplitDraftFailure {
    /// 1-based position of the failed group, for humans and stderr.
    pub(crate) fn position(&self) -> usize {
        self.index + 1
    }
}

impl std::fmt::Display for SplitDraftFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "group {} of {}: {} failed after {} attempts; nothing committed: {:#}",
            self.position(),
            self.total,
            self.stage,
            self.attempts,
            self.source
        )
    }
}

impl std::error::Error for SplitDraftFailure {}

pub(crate) async fn generate_split_commit_drafts(
    config: &Config,
    groups: &[SplitPlanGroup],
    context: &str,
    full_gitmoji_spec: bool,
    extra_args: &[String],
) -> Result<Vec<SplitCommitDraft>, SplitDraftFailure> {
    generate_split_commit_drafts_from(config, groups, 0, context, full_gitmoji_spec, extra_args)
        .await
}

/// Draft the messages for `groups[start..]`. A resumed run keeps the drafts
/// that already succeeded, so a retry never re-bills or re-renders them.
pub(crate) async fn generate_split_commit_drafts_from(
    config: &Config,
    groups: &[SplitPlanGroup],
    start: usize,
    context: &str,
    full_gitmoji_spec: bool,
    extra_args: &[String],
) -> Result<Vec<SplitCommitDraft>, SplitDraftFailure> {
    let mut drafts = Vec::with_capacity(groups.len().saturating_sub(start));

    for (index, group) in groups.iter().enumerate().skip(start) {
        let draft = match generate_group_draft(
            config,
            group,
            index,
            groups.len(),
            context,
            full_gitmoji_spec,
            extra_args,
        )
        .await
        {
            Ok(draft) => draft,
            Err(mut failure) => {
                // Hand the successful drafts back so the interactive flow can
                // offer "commit the remaining groups" without regenerating.
                failure.drafts = std::mem::take(&mut drafts);
                return Err(failure);
            }
        };
        drafts.push(draft);
    }

    Ok(drafts)
}

async fn generate_group_draft(
    config: &Config,
    group: &SplitPlanGroup,
    index: usize,
    total: usize,
    context: &str,
    full_gitmoji_spec: bool,
    extra_args: &[String],
) -> Result<SplitCommitDraft, SplitDraftFailure> {
    let failure = |stage: &'static str, attempts: u32, source: anyhow::Error| SplitDraftFailure {
        drafts: Vec::new(),
        index,
        total,
        stage,
        attempts,
        source,
    };

    let group_diff =
        git::staged_diff(&group.files).map_err(|error| failure("staged diff read", 0, error))?;
    let draft_label = format!("commit {}/{}", index + 1, total);

    let mut last_error = None;
    for _ in 0..GROUP_MESSAGE_ATTEMPTS {
        let spinner = ui::StatusSpinner::start(
            format!("Drafting {}: {}", draft_label, group.title),
            ui::StatusPool::Waiting,
        );
        let progress = |event: generator::GenerationProgress| {
            spinner.on_generation_progress(event, &draft_label)
        };
        let generated = generator::generate_commit_message(
            config,
            &group_diff,
            full_gitmoji_spec,
            context,
            &group.files,
            Some(&progress),
        )
        .await;
        spinner.finish_and_clear();

        match generated {
            Ok(message) => {
                return Ok(SplitCommitDraft {
                    group: group.clone(),
                    message: apply_message_template(config, extra_args, &message),
                });
            }
            Err(error) => last_error = Some(error),
        }
    }

    Err(failure(
        "message generation",
        GROUP_MESSAGE_ATTEMPTS,
        last_error.unwrap_or_else(|| anyhow::anyhow!("provider returned no message")),
    ))
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
    complete_plan: bool,
) -> Result<()> {
    let push_plan = build_push_plan(
        config.gitpush,
        false,
        &git::remote_metadata()?,
        &config.remote_icon_style,
    )?;

    commit_split_drafts(config, drafts, extra_args, complete_plan)?;

    execute_push_plan(push_plan, config, false)
        .await
        .map(|_| ())
}

/// Create one commit per draft, all or nothing, and return their short
/// hashes. Each group is staged from a snapshot of the original index, with
/// the old path of any staged rename so the deletion side travels with it.
/// When `complete_plan` is set, the last group also takes every staged path
/// the plan never named (ignore-filtered files), so nothing staged is
/// dropped; otherwise those paths stay staged afterwards. If any commit
/// fails, the earlier split commits are undone and the original index is
/// restored before the error surfaces.
pub(crate) fn commit_split_drafts(
    config: &Config,
    drafts: &[SplitCommitDraft],
    extra_args: &[String],
    complete_plan: bool,
) -> Result<Vec<String>> {
    let snapshot = git::snapshot_index()?;
    let renames = git::staged_renames()?;
    let commit_args = filtered_extra_args(config, extra_args);

    let mut group_paths: Vec<Vec<String>> = drafts
        .iter()
        .map(|draft| {
            let files = &draft.group.files;
            let mut paths = files.clone();
            paths.extend(
                renames
                    .iter()
                    .filter(|(new, _)| files.contains(new))
                    .map(|(_, old)| old.clone()),
            );
            paths
        })
        .collect();
    let unplanned: Vec<String> = snapshot
        .paths
        .iter()
        .filter(|path| !group_paths.iter().flatten().any(|planned| planned == *path))
        .cloned()
        .collect();
    if complete_plan && let Some(last) = group_paths.last_mut() {
        last.extend(unplanned.iter().cloned());
    }

    let mut hashes = Vec::with_capacity(drafts.len());
    for (index, (draft, paths)) in drafts.iter().zip(&group_paths).enumerate() {
        let result = git::stage_from_snapshot(&snapshot, paths)
            .and_then(|()| git::commit(&draft.message, &commit_args))
            .and_then(|_| git::head_short_hash());
        match result {
            Ok(hash) => hashes.push(hash),
            Err(error) => {
                let position = format!("split commit {} of {}", index + 1, drafts.len());
                return Err(match git::restore_snapshot(&snapshot) {
                    Ok(()) if index == 0 => {
                        error.context(format!("{position} failed; nothing committed"))
                    }
                    Ok(()) => error.context(format!(
                        "{position} failed; rolled back the {index} earlier split commits, nothing committed"
                    )),
                    Err(rollback_error) => error.context(format!(
                        "{position} failed and rolling back the {index} earlier split commits also failed ({rollback_error:#}); inspect `git log` and the index"
                    )),
                });
            }
        }
    }
    if !complete_plan && !unplanned.is_empty() {
        git::stage_from_snapshot(&snapshot, &unplanned)?;
    }

    for (index, draft) in drafts.iter().enumerate() {
        ui::section(format!(
            "Split commit {}/{} created",
            index + 1,
            drafts.len()
        ));
        ui::headline(draft.message.lines().next().unwrap_or(&draft.message));
        append_commit_history(config, &draft.message, &draft.group.files);
    }
    Ok(hashes)
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
            true,
        )
        .await
        .unwrap();

        let subjects = git_stdout(repo.path(), ["log", "--format=%s", "-2"]);
        let lines = subjects.lines().collect::<Vec<_>>();
        assert_eq!(lines, vec!["feat: update library", "docs: update readme"]);
    }

    fn draft(files: &[&str], message: &str) -> SplitCommitDraft {
        SplitCommitDraft {
            group: SplitPlanGroup {
                title: message.to_owned(),
                rationale: message.to_owned(),
                files: files.iter().map(|file| (*file).to_owned()).collect(),
            },
            message: message.to_owned(),
        }
    }

    fn test_config() -> Config {
        Config {
            ai_provider: "test".to_owned(),
            gitpush: false,
            ..Config::default()
        }
    }

    #[test]
    fn split_commits_keep_rename_sources_and_unplanned_staged_files() {
        let _cwd = hold_cwd_for_test();
        let repo = init_repo();
        std::fs::write(repo.path().join("README.md"), "docs\n").unwrap();
        std::fs::write(repo.path().join("a.txt"), "a\n").unwrap();
        run_git_test(repo.path(), ["add", "."]);
        run_git_test(repo.path(), ["commit", "-m", "init"]);

        run_git_test(repo.path(), ["mv", "README.md", "GUIDE.md"]);
        std::fs::write(repo.path().join("a.txt"), "a2\n").unwrap();
        std::fs::write(repo.path().join("Cargo.lock"), "lock\n").unwrap();
        run_git_test(repo.path(), ["add", "a.txt", "Cargo.lock"]);

        let _dir = CurrentDirGuard::enter(repo.path());
        // The plan never names Cargo.lock (ignore-filtered) or README.md
        // (rename source); both must still land in a commit.
        commit_split_drafts(
            &test_config(),
            &[
                draft(&["GUIDE.md"], "docs: rename guide"),
                draft(&["a.txt"], "feat: update a"),
            ],
            &[],
            true,
        )
        .unwrap();

        assert_eq!(git_stdout(repo.path(), ["status", "--porcelain"]), "");
        assert_eq!(
            git_stdout(
                repo.path(),
                ["show", "--name-status", "--format=", "HEAD~1"]
            ),
            "R100\tREADME.md\tGUIDE.md"
        );
        assert_eq!(
            git_stdout(repo.path(), ["show", "--name-only", "--format=", "HEAD"]),
            "Cargo.lock\na.txt"
        );
    }

    #[cfg(unix)]
    #[test]
    fn failed_split_commit_rolls_back_earlier_commits() {
        let _cwd = hold_cwd_for_test();
        let repo = init_repo();
        std::fs::write(repo.path().join("base.txt"), "base\n").unwrap();
        run_git_test(repo.path(), ["add", "."]);
        run_git_test(repo.path(), ["commit", "-m", "init"]);
        let head = git_stdout(repo.path(), ["rev-parse", "HEAD"]);

        let hook = repo.path().join(".git/hooks/commit-msg");
        std::fs::write(&hook, "#!/bin/sh\n! grep -q reject \"$1\"\n").unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        std::fs::write(repo.path().join("a.txt"), "a\n").unwrap();
        std::fs::write(repo.path().join("b.txt"), "b\n").unwrap();
        run_git_test(repo.path(), ["add", "a.txt", "b.txt"]);

        let _dir = CurrentDirGuard::enter(repo.path());
        let error = commit_split_drafts(
            &test_config(),
            &[
                draft(&["a.txt"], "feat: add a"),
                draft(&["b.txt"], "feat: reject b"),
            ],
            &[],
            true,
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .starts_with("split commit 2 of 2 failed; rolled back")
        );
        assert_eq!(git_stdout(repo.path(), ["rev-parse", "HEAD"]), head);
        assert_eq!(
            git_stdout(repo.path(), ["diff", "--cached", "--name-only"]),
            "a.txt\nb.txt"
        );
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
