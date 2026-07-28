use std::{
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{bail, Context, Result};
use ignore::gitignore::GitignoreBuilder;

use crate::{config::REPO_IGNORE_FILE, errors::AicError};

use super::exec::{run_git, run_git_dynamic_in, run_git_in};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeSummary {
    pub status: String,
    pub path: String,
    pub previous_path: Option<String>,
}

pub fn assert_git_repo() -> Result<()> {
    run_git(["rev-parse"])?;
    Ok(())
}

pub fn repo_root() -> Result<PathBuf> {
    let output = run_git(["rev-parse", "--show-toplevel"])?;
    if output.stdout.trim().is_empty() {
        bail!(AicError::NotGitRepository);
    }
    Ok(PathBuf::from(output.stdout.trim()))
}

pub fn staged_files() -> Result<Vec<String>> {
    let root = repo_root()?;
    let output = run_git_in(&root, ["diff", "--name-only", "--cached", "--relative"])?;
    let files = parse_lines(&output.stdout);
    filter_ignored(&root, files)
}

pub fn changed_files() -> Result<Vec<String>> {
    let root = repo_root()?;
    let modified = run_git_in(&root, ["ls-files", "--modified"])?;
    let others = run_git_in(&root, ["ls-files", "--others", "--exclude-standard"])?;
    let mut files = parse_lines(&modified.stdout);
    files.extend(parse_lines(&others.stdout));
    files.sort();
    files.dedup();
    Ok(files)
}

pub fn add_files(files: &[String]) -> Result<()> {
    if files.is_empty() {
        return Ok(());
    }

    let root = repo_root()?;
    let mut args = vec!["add".to_owned()];
    args.extend(files.iter().cloned());
    run_git_dynamic_in(&root, args)?;
    Ok(())
}

pub fn unstage_files(files: &[String]) -> Result<()> {
    if files.is_empty() {
        return Ok(());
    }

    let root = repo_root()?;
    let mut args = if head_exists(&root)? {
        vec!["reset".to_owned(), "HEAD".to_owned(), "--".to_owned()]
    } else {
        vec!["rm".to_owned(), "--cached".to_owned(), "--".to_owned()]
    };
    args.extend(files.iter().cloned());
    run_git_dynamic_in(&root, args)?;
    Ok(())
}

pub fn clear_index() -> Result<()> {
    let root = repo_root()?;
    run_git_dynamic_in(
        &root,
        vec![
            "reset".to_owned(),
            "HEAD".to_owned(),
            "--".to_owned(),
            ".".to_owned(),
        ],
    )?;
    Ok(())
}

pub fn staged_diff(files: &[String]) -> Result<String> {
    let root = repo_root()?;
    let files = files
        .iter()
        .filter(|file| !is_excluded_from_diff(file))
        .cloned()
        .collect::<Vec<_>>();

    if files.is_empty() {
        return Ok(String::new());
    }

    let mut args = vec!["diff".to_owned(), "--staged".to_owned(), "--".to_owned()];
    args.extend(files);
    Ok(run_git_dynamic_in(&root, args)?.stdout)
}

pub fn staged_change_summaries(files: &[String]) -> Result<Vec<ChangeSummary>> {
    let root = repo_root()?;
    change_summaries_from_args(
        &root,
        vec![
            "diff".to_owned(),
            "--cached".to_owned(),
            "--name-status".to_owned(),
            "--find-renames".to_owned(),
            "--".to_owned(),
        ],
        files,
    )
}

pub fn partially_staged_files(staged_files: &[String]) -> Result<Vec<String>> {
    if staged_files.is_empty() {
        return Ok(Vec::new());
    }

    let root = repo_root()?;
    let unstaged = run_git_in(&root, ["diff", "--name-only", "--relative"])?;
    let unstaged = parse_lines(&unstaged.stdout)
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    let mut partial = staged_files
        .iter()
        .filter(|file| unstaged.contains(*file))
        .cloned()
        .collect::<Vec<_>>();
    partial.sort();
    partial.dedup();
    Ok(partial)
}

pub fn assert_clean_worktree() -> Result<()> {
    let root = repo_root()?;
    let output = run_git_in(&root, ["status", "--porcelain"])?;
    if !output.stdout.trim().is_empty() {
        bail!("working tree has uncommitted changes; commit or stash them first");
    }
    Ok(())
}

pub(crate) fn parse_lines(input: &str) -> Vec<String> {
    input
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

pub(crate) fn filter_ignored(root: &Path, files: Vec<String>) -> Result<Vec<String>> {
    let ignore_path = root.join(REPO_IGNORE_FILE);
    if !ignore_path.exists() {
        return Ok(files);
    }

    let mut builder = GitignoreBuilder::new(root);
    if let Some(err) = builder.add(&ignore_path) {
        return Err(err).context("failed to read .aicommitignore");
    }
    let matcher = builder.build()?;
    Ok(files
        .into_iter()
        .filter(|file| !matcher.matched_path_or_any_parents(file, false).is_ignore())
        .collect())
}

pub(crate) fn change_summaries_from_args(
    root: &Path,
    mut args: Vec<String>,
    files: &[String],
) -> Result<Vec<ChangeSummary>> {
    if files.is_empty() {
        return Ok(Vec::new());
    }

    args.extend(files.iter().cloned());
    Ok(parse_name_status(&run_git_dynamic_in(root, args)?.stdout))
}

fn is_excluded_from_diff(file: &str) -> bool {
    let lower = file.to_lowercase();
    lower.contains(".lock")
        || lower.contains("-lock.")
        || lower.ends_with(".svg")
        || lower.ends_with(".png")
        || lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".webp")
        || lower.ends_with(".gif")
}

pub(crate) fn parse_name_status(input: &str) -> Vec<ChangeSummary> {
    input
        .lines()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let raw_status = fields.next()?.trim();
            let first_path = fields.next()?.trim();
            let second_path = fields.next().map(str::trim);
            let status = match raw_status.chars().next().unwrap_or('M') {
                'A' => "added",
                'M' => "modified",
                'D' => "deleted",
                'R' => "renamed",
                'C' => "copied",
                'T' => "type-changed",
                'U' => "unmerged",
                _ => "changed",
            }
            .to_owned();

            match raw_status.chars().next().unwrap_or('M') {
                'R' | 'C' => Some(ChangeSummary {
                    status,
                    path: second_path.unwrap_or(first_path).to_owned(),
                    previous_path: Some(first_path.to_owned()),
                }),
                _ => Some(ChangeSummary {
                    status,
                    path: first_path.to_owned(),
                    previous_path: None,
                }),
            }
        })
        .collect()
}

fn head_exists(root: &Path) -> Result<bool> {
    let output = Command::new("git")
        .args(["rev-parse", "--verify", "HEAD"])
        .current_dir(root)
        .output()?;
    Ok(output.status.success())
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
    fn detects_partially_staged_files() {
        let _cwd = hold_cwd_for_test();
        let repo = init_repo();
        std::fs::write(repo.path().join("src.txt"), "base\nstaged\nunstaged\n").unwrap();
        run_git_test(repo.path(), ["add", "src.txt"]);
        std::fs::write(
            repo.path().join("src.txt"),
            "base\nstaged\nunstaged\nmore\n",
        )
        .unwrap();

        let _dir = CurrentDirGuard::enter(repo.path());
        let partial = partially_staged_files(&["src.txt".to_owned()]).unwrap();

        assert_eq!(partial, vec!["src.txt".to_owned()]);
    }

    #[test]
    fn clear_index_unstages_files() {
        let _cwd = hold_cwd_for_test();
        let repo = init_repo();
        std::fs::write(repo.path().join("extra.txt"), "hello\n").unwrap();
        run_git_test(repo.path(), ["add", "extra.txt"]);

        let _dir = CurrentDirGuard::enter(repo.path());
        clear_index().unwrap();
        let staged_after = staged_files().unwrap();

        assert!(staged_after.is_empty());
    }

    #[test]
    fn unstage_files_removes_only_selected_files_from_index() {
        let _cwd = hold_cwd_for_test();
        let repo = init_repo();
        std::fs::write(repo.path().join("extra.txt"), "hello\n").unwrap();
        std::fs::write(repo.path().join("keep.txt"), "keep\n").unwrap();
        run_git_test(repo.path(), ["add", "extra.txt", "keep.txt"]);

        let _dir = CurrentDirGuard::enter(repo.path());
        unstage_files(&["extra.txt".to_owned()]).unwrap();
        let staged_after = staged_files().unwrap();

        assert_eq!(staged_after, vec!["keep.txt".to_owned()]);
    }

    #[test]
    fn unstage_files_preserves_working_tree_contents() {
        let _cwd = hold_cwd_for_test();
        let repo = init_repo();
        std::fs::write(repo.path().join("extra.txt"), "hello\n").unwrap();
        run_git_test(repo.path(), ["add", "extra.txt"]);

        let _dir = CurrentDirGuard::enter(repo.path());
        unstage_files(&["extra.txt".to_owned()]).unwrap();

        assert_eq!(
            std::fs::read_to_string(repo.path().join("extra.txt")).unwrap(),
            "hello\n"
        );
        assert!(changed_files().unwrap().contains(&"extra.txt".to_owned()));
    }

    #[test]
    fn unstage_files_works_before_first_commit() {
        let _cwd = hold_cwd_for_test();
        let repo = init_unborn_repo();
        std::fs::write(repo.path().join("extra.txt"), "hello\n").unwrap();
        run_git_test(repo.path(), ["add", "extra.txt"]);

        let _dir = CurrentDirGuard::enter(repo.path());
        unstage_files(&["extra.txt".to_owned()]).unwrap();

        assert!(staged_files().unwrap().is_empty());
        assert_eq!(
            std::fs::read_to_string(repo.path().join("extra.txt")).unwrap(),
            "hello\n"
        );
        assert!(changed_files().unwrap().contains(&"extra.txt".to_owned()));
    }

    #[test]
    fn parse_name_status_tracks_rename_metadata() {
        let parsed = parse_name_status("A\tassets/image.png\nR100\told.png\tnew.png\n");

        assert_eq!(
            parsed,
            vec![
                ChangeSummary {
                    status: "added".to_owned(),
                    path: "assets/image.png".to_owned(),
                    previous_path: None,
                },
                ChangeSummary {
                    status: "renamed".to_owned(),
                    path: "new.png".to_owned(),
                    previous_path: Some("old.png".to_owned()),
                },
            ]
        );
    }

    fn init_repo() -> TempDir {
        let temp = TempDir::new().unwrap();
        run_git_test(temp.path(), ["init", "-b", "main"]);
        run_git_test(temp.path(), ["config", "user.email", "test@example.com"]);
        run_git_test(temp.path(), ["config", "user.name", "Test User"]);
        std::fs::write(temp.path().join("src.txt"), "base\n").unwrap();
        run_git_test(temp.path(), ["add", "src.txt"]);
        run_git_test(temp.path(), ["commit", "-m", "feat: initial"]);
        temp
    }

    fn init_unborn_repo() -> TempDir {
        let temp = TempDir::new().unwrap();
        run_git_test(temp.path(), ["init", "-b", "main"]);
        run_git_test(temp.path(), ["config", "user.email", "test@example.com"]);
        run_git_test(temp.path(), ["config", "user.name", "Test User"]);
        temp
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
