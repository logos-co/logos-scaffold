# logos-scaffold

`logos-scaffold` is a Rust CLI for bootstrapping LSSA `program_deployment` projects in standalone mode.

## Scope

- Single external dependency: [lssa](https://github.com/logos-blockchain/lssa/)
- Standalone sequencer flow only
- No `logos-blockchain` dependency
- No full-stack/circuits management

## Build

```bash
cargo build
```

## CLI

```bash
logos-scaffold create <name> [--vendor-deps] [--lssa-path PATH] [--cache-root PATH]
logos-scaffold new <name> [--vendor-deps] [--lssa-path PATH] [--cache-root PATH]
logos-scaffold setup [--wallet-install auto|always|never]
logos-scaffold build [project-path]
logos-scaffold localnet start [--timeout-sec N]
logos-scaffold localnet stop
logos-scaffold localnet status [--json]
logos-scaffold localnet logs [--tail N]
logos-scaffold doctor [--json]
logos-scaffold help
```

## Command Semantics

- `create` and `new` are aliases.
- `setup` syncs LSSA to pinned commit, builds standalone `sequencer_runner`, and installs wallet based on `--wallet-install` policy.
- `build [project-path]` runs `setup` with wallet policy `auto` and then `cargo build --workspace`.
- `localnet start` waits until localnet is actually ready (`pid alive` + `127.0.0.1:3040` reachable), otherwise fails with diagnostics.
- `localnet status` distinguishes managed process, stale state, and foreign listeners.
- `doctor` prints actionable checks and next steps; `--json` is for CI/machine parsing.

## Pinned LSSA Commit

Scaffold enforces this commit for standalone mode:

- `dee3f7fa6f2bf63abef00828f246ddacade9cdaf`

## First Success Path

```bash
logos-scaffold new my-app
cd my-app
logos-scaffold setup
logos-scaffold localnet start
export NSSA_WALLET_HOME_DIR=$(pwd)/.scaffold/wallet
logos-scaffold build
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
