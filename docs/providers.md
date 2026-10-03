# Providers

V1 ships with these provider paths:

```text
openai
azure-openai
anthropic
groq
ollama
claude-code
codex
copilot
apple
```

`openai`, `azure-openai`, `groq`, and `ollama` use the OpenAI chat-completions wire format.

`anthropic` uses Anthropic's Messages API directly.

`claude-code`, `codex`, and `copilot` are experimental local-binary providers. They use the installed `claude`, `codex`, and `copilot` CLIs from your `PATH`, so authentication is managed by those tools rather than `aic`.

`apple` is an experimental local-binary provider for Apple's on-device Foundation Model. It runs the `fm respond` CLI that ships with macOS, so nothing leaves the machine and no API key is needed.

```mermaid
flowchart TD
    Config["AIC_AI_PROVIDER"] --> Provider{"Provider"}
    Provider -->|openai| OpenAI["OpenAI API"]
    Provider -->|azure-openai| Azure["Azure OpenAI v1 API"]
    Provider -->|anthropic| Anthropic["Anthropic Messages API"]
    Provider -->|groq| Groq["Groq OpenAI-compatible API"]
    Provider -->|ollama| Ollama["Local Ollama OpenAI-compatible API"]
    Provider -->|claude-code| Claude["Local claude CLI"]
    Provider -->|codex| Codex["Local codex exec CLI"]
    Provider -->|copilot| Copilot["Local GitHub Copilot CLI"]
    Provider -->|apple| Apple["Local fm respond CLI"]
    OpenAI --> Chat["Chat completions request"]
    Azure --> Chat
    Groq --> Chat
    Ollama --> Chat
    Anthropic --> Messages["Messages request"]
    Claude --> Prompt["Flattened prompt over stdin"]
    Codex --> Prompt
    Copilot --> Prompt
    Apple --> Instructions["System prompt via -i, diff over stdin"]
    Chat --> Result["Generated commit message"]
    Messages --> Result
    Prompt --> Result
    Instructions --> Result
```

Configure OpenAI:

```sh
aic config set AIC_AI_PROVIDER=openai AIC_API_KEY=<key> AIC_MODEL=gpt-5.4-mini
```

The default OpenAI model is `gpt-5.4-mini`, the cost-efficient GPT-5.4 variant.

Use a custom compatible endpoint:

```sh
aic config set AIC_AI_PROVIDER=openai AIC_API_URL=https://example.com/v1
```

This existing `openai` + `AIC_API_URL` path remains the catch-all way to use other compatible endpoints when you do not want a dedicated provider preset.

Configure Azure OpenAI:

```sh
aic config set AIC_AI_PROVIDER=azure-openai AIC_API_KEY=<key> AIC_API_URL=https://<resource>.openai.azure.com/openai/v1 AIC_MODEL=<deployment-name>
```

For Azure OpenAI, `AIC_MODEL` is the deployment name used by your Azure OpenAI resource. `AIC_API_URL` must point at the Azure OpenAI v1 base URL.

Configure Anthropic:

```sh
aic config set AIC_AI_PROVIDER=anthropic AIC_API_KEY=<key> AIC_MODEL=claude-sonnet-4-20250514
```

Anthropic defaults to `https://api.anthropic.com/v1`. Override `AIC_API_URL` only if you need a proxy or gateway in front of the Anthropic API.

Configure Groq:

```sh
aic config set AIC_AI_PROVIDER=groq AIC_API_KEY=<key> AIC_MODEL=llama-3.1-8b-instant
```

Groq defaults to `https://api.groq.com/openai/v1` and uses the same chat-completions flow as other OpenAI-compatible providers.

Configure Ollama:

```sh
aic config set AIC_AI_PROVIDER=ollama AIC_MODEL=llama3.2
```

Ollama defaults to `http://localhost:11434/v1` and does not require `AIC_API_KEY`. Override `AIC_API_URL` if your Ollama server is running on another host or port.

Configure Claude Code:

```sh
aic config set AIC_AI_PROVIDER=claude-code AIC_MODEL=default
```

Configure Codex:

```sh
aic config set AIC_AI_PROVIDER=codex AIC_MODEL=default
```

Configure GitHub Copilot CLI:

```sh
aic config set AIC_AI_PROVIDER=copilot AIC_MODEL=default
```

Configure Apple Foundation Models:

```sh
aic config set AIC_AI_PROVIDER=apple AIC_MODEL=default
```

The `apple` provider needs a Mac with Apple Intelligence enabled and the `fm` CLI on `PATH`; run `fm available` to check the model is ready. `aic` calls `fm respond --no-stream --greedy --guardrails permissive-content-transformations`, passing the system prompt and few-shot example through `-i` and only the staged diff over stdin. The alias `fm` is accepted and normalized to `apple`.

The on-device model has an 8,192-token context window, so `aic` caps `AIC_TOKENS_MAX_INPUT` at `6000` for this provider (a lower configured value is kept). Larger diffs are split into chunks and synthesized as usual, which works but takes longer, and summaries of big multi-chunk diffs are noticeably less accurate than hosted models. It is best suited to small, focused commits. If the model still runs out of context, `aic` says so and suggests lowering `AIC_TOKENS_MAX_INPUT`.

For local CLI providers, `AIC_MODEL=default` means "use the CLI's own default model". `aic` does not pass a model flag through in v1.

Use `--provider` to override the configured provider for a single run:

```sh
aic --provider anthropic
aic review --provider groq
aic --provider ollama
aic --provider claude-code
aic review --provider codex
aic review --provider copilot
aic --provider apple
aic log --provider codex --yes
aic models --provider ollama
```

The alias `claudecode` is accepted and normalized to `claude-code`, and `fm` is normalized to `apple`.

List cached or fallback models:

```sh
aic models
aic models --refresh
aic models --provider anthropic
aic models --provider groq
aic models --provider ollama
aic models --provider azure-openai
aic models --provider claude-code
aic models --provider copilot
aic models --provider apple
```

API-provider model responses are cached at `~/.aicommit-models.json` with a 7-day TTL. Local CLI providers report the static `default` model and a note about the installed binary instead of calling a remote models endpoint.
