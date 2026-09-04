use std::path::Path;

use anyhow::Result;

use crate::git::{
    branch::current_branch,
    exec::{GitOutput, run_git, run_git_dynamic_in},
    repo::{parse_lines, repo_root},
};

use super::host_parser::remote_url_info;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitProvider {
    label: Option<String>,
    nerd_font_icon: Option<String>,
    emoji_icon: Option<String>,
    commit_path: Option<String>,
}

impl GitProvider {
    pub fn known(label: impl Into<String>) -> Self {
        Self {
            label: Some(label.into()),
            nerd_font_icon: None,
            emoji_icon: None,
            commit_path: None,
        }
    }

    pub fn known_with_icons(
        label: impl Into<String>,
        nerd_font_icon: Option<String>,
        emoji_icon: Option<String>,
        commit_path: Option<String>,
    ) -> Self {
        Self {
            label: Some(label.into()),
            nerd_font_icon,
            emoji_icon,
            commit_path,
        }
    }

    pub fn unknown() -> Self {
        Self {
            label: None,
            nerd_font_icon: None,
            emoji_icon: None,
            commit_path: None,
        }
    }

    pub fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }

    pub fn nerd_font_icon(&self) -> Option<&str> {
        self.nerd_font_icon.as_deref()
    }

    pub fn emoji_icon(&self) -> Option<&str> {
        self.emoji_icon.as_deref()
    }

    pub fn commit_path(&self) -> Option<&str> {
        self.commit_path.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitRemoteMetadata {
    pub name: String,
    pub fetch_url: Option<String>,
    pub push_url: Option<String>,
    pub web_url: Option<String>,
    pub provider: GitProvider,
}

impl GitRemoteMetadata {
    fn from_urls(name: String, fetch_url: Option<String>, push_url: Option<String>) -> Self {
        let info = push_url
            .as_deref()
            .or(fetch_url.as_deref())
            .and_then(remote_url_info);
        let (web_url, provider) = info
            .map(|info| (info.web_url, info.provider))
            .unwrap_or((None, GitProvider::unknown()));

        Self {
            name,
            fetch_url,
            push_url,
            web_url,
            provider,
        }
    }
}

pub fn commit(message: &str, extra_args: &[String]) -> Result<GitOutput> {
    let root = repo_root()?;
    let mut args = vec!["commit".to_owned(), "-m".to_owned(), message.to_owned()];
    args.extend(extra_args.iter().cloned());
    run_git_dynamic_in(&root, args)
}

pub fn remotes() -> Result<Vec<String>> {
    Ok(parse_lines(&run_git(["remote"])?.stdout))
}

pub fn remote_metadata() -> Result<Vec<GitRemoteMetadata>> {
    let root = repo_root()?;
    let remotes = remotes()?;
    Ok(remotes
        .into_iter()
        .map(|remote| {
            let fetch_url = remote_get_url(&root, &remote, false);
            let push_url = remote_get_url(&root, &remote, true);
            GitRemoteMetadata::from_urls(remote, fetch_url, push_url)
        })
        .collect())
}

pub fn push(remote: Option<&str>) -> Result<GitOutput> {
    let root = repo_root()?;
    let mut args = vec!["push".to_owned()];
    if let Some(remote) = remote {
        if let Some(branch) = current_branch() {
            args.push("--set-upstream".to_owned());
            args.push(remote.to_owned());
            args.push(branch);
        } else {
            args.push(remote.to_owned());
        }
    }
    run_git_dynamic_in(&root, args)
}

fn remote_get_url(root: &Path, remote: &str, push: bool) -> Option<String> {
    let mut args = vec!["remote".to_owned(), "get-url".to_owned()];
    if push {
        args.push("--push".to_owned());
    }
    args.push(remote.to_owned());

    run_git_dynamic_in(root, args)
        .ok()
        .map(|output| output.stdout)
        .filter(|url| !url.trim().is_empty())
}
