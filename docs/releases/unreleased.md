# aicommit Unreleased

## Fixed

- Commit generation no longer appears to hang on very large staged diffs: the token counter now builds its encoder once per process instead of on every call, and diff splitting runs in a single pass (previously minutes to hours of CPU on multi-MB diffs; now seconds).
- Local CLI providers (`claude-code`, `codex`, `copilot`) no longer deadlock when the prompt exceeds the OS pipe buffer (~64 KiB): the prompt is now written to the child process from a dedicated thread while its output is drained concurrently.
- Provider responses rejecting an oversized prompt ("maximum context length", "prompt is too long", HTTP 413) are now reported as the diff-too-large error with remediation hints, instead of misleading "service unavailable" or "model is not available" messages.
- Chunked commit generation reserves budget for the chunk-summary preamble, so tightly-packed chunks no longer trip the provider token guard.

## Added

- `AIC_HTTP_TIMEOUT` config key: per-request timeout in seconds for the HTTP providers (default `120`, `0` disables), with a 10s connection timeout. A timed-out request now fails with a clear message instead of waiting forever.
- Live status feedback during generation: the spinner now shows the current stage (`Splitting diff`, `Summarizing chunk 2/4`, `Synthesizing commit message`), a rotating status detail, and elapsed time. Large diffs print an upfront note with the chunk count, split-commit drafting shows per-commit progress, and the `prepare-commit-msg` hook announces itself on stderr.
- The rotating status lines ship in `src/status_messages.toml` (including an occasional `[rare]` interjection) and can be personalized per user via `~/.aicommit-status.toml`; each run shuffles the rotation order so the lines stay fresh.
