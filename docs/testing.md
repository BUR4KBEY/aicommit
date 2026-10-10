# Testing

Run formatting:

```sh
cargo fmt --check
```

Run compile checks:

```sh
cargo check
```

Run lint checks:

```sh
cargo clippy --all-targets --all-features -- -D warnings
```

Run tests:

```sh
cargo test
```

The test suite is designed to cover configuration precedence, prompt-template interpolation, token splitting, provider payload handling, Git repository flows, ignore-file behavior, and hook message insertion.

`tests/cli.rs` drives the `aic` binary end to end. Most cases use the built-in
`test` provider; the split-failure cases start a local HTTP mock (`spawn_mock_provider`) so the binary can hit a real OpenAI-compatible endpoint and exercise provider-driven behavior such as token-cap truncation, per-group retries, and "nothing committed" failures.
