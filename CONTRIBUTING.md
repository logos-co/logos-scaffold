# Contributing to `logos-scaffold`

## Local Development

Build the scaffold CLI itself:

```bash
cargo build
```

Run the CLI from source:

```bash
cargo run --bin logos-scaffold -- --help
cargo run --bin logos-scaffold -- new test-app
```

## Test Suite

Run all tests:

```bash
cargo test --all-targets
```

Formatting check:

```bash
cargo fmt --check
```

## Working on Generated Projects vs CLI

- This repository builds and tests the scaffold CLI.
- Generated projects are separate workspaces created by `logos-scaffold new`.
- Validate scaffold changes by creating a fresh project and running scaffold commands inside it.

## DOGFOODING Validation

Use [DOGFOODING.md](./DOGFOODING.md) as the canonical validation guide for scaffold DX.

At minimum:

- Onboarding, project creation, setup, localnet, or build changes: rerun `D1` and `D2`.
- Deploy, wallet, or diagnostics changes: rerun the affected `D3` to `D5` scenarios.
- LEZ template or generated-artifact changes: rerun `L1` to `L3`.

Keep all temporary dogfood directories inside your local workspace and remove them after validation.
