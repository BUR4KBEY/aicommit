# aicommit Unreleased

## Machine-Readable Output (`--json` / `--quiet`)

- New top-level `--json` flag for the `commit`, `review`, and `pr` flows: stdout holds exactly one JSON object (`command`, `dry_run`, `provider`, `model`, optional `message` / `commits` / `split_plan` / `error`), with all progress and TUI output suppressed and diagnostics routed to stderr. On failure the exit code is nonzero and stdout holds one JSON `error` object (`code`, `message`, `retryable`, `attempts`).
- New top-level `--quiet` flag: prints only the decisive line(s) — the message for dry-runs, or `<hash> <subject>` per commit — and implies non-interactive behavior like `-y`.
- `--json` requires `-y` for real commits (dry-run excepted); `--json` and `--quiet` are mutually exclusive. Machine runs are single-commit and never prompt. Human output without either flag is byte-identical to before.

## Non-Interactive Split (`--split auto`)

- New top-level `--split auto|off` flag (default `off`; zero behavior change): `aic -y --split auto` runs the full split pipeline without prompts — split plan, one message per group, commits in plan order — while `aic -d --split auto` prints the plan without committing. With `--json`, the plan is surfaced in `"split_plan": [...]` alongside one `commits` entry per group.
- Consent guardrails: fewer than 2 usable groups, more groups than `AIC_SPLIT_MAX` (new config key, default `10`, minimum `2`), partially-staged files, single-file sets, `--amend`, and metadata-only inputs all fall back to the single-commit path with a stderr warning. `--split auto` requires `-y` (or `-d`) and never prompts. The interactive split picker is unchanged.
