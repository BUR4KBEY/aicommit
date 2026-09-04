use std::sync::OnceLock;

use serde::Deserialize;

use super::metadata::GitProvider;

const GIT_HOSTS_TOML: &str = include_str!("../../git_hosts.toml");

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RemoteUrlInfo {
    pub(super) web_url: Option<String>,
    pub(super) provider: GitProvider,
}

#[derive(Debug, Deserialize)]
struct GitHostConfig {
    #[serde(default)]
    providers: Vec<GitHostProvider>,
}

#[derive(Debug, Deserialize)]
struct GitHostProvider {
    label: String,
    nerd_font_icon: Option<String>,
    emoji_icon: Option<String>,
    commit_path: Option<String>,
    #[serde(default)]
    hosts: Vec<String>,
    #[serde(default)]
    host_suffixes: Vec<String>,
    #[serde(default)]
    rewrites: Vec<GitHostRewrite>,
}

#[derive(Debug, Deserialize)]
struct GitHostRewrite {
    host: String,
    path_prefix: String,
    template: String,
}

pub(super) fn remote_url_info(url: &str) -> Option<RemoteUrlInfo> {
    let (host, path) = remote_url_host_and_path(url)?;
    let provider_config = provider_for_host(&host);
    let provider = provider_config
        .map(|provider| {
            GitProvider::known_with_icons(
                provider.label.clone(),
                provider.nerd_font_icon.clone(),
                provider.emoji_icon.clone(),
                provider.commit_path.clone(),
            )
        })
        .unwrap_or_else(GitProvider::unknown);
    let web_url = web_url_for_remote(&host, &path, provider_config);

    Some(RemoteUrlInfo { web_url, provider })
}

fn remote_url_host_and_path(url: &str) -> Option<(String, String)> {
    let trimmed = url.trim().trim_end_matches('/');

    if let Some(rest) = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
    {
        return split_host_and_path(rest);
    }

    if let Some(rest) = trimmed.strip_prefix("ssh://") {
        return split_host_and_path(rest);
    }

    if !trimmed.contains("://") {
        return split_scp_style_remote(trimmed);
    }

    None
}

fn split_host_and_path(input: &str) -> Option<(String, String)> {
    let (host, path) = input.split_once('/')?;
    let host = normalize_host(host);
    let path = normalize_path(path);

    if host.is_empty() || path.is_empty() {
        return None;
    }

    Some((host, path))
}

fn split_scp_style_remote(input: &str) -> Option<(String, String)> {
    let (host, path) = input.split_once(':')?;

    if host.contains('/') || (!host.contains('@') && !host.contains('.')) {
        return None;
    }

    let host = normalize_host(host);
    let path = normalize_path(path);

    if host.is_empty() || path.is_empty() {
        return None;
    }

    Some((host, path))
}

fn normalize_host(host: &str) -> String {
    host.rsplit('@')
        .next()
        .unwrap_or(host)
        .split(':')
        .next()
        .unwrap_or(host)
        .to_lowercase()
}

fn normalize_path(path: &str) -> String {
    let path = path.split(['?', '#']).next().unwrap_or(path);
    let path = path.trim_matches('/');
    path.strip_suffix(".git").unwrap_or(path).to_owned()
}

fn provider_for_host(host: &str) -> Option<&'static GitHostProvider> {
    host_provider_config()
        .providers
        .iter()
        .find(|provider| provider.matches_host(host))
}

fn web_url_for_remote(
    host: &str,
    path: &str,
    provider: Option<&GitHostProvider>,
) -> Option<String> {
    if let Some(url) = provider.and_then(|provider| provider.rewrite_web_url(host, path)) {
        return Some(url);
    }

    provider.map(|_| format!("https://{host}/{path}"))
}

fn host_provider_config() -> &'static GitHostConfig {
    static CONFIG: OnceLock<GitHostConfig> = OnceLock::new();
    CONFIG.get_or_init(|| {
        toml_edit::de::from_str(GIT_HOSTS_TOML).expect("embedded git host config should be valid")
    })
}

impl GitHostProvider {
    fn matches_host(&self, host: &str) -> bool {
        self.hosts.iter().any(|candidate| candidate == host)
            || self
                .host_suffixes
                .iter()
                .any(|suffix| host.ends_with(suffix))
    }

    fn rewrite_web_url(&self, host: &str, path: &str) -> Option<String> {
        self.rewrites
            .iter()
            .find(|rewrite| rewrite.host == host)
            .and_then(|rewrite| rewrite.render(path))
    }
}

impl GitHostRewrite {
    fn render(&self, path: &str) -> Option<String> {
        let prefix_parts = self
            .path_prefix
            .split('/')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>();
        let path_parts = path.split('/').collect::<Vec<_>>();

        if path_parts.len() <= prefix_parts.len()
            || !path_parts
                .iter()
                .zip(prefix_parts.iter())
                .all(|(path, prefix)| path == prefix)
        {
            return None;
        }

        let values = &path_parts[prefix_parts.len()..];
        let mut rendered = self.template.clone();

        for (index, value) in values.iter().enumerate() {
            rendered = rendered.replace(&format!("{{{}}}", index + 1), value);
            rendered = rendered.replace(&format!("{{{}+}}", index + 1), &values[index..].join("/"));
        }

        if rendered.contains('{') || rendered.contains('}') {
            return None;
        }

        Some(rendered)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn known_provider(
        label: &str,
        nerd_font_icon: &str,
        emoji_icon: &str,
        commit_path: Option<&str>,
    ) -> GitProvider {
        GitProvider::known_with_icons(
            label,
            Some(nerd_font_icon.to_owned()),
            Some(emoji_icon.to_owned()),
            commit_path.map(ToOwned::to_owned),
        )
    }

    #[test]
    fn parses_github_https_remote() {
        assert_eq!(
            remote_url_info("https://github.com/russmckendrick/aicommit.git"),
            Some(RemoteUrlInfo {
                web_url: Some("https://github.com/russmckendrick/aicommit".to_owned()),
                provider: known_provider("GitHub", "", "🐙", Some("commit")),
            })
        );
    }

    #[test]
    fn parses_bitbucket_ssh_remote() {
        assert_eq!(
            remote_url_info("git@bitbucket.org:workspace/project.git"),
            Some(RemoteUrlInfo {
                web_url: Some("https://bitbucket.org/workspace/project".to_owned()),
                provider: known_provider("Bitbucket", "", "🪣", Some("commits")),
            })
        );
    }

    #[test]
    fn parses_gitlab_ssh_remote() {
        assert_eq!(
            remote_url_info("git@gitlab.com:group/project.git"),
            Some(RemoteUrlInfo {
                web_url: Some("https://gitlab.com/group/project".to_owned()),
                provider: known_provider("GitLab", "", "🦊", Some("-/commit")),
            })
        );
    }

    #[test]
    fn parses_azure_devops_https_remote() {
        assert_eq!(
            remote_url_info("https://organization@dev.azure.com/organization/project/_git/repo"),
            Some(RemoteUrlInfo {
                web_url: Some("https://dev.azure.com/organization/project/_git/repo".to_owned()),
                provider: known_provider("Azure DevOps", "", "☁", None),
            })
        );
    }

    #[test]
    fn parses_azure_devops_ssh_remote() {
        assert_eq!(
            remote_url_info("git@ssh.dev.azure.com:v3/organization/project/repo"),
            Some(RemoteUrlInfo {
                web_url: Some("https://dev.azure.com/organization/project/_git/repo".to_owned()),
                provider: known_provider("Azure DevOps", "", "☁", None),
            })
        );
    }

    #[test]
    fn does_not_guess_web_url_for_unknown_scp_style_host() {
        assert_eq!(
            remote_url_info("git@example.com:team/repo.git"),
            Some(RemoteUrlInfo {
                web_url: None,
                provider: GitProvider::unknown(),
            })
        );
    }

    #[test]
    fn strips_git_suffix_from_remote_url() {
        assert_eq!(
            remote_url_info("ssh://git@github.com:22/team/repo.git"),
            Some(RemoteUrlInfo {
                web_url: Some("https://github.com/team/repo".to_owned()),
                provider: known_provider("GitHub", "", "🐙", Some("commit")),
            })
        );
    }

    #[test]
    fn does_not_guess_web_url_for_unknown_https_host() {
        assert_eq!(
            remote_url_info("https://git.example.test/team/repo.git"),
            Some(RemoteUrlInfo {
                web_url: None,
                provider: GitProvider::unknown(),
            })
        );
    }
}
