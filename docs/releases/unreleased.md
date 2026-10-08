# aicommit Unreleased

## Machine-Readable Output (`--json` / `--quiet`)

- New top-level `--json` flag for the `commit`, `review`, and `pr` flows: stdout holds exactly one JSON object (`command`, `dry_run`, `provider`, `model`, optional `message` / `commits` / `split_plan` / `error`), with all progress and TUI output suppressed and diagnostics routed to stderr. On failure the exit code is nonzero and stdout holds one JSON `error` object (`code`, `message`, `retryable`, `attempts`).
- New top-level `--quiet` flag: prints only the decisive line(s) — the message for dry-runs, or `<hash> <subject>` per commit — and implies non-interactive behavior like `-y`.
- `--json` requires `-y` for real commits (dry-run excepted); `--json` and `--quiet` are mutually exclusive. Machine runs are single-commit and never prompt. Human output without either flag is byte-identical to before.

## Exit-Code Taxonomy and No-TTY Hint

- `aic` now exits `0` on success, `1` on runtime/provider failure, and `2` on misuse or non-actionable environment (no changes, not a git repository, stdin not a TTY, bad flags/config, user abort). `aic` never exits `0` with the commit undone and never exits `1` with the commit created; see `src/exit.rs` and the new "Exit Codes" table in `docs/usage.md`.
- Whenever a prompt would run without a TTY on stdin, `aic` prints one actionable line to stderr (`aic: interactive mode requires a TTY — use "aic -y" for non-interactive commit (-y auto-stages), or "aic -d" to preview`), keeps stdout empty, and exits `2`. Applies to the staged-file, commit-mode (split), message-accept, and staging menus, interactive history, `aic setup`, `aic pr`, and `aic log` confirmations.
- `--json` `error.code` values mirror the taxonomy: new `not_a_tty`, `aborted`, `invalid_args`, `invalid_split_plan`, `provider_error`, `push_failed`, and `misuse` codes sit alongside the existing provider/config codes.
