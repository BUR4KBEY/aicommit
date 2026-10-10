# aicommit Unreleased

## Machine-Readable Output (`--json` / `--quiet`)

- New top-level `--json` flag for the `commit`, `review`, and `pr` flows: stdout holds exactly one JSON object (`command`, `dry_run`, `provider`, `model`, optional `message` / `commits` / `split_plan` / `error`), with all progress and TUI output suppressed and diagnostics routed to stderr. On failure the exit code is nonzero and stdout holds one JSON `error` object (`code`, `message`, `retryable`, `attempts`).
- New top-level `--quiet` flag: prints only the decisive line(s) — the message for dry-runs, or `<hash> <subject>` per commit — and implies non-interactive behavior like `-y`.
- `--json` requires `-y` for real commits (dry-run excepted); `--json` and `--quiet` are mutually exclusive. Machine runs are single-commit and never prompt. Human output without either flag is byte-identical to before.

## Exit-Code Taxonomy and No-TTY Hint

- `aic` now exits `0` on success, `1` on runtime/provider failure, and `2` on misuse or non-actionable environment (no changes, not a git repository, stdin not a TTY, bad flags/config, user abort). `aic` never exits `0` with the commit undone and never exits `1` with the commit created; see `src/exit.rs` and the new "Exit Codes" table in `docs/usage.md`.
- Whenever a prompt would run without a TTY on stdin, `aic` prints one actionable line to stderr (`aic: interactive mode requires a TTY — use "aic -y" for non-interactive commit (-y auto-stages), or "aic -d" to preview`), keeps stdout empty, and exits `2`. Applies to the staged-file, commit-mode (split), message-accept, and staging menus, interactive history, `aic setup`, `aic pr`, and `aic log` confirmations.
- `--json` `error.code` values mirror the taxonomy: new `not_a_tty`, `aborted`, `invalid_args`, `invalid_split_plan`, `provider_error`, `push_failed`, and `misuse` codes sit alongside the existing provider/config codes.

## Non-Interactive Split (`--split auto`)

- New top-level `--split auto|off` flag (default `off`; zero behavior change): `aic -y --split auto` runs the full split pipeline without prompts — split plan, one message per group, commits in plan order — while `aic -d --split auto` prints the plan without committing. With `--json`, the plan is surfaced in `"split_plan": [...]` alongside one `commits` entry per group.
- Consent guardrails: fewer than 2 usable groups, more groups than `AIC_SPLIT_MAX` (new config key, default `10`, minimum `2`), partially-staged files, single-file sets, `--amend`, and metadata-only inputs all fall back to the single-commit path with a stderr warning. `--split auto` requires `-y` (or `-d`) and never prompts. The interactive split picker is unchanged.

## Split-Plan Robustness and Group Failure Semantics

- Split-plan requests no longer inherit the commit-sized output cap: they ask for at least `4096` output tokens (bounded by what `AIC_TOKENS_MAX_INPUT` leaves for output). When the provider reports it stopped on the token cap mid-JSON, `aic` retries the plan once with a 4x cap instead of falling back to a single commit.
- A plan that stays truncated now degrades to one commit with an actionable stderr warning (`the split plan response was truncated at the N-token plan cap; raise AIC_TOKENS_MAX_OUTPUT ...`) rather than a bare `EOF while parsing a string`.
- Per-group commit messages are generated for every group before any commit is created (unchanged atomicity) and each group message is now attempted up to 4 times. After the retries are exhausted, the interactive split flow offers `Retry failed group` / `Commit remaining groups` / `Abort` instead of aborting the whole invocation, keeping the messages it already generated.
- Non-interactive splits (`-y --split auto`, `--json`) never partial-commit: they exit nonzero with one line naming the group, the stage, and the attempts, e.g. `group 2 of 3: message generation failed after 4 attempts; nothing committed: ...`. The `--json` envelope reports the same as `error.code: "split_stage_failed"` with the real `attempts`.

## OpenCode Go Provider (`opencode-go`)

- New `opencode-go` provider for the OpenCode Go gateway (`https://opencode.ai/zen/go/v1`), a hosted catalog of open coding models. It defaults to `glm-5.3-flash`, requires `AIC_API_KEY`, and uses the standard chat-completions flow; `aic models --provider opencode-go` lists the live catalog.
- `aic` now sends the `x-opencode-session` header OpenCode Go requires, generated as a fresh UUID once per process. Requests without it fail with `MissingSessionID`, so wrapping `aic` to inject `AIC_API_CUSTOM_HEADERS='{"x-opencode-session":"..."}'` is no longer required. Setting `x-opencode-session` in `AIC_API_CUSTOM_HEADERS` yourself still wins.
- The id is per-process on purpose: a fixed id lets the gateway serve stale cached responses for repeated prompts, which shows up as `AI provider returned an empty response` on identical back-to-back runs. Back-to-back `aic` invocations therefore always use fresh ids.
- The header is also added when `AIC_API_URL` points at `opencode.ai/zen/go` under any provider id, so existing `openai` + `AIC_API_URL` setups get it too. It is deliberately scoped to the Go path: the sibling OpenCode Zen gateway (`/zen/v1`) is a separate pay-per-token product and does not document the header. No other provider's requests change.
- Only OpenCode Go models served by `/chat/completions` work; the rest of the catalog needs the `/responses` or `/messages` wire formats, which `aic` does not implement.
