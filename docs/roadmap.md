# Roadmap

The v1 priority is a reliable Rust CLI for local commit generation through `aic`.

## High Impact

- ~~**`aic log` - rewrite past commit messages**~~ - ✅ clean up the last N commit messages on a branch using AI before opening a PR.
- ~~**`aic review` - AI-powered diff review**~~ - ✅ get feedback on the staged diff (bugs, style, security) using a review-focused prompt and the existing provider infrastructure.
- ~~**Shell completion generation**~~ - ✅ `aic completions <shell>` subcommand added via `clap_complete`.

## Medium Impact, Low Effort

- ~~**`--dry-run` flag**~~ - ✅ generate and display the commit message without committing.
- ~~**`--amend` flag**~~ - ✅ regenerate and amend the last commit message.
- ~~**Branch name as context**~~ - ✅ ticket/issue numbers extracted from branch names and fed into the prompt.
- ~~**Conventional commit scope hints**~~ - ✅ detect likely scopes from changed file paths and feed them into the prompt to improve scope consistency.
- ~~**Commit message history**~~ - ✅ local log of generated messages and reviews (`~/.aicommit-history.json`) with `aic history` to browse them.
- ~~**CLI help coverage**~~ - ✅ richer top-level and nested `--help` output, with shared metadata for help text, shell completions, and `aic config describe`.

## Medium Impact, Moderate Effort

- ~~**`aic pr` - PR description generation**~~ - ✅ generate a pull request title and description from the branch's commits and cumulative diff against the base branch.
- ~~**Interactive diff splitting**~~ - ✅ analyze one staged change set, suggest file-based commit groups, preview one message per group, and create multiple commits sequentially.
- **Config profiles** - named profiles (`~/.aicommit.d/work.toml`, `~/.aicommit.d/personal.toml`) selectable via `--profile` or auto-detected from the git remote URL.
- Commitlint-style config inference.

## Visualization - `aic map`

- ~~**Codebase treemap** (`aic map tree`)~~ - ✅ squarified treemap of the file hierarchy, sized by line count and coloured by directory.
- ~~**Commit timeline** (`aic map history`)~~ - ✅ vertical zigzag commit timeline with date circles, full commit messages, and file-change dots.
- ~~**Change heatmap** (`aic map heat`)~~ - ✅ horizontal bar chart of files ranked by modification frequency with a cold-to-hot colour scale.
- ~~**Activity graph** (`aic map activity`)~~ - ✅ GitHub-style contribution grid generated from commit timestamps.

## Quality of Life

- **`aic init`** - generate a `.aicommitignore` with sensible defaults for the detected project type.
- **Cost estimation** - show estimated token usage and approximate cost before sending to the API.
- **Clipboard copy option** - add "Copy to clipboard" alongside Yes/No/Edit in the confirmation prompt.
- ~~**`--quiet` / `--json` output mode**~~ - ✅ output only the generated message (quiet) or a single-line JSON envelope (json) on stdout for piping into other tools; see `docs/usage.md#machine-readable-output`.
- Broader language prompt examples.
- Release packaging for common package managers.
