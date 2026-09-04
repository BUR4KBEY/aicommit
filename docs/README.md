# Documentation

This folder is the detailed documentation entry point for `aic`, the Rust CLI for generating Git commit messages, PR drafts, and reviews with AI.

## Start Here

- [Installation](installation.md): install `aic` with Homebrew, WinGet, direct binaries, or from source.
- [Usage](usage.md): run the commit-message workflow and pass Git flags through.
- [Configuration](configuration.md): set provider, model, prompt, token, hook, and output behavior.
- [Providers](providers.md): choose between OpenAI, Azure OpenAI, Anthropic, Groq, Ollama, Claude Code, Codex, and GitHub Copilot CLI.
- [Hooks](hooks.md): install or remove the Git `prepare-commit-msg` hook.
- [Visualization](map.md): generate SVG treemaps, timelines, heatmaps, and activity graphs.
- [Architecture](architecture.md): understand the Rust modules and data flow.
- [Testing](testing.md): run the verification suite.
- [Roadmap](roadmap.md): see deferred v1 items.
- [Release Notes - Unreleased](releases/unreleased.md): upcoming changes not yet shipped in a tagged release.
- [Release Notes - 0.0.9](releases/0.0.9.md): latest release notes.
- [Release Notes - 0.0.8](releases/0.0.8.md): previous release notes.
- [Release Notes - 0.0.7](releases/0.0.7.md): previous release notes.
- [Release Notes - 0.0.6](releases/0.0.6.md): previous release notes.
- [Release Notes - 0.0.5](releases/0.0.5.md): previous release notes.
- [Release Notes - 0.0.4](releases/0.0.4.md): previous release notes.
- [Release Notes - 0.0.3](releases/0.0.3.md): previous release notes.
- [Release Notes - 0.0.2](releases/0.0.2.md): previous release notes.
- [Release Notes - 0.0.1](releases/0.0.1.md): initial release notes.

## Workflow Map

```mermaid
flowchart TD
    A["Install aic"] --> B["Run aic setup"]
    B --> C{"Which command?"}
    C -->|aic| D["Stage files with git add (optional)"]
    C -->|aic map| MAP["Choose visualization"]
    C -->|aic review| REV["AI diff review"]
    C -->|aic log| LOG["Rewrite commit messages"]
    C -->|aic pr| PR["Draft PR title + description"]
    MAP --> TREE["aic map tree"]
    MAP --> HIST["aic map history"]
    MAP --> HEAT["aic map heat"]
    MAP --> ACT["aic map activity"]
    TREE --> SVG["SVG output"]
    HIST --> SVG
    HEAT --> SVG
    ACT --> SVG
    D --> STAGE{"Anything staged?"}
    STAGE -->|No| PICK["Stage all, choose files, or cancel"]
    STAGE -->|Yes| SPLIT{"Split into multiple commits?"}
    PICK --> SPLIT
    SPLIT -->|No| E["Review generated message"]
    SPLIT -->|Yes| SG["Preview split groups + messages"]
    SG --> GC["Create grouped commits"]
    E --> DRY{"--dry-run?"}
    DRY -->|Yes| PRINT["Print message and exit"]
    DRY -->|No| F{"Accept message?"}
    F -->|Yes| G["Create commit"]
    F -->|No| H["Regenerate, edit, or abort"]
    G --> I{"Push enabled?"}
    I -->|Yes| J["git push"]
    I -->|No| K["Done"]
    GC --> I
    AMEND["Run aic --amend"] --> E
```
