//! Process exit codes for `aic`.
//!
//! Taxonomy (documented in `docs/usage.md`):
//!
//! | Code | Meaning | Examples |
//! | ---- | ------- | -------- |
//! | `0` | success | commit created (or deliberately fell back from split plan to one commit), dry-run message printed, review/PR text printed |
//! | `1` | runtime failure | provider down after retries exhausted, push rejected after commit creation, model bugs (unparseable plan) |
//! | `2` | misuse / non-actionable environment | no changes detected, not a git repository, stdin is not a TTY, bad flags/config, user abort |
//!
//! Invariants:
//!
//! - never exit `0` with the commit undone;
//! - never exit `1` with the commit created.

pub const SUCCESS: i32 = 0;
pub const RUNTIME_FAILURE: i32 = 1;
pub const MISUSE: i32 = 2;

/// Hint printed to stderr whenever a prompt would run but stdin is not a TTY.
pub const NO_TTY_HINT: &str = "aic: interactive mode requires a TTY \u{2014} use \"aic -y\" for non-interactive commit (-y auto-stages), or \"aic -d\" to preview";

/// Map a top-level error to its process exit code.
pub fn code_for(error: &anyhow::Error) -> i32 {
    let error = crate::output::inner_json_error(error);
    if let Some(aic) = error.downcast_ref::<crate::errors::AicError>() {
        return match aic {
            crate::errors::AicError::NoChanges
            | crate::errors::AicError::NotGitRepository
            | crate::errors::AicError::NotTty
            | crate::errors::AicError::Aborted
            | crate::errors::AicError::UnsupportedConfigKey(_)
            | crate::errors::AicError::InvalidConfigValue { .. }
            | crate::errors::AicError::MissingApiKey(_)
            | crate::errors::AicError::ModelNotFound { .. }
            | crate::errors::AicError::TooManyTokens => MISUSE,
            crate::errors::AicError::Authentication(_)
            | crate::errors::AicError::RateLimited(_)
            | crate::errors::AicError::InsufficientCredits(_)
            | crate::errors::AicError::ServiceUnavailable(_)
            | crate::errors::AicError::EmptyMessage
            | crate::errors::AicError::CommitCreatedPushFailed(_) => RUNTIME_FAILURE,
        };
    }

    code_for_message(&error.to_string())
}

/// Classify errors that surface as plain `anyhow!` messages (no [`AicError`]
/// payload). Public so `--json` mapping can share one classifier.
pub fn code_for_message(message: &str) -> i32 {
    let lower = message.to_lowercase();
    if is_misuse_message(&lower) {
        MISUSE
    } else {
        RUNTIME_FAILURE
    }
}

fn is_misuse_message(lower: &str) -> bool {
    const MARKERS: &[&str] = &[
        "aborted",
        "abort",
        "cancelled",
        "canceled",
        "invalid",
        "usage:",
        "mutually exclusive",
        "requires -y",
        "no files staged",
        "no files selected",
        "no changes",
        "no commit context",
        "no diff",
        "no commits",
        "not a git repository",
        "no files in the last commit",
        "unknown config key",
        "unsupported config key",
        "unsupported provider",
        "not supported",
        "requires a tty",
        "not a tty",
        "no tty",
        "non-interactive",
        "cannot auto-push",
        "machine mode cannot",
        "behind its upstream",
        "has diverged",
        "uncommitted changes",
        "merge commits",
        "existing ref to --base",
        "determine a base branch",
        "no file changes found",
        "hookrun",
        "not managed by aic",
        "prompt is too long",
        "too many tokens",
        "context length",
        "context window",
    ];
    MARKERS.iter().any(|marker| lower.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_tty_hint_is_exact() {
        assert_eq!(
            NO_TTY_HINT,
            "aic: interactive mode requires a TTY \u{2014} use \"aic -y\" for non-interactive commit (-y auto-stages), or \"aic -d\" to preview"
        );
    }

    #[test]
    fn benign_environment_failures_exit_2() {
        for message in [
            "commit aborted",
            "no changes detected",
            "no files staged",
            "not a git repository",
            "branch is behind its upstream",
            "cannot auto-push with --yes because multiple remotes are configured",
            "--json and --quiet are mutually exclusive",
            "The input device is not a TTY",
            "interactive mode requires a TTY",
        ] {
            assert_eq!(code_for_message(message), MISUSE, "message: {message}");
        }
    }

    #[test]
    fn runtime_failures_exit_1() {
        for message in [
            "failed to call AI provider",
            "connection reset by peer",
            "commit was created locally, but the push was rejected",
            "rebase stopped with conflicts",
            "failed to parse split plan JSON",
        ] {
            assert_eq!(
                code_for_message(message),
                RUNTIME_FAILURE,
                "message: {message}"
            );
        }
    }
}
