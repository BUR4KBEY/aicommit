# Usage

Generate a commit message from staged files:

```sh
git add <files>
aic
```

In an interactive terminal, if files are already staged, `aic` first shows a lightweight preflight where you can continue, unstage selected files, or abort.
If no files are staged, `aic` shows a menu that lets you stage all changed files, choose files interactively, or cancel.
When the staged change set only contains filtered or binary files such as images, SVGs, or lockfiles, `aic` falls back to Git change metadata so it can still draft a cautious high-level commit message without reading file contents.

When at least two files are staged in the normal interactive flow, `aic` can also split the change set into multiple file-group commits. It first suggests commit groups with AI, then lets you accept the groups, rebuild them manually, or fall back to one commit.

Once a message is generated you can accept it, regenerate, or edit it; `Edit` opens your `$EDITOR` with the message preloaded. After a successful push, `aic` prints a one-line confirmation (`Pushed main → origin`) and a final `Commit created` summary with the short hash, branch, remote, and a `+added −removed` diffstat. In terminals that support hyperlinks, the hash links straight to the commit page on GitHub, GitLab, or Bitbucket.

Skip the commit prompts and auto-stage all changed files. If push-after-commit is enabled and exactly one remote exists, `aic --yes` also pushes automatically:

```sh
aic --yes
```

If multiple remotes are configured, `aic --yes` stops with a clear error instead of guessing where to push.

When push-after-commit is enabled and the current branch tracks an upstream, `aic` fetches that upstream before it starts the commit session (with a "Checking branch sync" spinner while the fetch runs). If the branch is behind or has diverged, `aic` stops before creating a new commit and shows recovery guidance so you can sync safely first.

Add extra context for the generated message:

```sh
aic --context "include issue 123"
```

Use the full GitMoji prompt:

```sh
aic --fgm
```

Generate and display the message without committing:

```sh
aic --dry-run
```

Regenerate and amend the last commit message:

```sh
aic --amend
```

Pass Git commit flags through:

```sh
aic --no-verify
```

Use a message template:

```sh
aic "issue-123: $msg"
```

The placeholder defaults to `$msg` and can be changed with `AIC_MESSAGE_TEMPLATE_PLACEHOLDER`.

Tune the system prompt without recompiling by setting `AIC_PROMPT_FILE` to a custom prompt-template path.

If a push is rejected after the commit has already been created locally, `aic` now treats that as a recovery flow instead of dumping raw Git output only. In interactive clean-tree cases it can offer a `pull --rebase` retry; otherwise it explains what happened and leaves the commit in your local history for you to push after syncing manually.

## Interactive Diff Splitting

Use the normal commit flow and choose `Split into multiple commits` when prompted:

```sh
git add <files>
aic
```

`aic` will:

- analyze the staged change set and suggest 2-4 commit groups
- let you keep the suggested groups or build them manually
- generate one commit message per group
- preview the whole commit sequence before creating anything

Limitations in v1:

- splitting is file-based, not hunk-based
- split flow is only offered in the normal interactive `aic` path
- split flow is skipped when a staged file also has unstaged changes
- `--yes`, `--dry-run`, and `--amend` stay single-commit

## Command Help

Use the built-in help when you want a quick command reference from the terminal:

```sh
aic --help
aic config --help
aic hook --help
aic review --help
```

Top-level commands, nested subcommands, and important positional arguments now include descriptions, so the CLI can be used as a quick reference without jumping back to the docs.

## Diff Review

Get AI-powered feedback on staged changes before committing:

```sh
aic review
```

Optionally focus the review on a particular concern:

```sh
aic review --context "focus on security"
```

Findings are grouped by severity (Critical, Warning, Suggestion) and cover bugs, security, performance, correctness, and readability. Large diffs are automatically chunked and synthesized into a single review — the same chunking applies to `aic` commit generation and `aic pr` drafts, with the spinner reporting per-chunk progress and elapsed time while the requests run.

If files are already staged, `aic review` analyzes that staged set directly. In an interactive terminal, if nothing is staged, it reuses the staging menu so you can stage all changes or pick files first, but it keeps the review flow read-only once files are staged.

## Pull Request Drafts

Generate a pull request title and Markdown description from the current branch:

```sh
aic pr
aic pr --base origin/main
aic pr --context "call out migration risk"
aic pr --yes
```

`aic pr` compares `HEAD` against a base ref, uses the branch commits plus cumulative diff as context, and prints a copy-ready title and description. If you do not pass `--base`, `aic` tries `refs/remotes/origin/HEAD`, then `origin/main`, `origin/master`, `main`, and `master`.

## Commit History

Every generated commit message, PR draft, and review is saved to `~/.aicommit-history.json`. Browse recent entries:

```sh
aic history
aic history --non-interactive
aic history -n 5
aic history --all
aic history --verbose
```

When `aic history` is run in a terminal, it opens the interactive picker by default. Use `--non-interactive` for the plain text summary view.

Filter by kind:

```sh
aic history --kind commit
aic history --kind pr
aic history --kind review
```

## Rewrite Commit Messages

Clean up the last N commit messages on your branch using AI before opening a PR:

```sh
aic log
aic log -n 3
aic log -n 5 --yes
```

This generates new messages for each commit, shows a before/after comparison, and rewrites them via `git rebase` on confirmation.

Requirements:
- The working tree must be clean (no uncommitted changes).
- The range must not contain merge commits.
- This rewrites git history - do not use on commits that have been pushed to a shared branch.

## Visualization

Generate SVG visualizations of the codebase and commit history:

```sh
aic map tree                     # file hierarchy treemap
aic map history                  # zigzag commit timeline
aic map heat                     # file modification heatmap
aic map activity                 # GitHub-style contribution grid
```

Control the commit range and output path:

```sh
aic map history -n 30 -o timeline.svg
aic map heat -n 200
aic map activity -n 1000
```

All subcommands produce standalone SVG files. See [Visualization](map.md) for full flag reference.

## Scope Hints

When conventional commit scopes are enabled (the default), `aic` detects likely scopes from the staged file paths and suggests them to the AI. For example, changes to `src/ai/` hint at the scope `ai`, while `Cargo.toml` hints at `deps`. This improves scope consistency without requiring manual input. Disable scopes entirely with `AIC_OMIT_SCOPE=true`.

## Branch Name Context

`aic` automatically detects ticket or issue references in the current branch name and feeds them into the prompt. For example, on a branch named `feature/PROJ-123-add-auth`, the generated message may reference `PROJ-123`. Both JIRA-style (`PROJ-123`) and GitHub-style (`#456`) patterns are recognised.

## Shell Completions

Generate shell completions:

```sh
aic completions bash
aic completions zsh
aic completions fish
```

Shell completion descriptions are generated from the same metadata as `aic --help`, so command wording stays consistent across both surfaces.
