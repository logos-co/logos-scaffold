# logos-scaffold

`logos-scaffold` is a Rust CLI for bootstrapping LSSA `program_deployment` projects in standalone mode.

## Platform

The CLI is currently Unix-only.
Localnet and process/port detection rely on Unix tools (lsof, ps, kill).

## Scope

- Single external dependency: [lssa](https://github.com/logos-blockchain/lssa/)
- Standalone sequencer flow only
- No `logos-blockchain` dependency
- No full-stack/circuits management

## Build

```bash
cargo build
```

## Publishing

1. Bump `version` in `Cargo.toml`.
2. Commit and push the version bump.
3. Create and push a release tag in either format:
   - `X.Y.Z`
   - `vX.Y.Z`
4. The `Publish crate` workflow runs automatically on pushed tags.
5. For manual fallback, run the same workflow via `workflow_dispatch` and pass `version` as `X.Y.Z` or `vX.Y.Z`.

Required secret for publishing:

- `CARGO_REGISTRY_TOKEN` (crates.io API token)

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
logos-scaffold help
```

## Command Semantics

- `create` and `new` are aliases.
- `setup` syncs LSSA to pinned commit, builds standalone `sequencer_runner`, and installs wallet based on `--wallet-install` policy.
- `build [project-path]` runs `setup` with wallet policy `auto` and then `cargo build --workspace`.
- `deploy [program-name]` deploys one or all guest programs discovered in `methods/guest/src/bin/*.rs` using prebuilt `.bin` artifacts.
- `localnet start` waits until localnet is actually ready (`pid alive` + `127.0.0.1:3040` reachable), otherwise fails with diagnostics.
- `localnet status` distinguishes managed process, stale state, and foreign listeners.
- `wallet list` shows known wallet accounts (`wallet account list`).
- `wallet topup` uses a single Piñata faucet claim (`wallet pinata claim --to ...`). If address is omitted, scaffold uses project default wallet from `.scaffold/state/wallet.state`.
- `wallet default set` stores a project-scoped default wallet address in `.scaffold/state/wallet.state`.
- `wallet -- ...` forwards raw wallet CLI commands while preserving project wallet environment.
- `doctor` prints actionable checks and next steps; `--json` is for CI/machine parsing.

## Pinned LSSA Commit

Scaffold enforces this commit for standalone mode:

- `767b5afd388c7981bcdf6f5b5c80159607e07e5b`

## First Success Path

```bash
logos-scaffold new my-app
cd my-app
logos-scaffold setup
logos-scaffold localnet start
export NSSA_WALLET_HOME_DIR=$(pwd)/.scaffold/wallet
logos-scaffold build
logos-scaffold deploy
logos-scaffold wallet list
logos-scaffold wallet default set Public/<base58-account-id>
logos-scaffold wallet topup
export EXAMPLE_PROGRAMS_BUILD_DIR=$(pwd)/target/riscv-guest/example_program_deployment_methods/example_program_deployment_programs/riscv32im-risc0-zkvm-elf/release
wallet check-health
```

Checkpoint commands:

```bash
logos-scaffold localnet status
logos-scaffold doctor
```

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
cargo run --bin run_hello_world -- --program-path "$EXAMPLE_PROGRAMS_BUILD_DIR/hello_world.bin" <public_account_id>
cargo run --bin run_hello_world_through_tail_call_private -- --simple-tail-call-path "$EXAMPLE_PROGRAMS_BUILD_DIR/simple_tail_call.bin" --hello-world-path "$EXAMPLE_PROGRAMS_BUILD_DIR/hello_world.bin" <private_account_id>
```
