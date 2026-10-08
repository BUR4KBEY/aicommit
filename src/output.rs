use serde::Serialize;

/// Machine-readable output mode selected via top-level CLI flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputMode {
    /// Human TUI output (default, unchanged behavior).
    #[default]
    Human,
    /// `--quiet`: decisive line(s) only, no decoration.
    Quiet,
    /// `--json`: single-line JSON object on stdout, diagnostics on stderr.
    Json,
}

impl OutputMode {
    /// Resolve `--json` / `--quiet` into a mode. Both flags together is an
    /// error.
    pub fn from_flags(json: bool, quiet: bool) -> anyhow::Result<Self> {
        match (json, quiet) {
            (true, true) => anyhow::bail!("--json and --quiet are mutually exclusive"),
            (true, false) => Ok(Self::Json),
            (false, true) => Ok(Self::Quiet),
            (false, false) => Ok(Self::Human),
        }
    }

    pub fn is_machine(self) -> bool {
        !matches!(self, Self::Human)
    }

    pub fn is_json(self) -> bool {
        matches!(self, Self::Json)
    }
}

/// Stable JSON envelope for `commit` / `review` / `pr`.
/// Tolerant of additions; consumers must ignore unknown fields.
#[derive(Debug, Clone, Serialize)]
pub struct JsonOutput {
    pub command: String,
    pub dry_run: bool,
    pub provider: String,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commits: Option<Vec<JsonCommit>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub split_plan: Option<Vec<JsonSplitGroup>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonError>,
}

#[derive(Debug, Clone, Serialize)]
pub struct JsonCommit {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    pub subject: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct JsonSplitGroup {
    pub title: String,
    pub summary: String,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct JsonError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    pub attempts: u32,
}

impl From<&crate::prompt::SplitPlanGroup> for JsonSplitGroup {
    fn from(group: &crate::prompt::SplitPlanGroup) -> Self {
        Self {
            title: group.title.clone(),
            summary: group.rationale.clone(),
            files: group.files.clone(),
        }
    }
}

/// Split a commit message into subject (first line) and optional body.
pub fn split_message(message: &str) -> (String, Option<String>) {
    let mut lines = message.lines();
    let subject = lines.next().unwrap_or_default().trim().to_owned();
    let body: String = lines.collect::<Vec<_>>().join("\n").trim().to_owned();
    if body.is_empty() {
        (subject, None)
    } else {
        (subject, Some(body))
    }
}

/// Map an error to a stable `(code, retryable)` pair.
/// `attempts` is always 1: aic does not retry provider calls.
pub fn error_code(error: &anyhow::Error) -> (String, bool) {
    if let Some(aic) = error.downcast_ref::<crate::errors::AicError>() {
        return match aic {
            crate::errors::AicError::NoChanges => ("no_changes".to_owned(), false),
            crate::errors::AicError::NotGitRepository => ("not_git_repo".to_owned(), false),
            crate::errors::AicError::MissingApiKey(_) => ("missing_api_key".to_owned(), false),
            crate::errors::AicError::ModelNotFound { .. } => ("model_not_found".to_owned(), false),
            crate::errors::AicError::Authentication(_) => ("auth_failed".to_owned(), false),
            crate::errors::AicError::RateLimited(_) => ("rate_limited".to_owned(), true),
            crate::errors::AicError::InsufficientCredits(_) => {
                ("insufficient_credits".to_owned(), false)
            }
            crate::errors::AicError::ServiceUnavailable(_) => {
                ("service_unavailable".to_owned(), true)
            }
            crate::errors::AicError::EmptyMessage => ("empty_response".to_owned(), false),
            crate::errors::AicError::TooManyTokens => ("too_many_tokens".to_owned(), false),
            crate::errors::AicError::UnsupportedConfigKey(_)
            | crate::errors::AicError::InvalidConfigValue { .. } => {
                ("invalid_config".to_owned(), false)
            }
        };
    }

    let message = error.to_string().to_lowercase();
    if message.contains("aborted") || message.contains("cancelled") {
        ("aborted".to_owned(), false)
    } else if message.contains("no files staged") || message.contains("no changes") {
        ("no_changes".to_owned(), false)
    } else if message.contains("not a git repository") {
        ("not_git_repo".to_owned(), false)
    } else {
        ("unknown".to_owned(), false)
    }
}

/// Print a single-line JSON object to stdout. Never emits ANSI bytes.
pub fn emit_json(output: &JsonOutput) {
    match serde_json::to_string(output) {
        Ok(line) => println!("{line}"),
        Err(error) => {
            eprintln!("failed to serialize JSON output: {error}");
        }
    }
}

pub fn json_error_output(
    command: &str,
    dry_run: bool,
    provider: &str,
    model: &str,
    error: &anyhow::Error,
) -> JsonOutput {
    let (code, retryable) = error_code(error);
    JsonOutput {
        command: command.to_owned(),
        dry_run,
        provider: provider.to_owned(),
        model: model.to_owned(),
        message: None,
        commits: None,
        split_plan: None,
        error: Some(JsonError {
            code,
            message: error.to_string(),
            retryable,
            attempts: 1,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_subject_and_body() {
        let (subject, body) = split_message("feat: add json\n\nBody line 1\nBody line 2\n");
        assert_eq!(subject, "feat: add json");
        assert_eq!(body.as_deref(), Some("Body line 1\nBody line 2"));
    }

    #[test]
    fn single_line_message_has_no_body() {
        let (subject, body) = split_message("feat: one liner");
        assert_eq!(subject, "feat: one liner");
        assert_eq!(body, None);
    }

    #[test]
    fn maps_known_error_codes() {
        let error = anyhow::Error::new(crate::errors::AicError::RateLimited("openai".to_owned()));
        assert_eq!(error_code(&error), ("rate_limited".to_owned(), true));

        let error = anyhow::Error::new(crate::errors::AicError::NoChanges);
        assert_eq!(error_code(&error), ("no_changes".to_owned(), false));
    }

    #[test]
    fn maps_abort_message() {
        let error = anyhow::anyhow!("commit aborted");
        assert_eq!(error_code(&error), ("aborted".to_owned(), false));
    }

    #[test]
    fn json_output_is_single_line() {
        let output = JsonOutput {
            command: "commit".to_owned(),
            dry_run: true,
            provider: "test".to_owned(),
            model: "test-model".to_owned(),
            message: Some("feat: multi\nline\nmessage".to_owned()),
            commits: Some(vec![]),
            split_plan: None,
            error: None,
        };
        let line = serde_json::to_string(&output).unwrap();
        assert!(!line.contains('\n'));
        assert!(!line.contains('\x1b'));
    }
}
