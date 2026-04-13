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

## Recommended Validation Flow

For CLI changes affecting onboarding:

```bash
cargo build
cargo run --bin logos-scaffold -- new dogfood-app --lez-path /absolute/path/to/logos-execution-zone
cd dogfood-app
logos-scaffold setup
logos-scaffold localnet start
logos-scaffold doctor
logos-scaffold build
logos-scaffold deploy
logos-scaffold wallet topup
logos-scaffold localnet stop
```

Keep all temporary dogfood directories inside your local workspace and remove them after validation.
