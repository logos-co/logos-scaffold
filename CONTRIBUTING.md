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

## Cargo Command Contexts

Use Cargo directly when you are working on this repository:

```bash
cargo build
cargo test --all-targets
cargo fmt --check
```

Inside generated projects, prefer scaffold commands for the scaffolded workflow:

```bash
logos-scaffold setup
logos-scaffold localnet start
logos-scaffold build
logos-scaffold deploy
logos-scaffold wallet topup
logos-scaffold localnet stop
```

Direct `cargo run --bin ...` commands in generated projects are for example
runners after setup, localnet, build, and deploy prerequisites are ready.

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
- Keep temporary dogfood projects under `.dogfood/` so they do not pollute the source tree.

## Dogfooding Validation

Use [DOGFOODING.md](./DOGFOODING.md) as the canonical validation guide for scaffold DX.

At minimum:

- Onboarding, project creation, setup, localnet, or build changes: rerun `D1` and `D2`.
- Deploy, wallet, or diagnostics changes: rerun the affected `D3` to `D5` scenarios.
- LEZ template or generated-artifact changes: rerun `L1` to `L4`.

```bash
cargo build
cargo run --bin logos-scaffold -- new .dogfood/manual/dogfood-app --lez-path /absolute/path/to/logos-execution-zone
cd .dogfood/manual/dogfood-app
logos-scaffold setup
logos-scaffold localnet start
logos-scaffold doctor
logos-scaffold build
logos-scaffold deploy
logos-scaffold wallet topup
logos-scaffold localnet stop
```

On Raspberry Pi OS, use the Docker dogfood lab instead of running RISC0-related
flows directly on ARM:

```bash
./dogfood/run-container.sh
```

The AI agent should then follow `dogfood/AGENT.md` inside the container and
record evidence under `.dogfood/artifacts/`.
