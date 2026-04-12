# logos-scaffold

`logos-scaffold` is a Rust CLI for bootstrapping LSSA `program_deployment` projects in standalone mode.

## Platform

The CLI is currently Unix-only.
Localnet and process/port detection rely on Unix tools (lsof, ps, kill).

## Scope

- Single external dependency: [LEZ](https://github.com/logos-blockchain/lssa/)
- Standalone sequencer flow only
- No `logos-blockchain` dependency
- No full-stack/circuits management

## Prerequisites

- `git`, `rustc`, `cargo`
- Unix process helpers: `lsof`, `ps`, `kill`
- Container runtime for guest builds: Docker or Podman

## Install

```bash
cargo install --path .
```

## DOGFOODING

Canonical dogfooding scenarios live in [DOGFOODING.md](./DOGFOODING.md).
Keep that runbook updated whenever first-class commands, templates, or supported workflows change.

## CLI

```bash
logos-scaffold create <name> [--vendor-deps] [--lssa-path PATH] [--cache-root PATH]
logos-scaffold new <name> [--vendor-deps] [--lssa-path PATH] [--cache-root PATH]
logos-scaffold setup [--wallet-install auto|always|never]
logos-scaffold build [project-path]
logos-scaffold deploy [program-name]
logos-scaffold localnet start [--timeout-sec N]
logos-scaffold localnet stop
logos-scaffold localnet status [--json]
logos-scaffold localnet logs [--tail N]
logos-scaffold wallet list [--long]
logos-scaffold wallet topup [<address> | --address <address-ref>] [--dry-run]
logos-scaffold wallet default set <address-ref>
logos-scaffold wallet default set --address <address-ref>
logos-scaffold wallet -- <wallet-command...>
logos-scaffold doctor [--json]
logos-scaffold report [--out PATH] [--tail N]
logos-scaffold help
```

## Command Semantics

- `create` and `new` are aliases.
- `setup` syncs LSSA to pinned commit, builds standalone `sequencer_runner`, installs wallet based on `--wallet-install` policy, and seeds a deterministic default wallet from preconfigured public accounts when none is set.
- `build [project-path]` runs `setup` with wallet policy `auto` and then `cargo build --workspace`.
- `deploy [program-name]` deploys one or all guest programs discovered in `methods/guest/src/bin/*.rs` using prebuilt `.bin` artifacts.
- `localnet start` waits until localnet is actually ready (`pid alive` + `127.0.0.1:3040` reachable), otherwise fails with diagnostics.
- `localnet status` distinguishes managed process, stale state, and foreign listeners.
- `wallet list` shows known wallet accounts (`wallet account list`).
- `wallet topup` checks account state first (`wallet account get --account-id ...`), runs `wallet auth-transfer init --account-id ...` only when the destination is uninitialized, then performs Piñata faucet claim (`wallet pinata claim --to ...`). If address is omitted, scaffold uses project default wallet from `.scaffold/state/wallet.state`.
- `wallet default set` stores a project-scoped default wallet address in `.scaffold/state/wallet.state`.
- `wallet -- ...` forwards raw wallet CLI commands while preserving project wallet environment.
- `doctor` prints actionable checks and next steps; `--json` is for CI/machine parsing.
- `report` creates a `.tar.gz` diagnostics bundle for GitHub issues using strict allowlist collection with redaction and explicit skip reporting.
- Wallet-facing commands accept `LOGOS_SCAFFOLD_WALLET_PASSWORD` for password override (fallback: local dev default).

## Pinned LSSA Commit

Scaffold enforces this commit for standalone mode:

- `767b5afd388c7981bcdf6f5b5c80159607e07e5b`

## First Success Path

```bash
logos-scaffold new my-app
cd my-app
logos-scaffold setup
logos-scaffold localnet start
logos-scaffold build
logos-scaffold deploy
logos-scaffold wallet topup
logos-scaffold wallet -- check-health
```

`setup` automatically seeds `.scaffold/state/wallet.state` with the first preconfigured public account when no default is present.

Checkpoint commands:

```bash
logos-scaffold localnet status
logos-scaffold doctor
```

## LEZ Framework

To use the [LEZ Framework](https://github.com/jimmy-claw/lez-framework) for an
ergonomic developer experience similar to Anchor on Solana:

```
logos-scaffold new <name> --template lez-framework
```

See [LEZ Framework Template](./templates/lez-framework/README.md) for details.

## Troubleshooting

- If `localnet start` fails, inspect:

```bash
logos-scaffold localnet logs --tail 200
```

- If status reports `ownership: foreign`, stop external listeners on `127.0.0.1:3040` before starting scaffold localnet.
- If status reports stale state, run:

```bash
logos-scaffold localnet stop
logos-scaffold localnet start
```

- JSON status for tooling:

```bash
logos-scaffold localnet status --json
logos-scaffold doctor --json
logos-scaffold report --tail 500
```

## Example Runs

Run examples directly without passing `.bin` paths:

```bash
cargo run --bin run_hello_world -- <public_account_id>
cargo run --bin run_hello_world_private -- <private_account_id>
cargo run --bin run_hello_world_with_authorization -- <public_account_id>
cargo run --bin run_hello_world_with_move_function -- write-public <public_account_id> <text>
cargo run --bin run_hello_world_through_tail_call -- <public_account_id>
cargo run --bin run_hello_world_through_tail_call_private -- <private_account_id>
cargo run --bin run_hello_world_with_authorization_through_tail_call_with_pda
```

Optional overrides for custom binaries:

```bash
export EXAMPLE_PROGRAMS_BUILD_DIR=$(pwd)/target/riscv-guest/example_program_deployment_methods/example_program_deployment_programs/riscv32im-risc0-zkvm-elf/release
cargo run --bin run_hello_world -- --program-path "$EXAMPLE_PROGRAMS_BUILD_DIR/hello_world.bin" <public_account_id>
cargo run --bin run_hello_world_through_tail_call_private -- --simple-tail-call-path "$EXAMPLE_PROGRAMS_BUILD_DIR/simple_tail_call.bin" --hello-world-path "$EXAMPLE_PROGRAMS_BUILD_DIR/hello_world.bin" <private_account_id>
```
