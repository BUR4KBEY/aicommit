# aicommit Unreleased

## Changed

- Terminal output polish across the commit, review, and PR flows: one session header per run (previously "Commit session" printed twice), the staged-file list renders exactly once, the branch/provider metadata collapses into a single row, and the redundant "Sending to provider/model" line is gone.
- Interactive prompts now share aic's glyph vocabulary (`?` pending, `✔` answered, `❯` highlighted option) via a global prompt theme, and sections/cards manage their own vertical spacing so the transcript has a consistent rhythm.
- Push output is quieter and less repetitive: the push prompt shows a short remote label (`[GitHub] origin`) instead of the full URL, success prints `Pushed main → origin`, and the final `Commit created` summary shows the short hash, branch, `pushed to origin (GitHub)`, and a `+added −removed` diffstat. In terminals with OSC-8 hyperlink support the hash links to the commit page on GitHub, GitLab, or Bitbucket (`commit_path` in `git_hosts.toml`).
- Raw git output is no longer re-echoed after successful commits and pushes; it still appears on failures and recovery flows.
- Choosing `Edit` on a generated commit message now opens `$EDITOR` with the message preloaded (matching the split flow) instead of a single-line inline prompt.
- The pre-commit upstream fetch shows a spinner instead of stalling silently, and `aic models` marks the active model with `❯` in the shared list style.

## Fixed

- The generated PR preview cards no longer render twice when accepting or editing a draft.
- `aic log` now reports "Rewrote 1 commit message" (singular) and aligns the before/after subjects in the proposed-changes comparison.

- Commit generation no longer appears to hang on very large staged diffs: the token counter now builds its encoder once per process instead of on every call, and diff splitting runs in a single pass (previously minutes to hours of CPU on multi-MB diffs; now seconds).
- Local CLI providers (`claude-code`, `codex`, `copilot`) no longer deadlock when the prompt exceeds the OS pipe buffer (~64 KiB): the prompt is now written to the child process from a dedicated thread while its output is drained concurrently.
- Provider responses rejecting an oversized prompt ("maximum context length", "prompt is too long", HTTP 413) are now reported as the diff-too-large error with remediation hints, instead of misleading "service unavailable" or "model is not available" messages.
- Chunked commit generation reserves budget for the chunk-summary preamble, so tightly-packed chunks no longer trip the provider token guard.

## Added

- `AIC_HTTP_TIMEOUT` config key: per-request timeout in seconds for the HTTP providers (default `120`, `0` disables), with a 10s connection timeout. A timed-out request now fails with a clear message instead of waiting forever.
- Live status feedback during generation: the spinner now shows the current stage (`Splitting diff`, `Summarizing chunk 2/4`, `Synthesizing commit message`), a rotating status detail, and elapsed time. Large diffs print an upfront note with the chunk count, split-commit drafting shows per-commit progress, and the `prepare-commit-msg` hook announces itself on stderr.
- The rotating status lines ship in `src/status_messages.toml` (including an occasional `[rare]` interjection) and can be personalized per user via `~/.aicommit-status.toml`; each run shuffles the rotation order so the lines stay fresh.
