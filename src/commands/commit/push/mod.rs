use anyhow::{Result, anyhow};

use crate::config::Config;

mod display;
mod plan;

pub(crate) use plan::build_push_plan;
use plan::{PushPlan, PushRemoteOption};

const PUSH_NOW_OPTION: &str = "Push now";
const SKIP_PUSH_OPTION: &str = "Skip";
const PULL_REBASE_AND_RETRY_OPTION: &str = "Pull with rebase and retry push";
const KEEP_LOCAL_COMMIT_OPTION: &str = "Keep local commit only";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PushOutcome {
    Skipped,
    Pushed(PushRemoteOption),
}

pub(crate) async fn commit_and_maybe_push(
    config: &Config,
    message: &str,
    extra_args: &[String],
    staged_files: &[String],
    skip_confirmation: bool,
) -> Result<()> {
    let push_plan = build_push_plan(
        config.gitpush,
        skip_confirmation,
        &crate::git::remote_metadata()?,
        &config.remote_icon_style,
    )?;

    let output = crate::git::commit(message, &super::filtered_extra_args(config, extra_args))?;
    let commit_hash = crate::git::head_short_hash().ok();
    let branch = crate::git::current_branch();

    super::helpers::append_commit_history(config, message, staged_files);

    let push_outcome = execute_push_plan(push_plan, config, skip_confirmation).await?;

    crate::ui::section("Commit created");
    let mut items = Vec::new();
    if let Some(commit_hash) = commit_hash {
        let commit_url = match &push_outcome {
            PushOutcome::Pushed(remote) => remote
                .commit_url_base
                .as_ref()
                .map(|base| format!("{base}/{commit_hash}")),
            PushOutcome::Skipped => None,
        };
        items.push(match commit_url {
            Some(url) => crate::ui::hyperlink(&commit_hash, &url),
            None => commit_hash,
        });
    }
    if let Some(branch) = branch {
        items.push(branch);
    }
    if let PushOutcome::Pushed(remote) = &push_outcome {
        items.push(format!("pushed to {}", remote.summary));
    }
    if let Some(diffstat) = diffstat_summary(&output.stdout) {
        items.push(diffstat);
    }
    crate::ui::metadata_row(&items);
    crate::ui::headline(message.lines().next().unwrap_or(message));

    Ok(())
}

/// Condense git commit's " 3 files changed, 40 insertions(+), 2 deletions(-)"
/// stat line into "+40 −2".
fn diffstat_summary(commit_stdout: &str) -> Option<String> {
    let stat_line = commit_stdout
        .lines()
        .find(|line| line.contains(" changed"))?;
    let count_before = |marker: &str| -> Option<u64> {
        let end = stat_line.find(marker)?;
        stat_line[..end]
            .rsplit(|c: char| !c.is_ascii_digit())
            .find(|part| !part.is_empty())?
            .parse()
            .ok()
    };

    let insertions = count_before(" insertion");
    let deletions = count_before(" deletion");
    let mut parts = Vec::new();
    if let Some(insertions) = insertions {
        parts.push(format!("+{insertions}"));
    }
    if let Some(deletions) = deletions {
        parts.push(format!("−{deletions}"));
    }
    if parts.is_empty() {
        return None;
    }
    Some(parts.join(" "))
}

pub(crate) async fn execute_push_plan(
    plan: PushPlan,
    config: &Config,
    skip_confirmation: bool,
) -> Result<PushOutcome> {
    match plan {
        PushPlan::Skip => Ok(PushOutcome::Skipped),
        PushPlan::AutoPush(remote) => push_to_remote(&remote, config, skip_confirmation).await,
        PushPlan::ConfirmSingle { remote } => {
            let selection = crate::ui::select(
                &format!("Push this commit to {}?", remote.label),
                single_remote_push_actions(),
            )?;
            if selection == PUSH_NOW_OPTION {
                push_to_remote(&remote, config, skip_confirmation).await
            } else {
                Ok(PushOutcome::Skipped)
            }
        }
        PushPlan::SelectRemote(remotes) => {
            let selected =
                crate::ui::select("Choose a remote to push to", remote_push_options(&remotes))?;
            if let Some(remote) = remotes.iter().find(|remote| remote.label == selected) {
                push_to_remote(remote, config, skip_confirmation).await
            } else {
                Ok(PushOutcome::Skipped)
            }
        }
    }
}

async fn push_to_remote(
    remote: &PushRemoteOption,
    config: &Config,
    skip_confirmation: bool,
) -> Result<PushOutcome> {
    match crate::git::push(Some(&remote.name)) {
        Ok(_) => {
            crate::ui::success(push_success_message(remote));
            Ok(PushOutcome::Pushed(remote.clone()))
        }
        Err(error) => {
            let message = error.to_string();
            if !is_sync_push_rejection(&message) {
                return Err(error);
            }

            let snapshot = crate::git::fetch_sync_snapshot()?;
            let request = crate::generator::GitGuidanceRequest {
                scenario: crate::git::GitRecoveryScenario::PushRejected,
                snapshot: snapshot.clone(),
                commit_created: true,
                git_output: Some(message.clone()),
            };
            super::git_sync::render_guidance(config, "Push needs attention", &request).await;

            if should_offer_rebase_retry(&snapshot, skip_confirmation)? {
                let selection = crate::ui::select(
                    "How would you like to recover from this push rejection?",
                    rebase_retry_options(),
                )?;
                if selection == PULL_REBASE_AND_RETRY_OPTION {
                    return retry_push_after_rebase(remote, config).await;
                }

                crate::ui::warn("Keeping the new commit locally; it was not pushed.");
                return Ok(PushOutcome::Skipped);
            }

            Err(anyhow!(
                "commit was created locally, but the push was rejected; sync the branch and push again"
            ))
        }
    }
}

pub(crate) fn single_remote_push_actions() -> Vec<String> {
    vec![PUSH_NOW_OPTION.to_owned(), SKIP_PUSH_OPTION.to_owned()]
}

fn remote_push_options(remotes: &[PushRemoteOption]) -> Vec<String> {
    let mut options = remotes
        .iter()
        .map(|remote| remote.label.clone())
        .collect::<Vec<_>>();
    options.push(SKIP_PUSH_OPTION.to_owned());
    options
}

fn rebase_retry_options() -> Vec<String> {
    vec![
        PULL_REBASE_AND_RETRY_OPTION.to_owned(),
        KEEP_LOCAL_COMMIT_OPTION.to_owned(),
    ]
}

fn should_offer_rebase_retry(
    snapshot: &crate::git::GitSyncSnapshot,
    skip_confirmation: bool,
) -> Result<bool> {
    Ok(!skip_confirmation && snapshot.behind > 0 && crate::git::is_clean_for_pull_rebase()?)
}

fn is_sync_push_rejection(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.contains("failed to push some refs")
        && (lower.contains("fetch first")
            || lower.contains("non-fast-forward")
            || lower.contains("rejected"))
}

async fn retry_push_after_rebase(
    remote: &PushRemoteOption,
    config: &Config,
) -> Result<PushOutcome> {
    match crate::git::pull_rebase()? {
        crate::git::PullRebaseOutcome::Clean(output) => {
            crate::ui::session_step("Pulled remote changes with rebase");
            render_git_output(&output);
            match crate::git::push(Some(&remote.name)) {
                Ok(_) => {
                    crate::ui::success(push_success_message(remote));
                    Ok(PushOutcome::Pushed(remote.clone()))
                }
                Err(error) => {
                    let snapshot = crate::git::fetch_sync_snapshot()?;
                    let request = crate::generator::GitGuidanceRequest {
                        scenario: crate::git::GitRecoveryScenario::PushRejected,
                        snapshot,
                        commit_created: true,
                        git_output: Some(error.to_string()),
                    };
                    super::git_sync::render_guidance(config, "Push needs attention", &request)
                        .await;
                    Err(anyhow!(
                        "the push still failed after rebasing; inspect the branch state and try again"
                    ))
                }
            }
        }
        crate::git::PullRebaseOutcome::Conflicted { output } => {
            let request = crate::generator::GitGuidanceRequest {
                scenario: crate::git::GitRecoveryScenario::RebaseConflict,
                snapshot: crate::git::sync_snapshot()?,
                commit_created: true,
                git_output: Some(output),
            };
            super::git_sync::render_guidance(config, "Rebase needs attention", &request).await;
            Err(anyhow!(
                "rebase stopped with conflicts; resolve them before pushing again"
            ))
        }
        crate::git::PullRebaseOutcome::Failed { output } => {
            let request = crate::generator::GitGuidanceRequest {
                scenario: crate::git::GitRecoveryScenario::RebaseFailed,
                snapshot: crate::git::sync_snapshot()?,
                commit_created: true,
                git_output: Some(output),
            };
            super::git_sync::render_guidance(config, "Rebase needs attention", &request).await;
            Err(anyhow!(
                "rebase recovery did not complete; inspect the branch and retry manually"
            ))
        }
    }
}

fn push_success_message(remote: &PushRemoteOption) -> String {
    match crate::git::current_branch() {
        Some(branch) => format!("Pushed {branch} → {}", remote.name),
        None => format!("Pushed to {}", remote.name),
    }
}

fn render_git_output(output: &crate::git::GitOutput) {
    if !output.stderr.is_empty() {
        crate::ui::secondary(&output.stderr);
    }
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
    use crate::git::cwd_test_lock;

    #[test]
    fn single_remote_push_actions_are_explicit() {
        assert_eq!(
            single_remote_push_actions(),
            vec!["Push now".to_owned(), "Skip".to_owned()]
        );
    }

    fn plain_option(name: &str) -> PushRemoteOption {
        PushRemoteOption {
            name: name.to_owned(),
            label: name.to_owned(),
            summary: name.to_owned(),
            commit_url_base: None,
        }
    }

    #[test]
    fn remote_push_options_append_skip() {
        let options = remote_push_options(&[plain_option("origin"), plain_option("backup")]);

        assert_eq!(
            options,
            vec!["origin".to_owned(), "backup".to_owned(), "Skip".to_owned()]
        );
    }

    #[test]
    fn diffstat_summary_condenses_the_stat_line() {
        assert_eq!(
            diffstat_summary(
                "[main 8c4d5e6] feat: x\n 3 files changed, 40 insertions(+), 2 deletions(-)"
            ),
            Some("+40 −2".to_owned())
        );
        assert_eq!(
            diffstat_summary("[main 8c4d5e6] feat: x\n 1 file changed, 1 insertion(+)"),
            Some("+1".to_owned())
        );
        assert_eq!(
            diffstat_summary("[main 8c4d5e6] chore: x\n 1 file changed, 5 deletions(-)"),
            Some("−5".to_owned())
        );
        assert_eq!(diffstat_summary("[main 8c4d5e6] chore: rename only"), None);
        assert_eq!(diffstat_summary(""), None);
    }

    #[test]
    fn rebase_retry_options_are_explicit() {
        assert_eq!(
            rebase_retry_options(),
            vec![
                "Pull with rebase and retry push".to_owned(),
                "Keep local commit only".to_owned()
            ]
        );
    }

    #[test]
    fn detects_non_fast_forward_push_rejections() {
        assert!(is_sync_push_rejection(
            "failed to push some refs\n! [rejected] main -> main (fetch first)"
        ));
        assert!(!is_sync_push_rejection("permission denied"));
    }

    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn retry_push_after_rebase_succeeds_for_clean_repo() {
        let _cwd = hold_cwd_for_test();
        let remote = init_bare_repo();
        let local = clone_repo(remote.path());
        commit_file(local.path(), "src.txt", "base\n", "feat: base");
        run_git_test(local.path(), ["push", "-u", "origin", "HEAD"]);

        let peer = clone_repo(remote.path());
        commit_file(peer.path(), "src.txt", "base\nremote\n", "feat: remote");
        run_git_test(peer.path(), ["push"]);

        commit_file(local.path(), "extra.txt", "local\n", "feat: local");

        let outcome = {
            let _dir = CurrentDirGuard::enter(local.path());
            retry_push_after_rebase(
                &plain_option("origin"),
                &Config {
                    ai_provider: "test".to_owned(),
                    gitpush: true,
                    ..Config::default()
                },
            )
            .await
            .unwrap()
        };

        assert!(matches!(outcome, PushOutcome::Pushed(remote) if remote.name == "origin"));
        assert_eq!(
            git_stdout(remote.path(), ["rev-list", "--count", "--all"]),
            "3"
        );
    }

    fn init_bare_repo() -> TempDir {
        let temp = TempDir::new().unwrap();
        run_git_test(temp.path(), ["init", "--bare"]);
        temp
    }

    fn clone_repo(source: &Path) -> TempDir {
        let temp = TempDir::new().unwrap();
        let status = Command::new("git")
            .args([
                "clone",
                source.to_str().unwrap(),
                temp.path().to_str().unwrap(),
            ])
            .status()
            .unwrap();
        assert!(status.success());
        run_git_test(temp.path(), ["config", "user.email", "test@example.com"]);
        run_git_test(temp.path(), ["config", "user.name", "Test User"]);
        temp
    }

    fn commit_file(repo: &Path, file: &str, content: &str, message: &str) {
        std::fs::write(repo.join(file), content).unwrap();
        run_git_test(repo, ["add", file]);
        run_git_test(repo, ["commit", "-m", message]);
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
