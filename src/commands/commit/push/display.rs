use crate::git;

/// Prompt label like "[<icon> GitHub] origin" - never includes the URL.
pub(crate) fn remote_short_label(remote: &git::GitRemoteMetadata, icon_style: &str) -> String {
    let style = RemoteIconStyle::from_config(icon_style);
    match provider_display_label(&remote.provider, style) {
        Some(provider) => format!("[{provider}] {}", remote.name),
        None => remote.name.clone(),
    }
}

/// Summary label like "origin (GitHub)"; just the name for unknown hosts.
pub(crate) fn remote_summary_label(remote: &git::GitRemoteMetadata) -> String {
    match remote.provider.label() {
        Some(label) => format!("{} ({label})", remote.name),
        None => remote.name.clone(),
    }
}

/// Base URL for commit pages on the remote's host (append "/<hash>").
pub(crate) fn commit_url_base(remote: &git::GitRemoteMetadata) -> Option<String> {
    let web_url = remote.web_url.as_deref()?;
    let commit_path = remote.provider.commit_path()?;
    Some(format!("{web_url}/{commit_path}"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RemoteIconStyle {
    Auto,
    NerdFont,
    Emoji,
    Label,
}

impl RemoteIconStyle {
    fn from_config(value: &str) -> Self {
        match value.trim().to_lowercase().as_str() {
            "nerd" | "nerd-font" | "nerdfont" => Self::NerdFont,
            "emoji" => Self::Emoji,
            "label" | "labels" | "none" | "off" => Self::Label,
            _ => Self::Auto,
        }
    }
}

fn provider_display_label(provider: &git::GitProvider, style: RemoteIconStyle) -> Option<String> {
    let label = provider.label()?;
    let icon = match style {
        RemoteIconStyle::Auto | RemoteIconStyle::NerdFont => provider
            .nerd_font_icon()
            .or_else(|| provider.emoji_icon())
            .filter(|_| style != RemoteIconStyle::Label),
        RemoteIconStyle::Emoji => provider.emoji_icon(),
        RemoteIconStyle::Label => None,
    };

    Some(match icon {
        Some(icon) => format!("{icon} {label}"),
        None => label.to_owned(),
    })
}
