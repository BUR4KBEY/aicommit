use anyhow::{Result, bail};

use crate::{
    config::Config,
    generator::{GitGuidanceRequest, generate_git_guidance},
    git::{self, GitRecoveryScenario, GitSyncSnapshot, GitSyncState},
    ui,
};

pub(crate) async fn enforce_pre_commit_sync_guard(config: &Config, dry_run: bool) -> Result<()> {
    if !should_check_upstream_before_commit(config.gitpush, dry_run) {
        return Ok(());
    }

    let spinner = ui::spinner("Checking branch sync with remote");
    let snapshot = git::fetch_sync_snapshot();
    spinner.finish_and_clear();
    let snapshot = snapshot?;
    match snapshot.state {
        GitSyncState::NoUpstream | GitSyncState::UpToDate | GitSyncState::AheadOnly => Ok(()),
        GitSyncState::BehindOnly => {
            let request = GitGuidanceRequest {
                scenario: GitRecoveryScenario::PreCommitBehind,
                snapshot,
                commit_created: false,
                git_output: None,
            };
            render_guidance(config, "Branch sync required", &request).await;
            bail!("branch is behind its upstream; run `git pull --rebase` before using `aic`");
        }
        GitSyncState::Diverged => {
            let request = GitGuidanceRequest {
                scenario: GitRecoveryScenario::PreCommitDiverged,
                snapshot,
                commit_created: false,
                git_output: None,
            };
            render_guidance(config, "Branch sync required", &request).await;
            bail!("branch has diverged from its upstream; sync it before using `aic`");
        }
    }
}

pub(crate) async fn render_guidance(config: &Config, title: &str, request: &GitGuidanceRequest) {
    ui::section(title);
    ui::metadata_row(&snapshot_metadata(&request.snapshot));
    let guidance = generate_git_guidance(config, request).await;
    ui::markdown_card("Git guidance", &guidance);
}

pub(crate) fn should_check_upstream_before_commit(gitpush: bool, dry_run: bool) -> bool {
    gitpush && !dry_run
}

fn snapshot_metadata(snapshot: &GitSyncSnapshot) -> Vec<String> {
    let mut items = Vec::new();
    if let Some(branch) = snapshot.branch.as_deref() {
        items.push(format!("branch: {branch}"));
    }
    if let Some(upstream) = snapshot.upstream_ref.as_deref() {
        items.push(format!("upstream: {upstream}"));
    }
    if let Some(remote) = snapshot.remote.as_deref() {
        items.push(format!("remote: {remote}"));
    }
    items.push(format!("ahead: {}", snapshot.ahead));
    items.push(format!("behind: {}", snapshot.behind));
    items
}
