# Installation

## Homebrew (macOS)

The recommended way to install on macOS is via Homebrew:

```sh
brew install russmckendrick/tap/aicommit
```

To upgrade to the latest version:

```sh
brew upgrade aicommit
```

The Homebrew formula installs the public CLI as `aic`.

## WinGet (Windows)

The recommended way to install on Windows is via WinGet:

```powershell
winget install --id RussMckendrick.Aicommit -e
```

To upgrade later:

```powershell
winget upgrade --id RussMckendrick.Aicommit -e
```

The WinGet package installs the public CLI as `aic`.

If you need a release binary before the latest WinGet package update lands, use
the [GitHub Releases](#github-releases) installer instead.

## GitHub Releases

Pre-built binaries are available for Linux, macOS, and Windows from the
[GitHub Releases](https://github.com/russmckendrick/aicommit/releases) page.

### Linux

Download and install the latest Linux binary:

```sh
ARCH=$(uname -m | sed 's/x86_64/amd64/;s/aarch64/arm64/')
curl -sL "https://github.com/russmckendrick/aicommit/releases/latest/download/aic-linux-${ARCH}" -o aic
chmod +x aic
sudo mv aic /usr/local/bin/
```

### Windows

Download the latest Windows binary with PowerShell:

```powershell
Invoke-WebRequest -Uri "https://github.com/russmckendrick/aicommit/releases/latest/download/aic-windows-amd64.exe" -OutFile "aic.exe"
```

You can then move `aic.exe` to a directory in your `PATH`, or run it directly
from the download location.

### macOS

Homebrew is recommended on macOS, but you can also download the macOS binaries
from GitHub Releases:

```sh
ARCH=$(uname -m | sed 's/x86_64/amd64/;s/arm64/arm64/')
curl -sL "https://github.com/russmckendrick/aicommit/releases/latest/download/aic-darwin-${ARCH}" -o aic
chmod +x aic
sudo mv aic /usr/local/bin/
```

## From Source

Install locally with Cargo:

```sh
cargo install --path .
```

Or build from the repository root:

```sh
cargo build --release
```

The release binary is:

```text
target/release/aic
```

## Setup

After installation, run setup:

```sh
aic setup
```

For hosted providers such as OpenAI, Azure OpenAI, Anthropic, and Groq, have your API key ready before running setup.

If you plan to use `ollama`, start the local Ollama server and pull a model such as `llama3.2` before running `aic setup`.

If you plan to use `claude-code`, `codex`, or `copilot`, install the matching CLI first and sign in there before running `aic setup`. Those providers reuse the external tool's existing authentication instead of `AIC_API_KEY`.

If you plan to use `apple`, make sure Apple Intelligence is enabled and `fm available` reports the system model as available. The `fm` CLI ships with macOS, so there is nothing else to install or sign in to.
