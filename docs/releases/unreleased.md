# aicommit Unreleased

## Machine-Readable Output (`--json` / `--quiet`)

- New top-level `--json` flag for the `commit`, `review`, and `pr` flows: stdout holds exactly one JSON object (`command`, `dry_run`, `provider`, `model`, optional `message` / `commits` / `split_plan` / `error`), with all progress and TUI output suppressed and diagnostics routed to stderr. On failure the exit code is nonzero and stdout holds one JSON `error` object (`code`, `message`, `retryable`, `attempts`).
- New top-level `--quiet` flag: prints only the decisive line(s) — the message for dry-runs, or `<hash> <subject>` per commit — and implies non-interactive behavior like `-y`.
- `--json` requires `-y` for real commits (dry-run excepted); `--json` and `--quiet` are mutually exclusive. Machine runs are single-commit and never prompt. Human output without either flag is byte-identical to before.
