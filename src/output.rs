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
    let error = inner_json_error(error);
    if let Some(aic) = error.downcast_ref::<crate::errors::AicError>() {
        return match aic {
            crate::errors::AicError::NoChanges => ("no_changes".to_owned(), false),
            crate::errors::AicError::NotGitRepository => ("not_git_repo".to_owned(), false),
            crate::errors::AicError::NotTty => ("not_a_tty".to_owned(), false),
            crate::errors::AicError::Aborted => ("aborted".to_owned(), false),
            crate::errors::AicError::CommitCreatedPushFailed(_) => {
                ("push_failed".to_owned(), false)
            }
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

    let message = error.to_string();
    let lower = message.to_lowercase();
    // Keep provider-call wrappers retryable: anyhow joins the outer context
    // ("failed to call AI provider") with the normalized provider error
    // below, so match on the full chain.
    if lower.contains("failed to call ai provider")
        && (lower.contains("rate limit")
            || lower.contains("too many requests")
            || lower.contains("service unavailable"))
    {
        return ("service_unavailable".to_owned(), true);
    }
    // The commit happened; the push did not. Mirrors `CommitCreatedPushFailed`.
    if lower.contains("commit was created locally")
        || lower.contains("still failed after rebasing")
        || lower.contains("rebase stopped with conflicts")
        || lower.contains("rebase recovery did not complete")
    {
        return ("push_failed".to_owned(), false);
    }
    if lower.contains("failed to call ai provider") || lower.contains("failed to parse ai response")
    {
        return ("provider_error".to_owned(), false);
    }
    if lower.contains("failed to parse split plan")
        || lower.contains("split plan must contain")
        || lower.contains("unparseable")
    {
        return ("invalid_split_plan".to_owned(), false);
    }
    if message.contains("aborted")
        || message.contains("cancelled")
        || lower.contains("aborted")
        || lower.contains("cancelled")
    {
        ("aborted".to_owned(), false)
    } else if lower.contains("no files staged") || lower.contains("no changes") {
        ("no_changes".to_owned(), false)
    } else if lower.contains("not a git repository") {
        ("not_git_repo".to_owned(), false)
    } else if lower.contains("requires a tty")
        || lower.contains("not a tty")
        || lower.contains("input device is not a tty")
    {
        ("not_a_tty".to_owned(), false)
    } else if lower.contains("mutually exclusive") || lower.contains("requires -y") {
        ("invalid_args".to_owned(), false)
    } else if lower.contains("cannot auto-push")
        || lower.contains("machine mode cannot")
        || lower.contains("no files in the last commit")
        || lower.contains("behind its upstream")
        || lower.contains("has diverged")
    {
        ("misuse".to_owned(), false)
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

/// Marker so `main` skips its `Error:` line: `--json` flows already printed
/// the machine-readable error object to stdout. The wrapper preserves
/// downcasting for exit-code classification.
#[derive(Debug)]
struct JsonErrorEmitted(anyhow::Error);

impl std::fmt::Display for JsonErrorEmitted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for JsonErrorEmitted {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.0.as_ref())
    }
}

/// Wrap an error after its `--json` envelope was emitted to stdout.
#[must_use]
pub fn mark_json_error_emitted(error: anyhow::Error) -> anyhow::Error {
    anyhow::Error::new(JsonErrorEmitted(error))
}

/// True when a `--json` envelope was already printed for this error, so
/// `main` must only set the exit code without duplicating stderr.
#[must_use]
pub fn json_error_already_emitted(error: &anyhow::Error) -> bool {
    error.is::<JsonErrorEmitted>()
}

/// Peel the [`mark_json_error_emitted`] wrapper so exit-code and `error.code`
/// classifiers see the original payload.
#[must_use]
pub fn inner_json_error(error: &anyhow::Error) -> &anyhow::Error {
    match error.downcast_ref::<JsonErrorEmitted>() {
        Some(JsonErrorEmitted(inner)) => inner,
        None => error,
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
    fn maps_not_a_tty_and_invalid_args() {
        let error = anyhow::Error::new(crate::errors::AicError::NotTty);
        assert_eq!(error_code(&error), ("not_a_tty".to_owned(), false));

        let error = anyhow::anyhow!("The input device is not a TTY");
        assert_eq!(error_code(&error), ("not_a_tty".to_owned(), false));

        let error = anyhow::anyhow!("--json and --quiet are mutually exclusive");
        assert_eq!(error_code(&error), ("invalid_args".to_owned(), false));
    }

    #[test]
    fn maps_push_failed_and_invalid_split_plan() {
        let error = anyhow::Error::new(crate::errors::AicError::CommitCreatedPushFailed(
            "rejected".to_owned(),
        ));
        assert_eq!(error_code(&error), ("push_failed".to_owned(), false));

        let error = anyhow::anyhow!("commit was created locally, but the push was rejected");
        assert_eq!(error_code(&error), ("push_failed".to_owned(), false));

        let error = anyhow::anyhow!("failed to parse split plan JSON: EOF");
        assert_eq!(error_code(&error), ("invalid_split_plan".to_owned(), false));
    }

    #[test]
    fn maps_provider_call_wrapper() {
        let error = anyhow::anyhow!("failed to call AI provider: rate limit exceeded");
        assert_eq!(error_code(&error), ("service_unavailable".to_owned(), true));

        let error = anyhow::anyhow!("failed to parse AI response: EOF");
        assert_eq!(error_code(&error), ("provider_error".to_owned(), false));
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
