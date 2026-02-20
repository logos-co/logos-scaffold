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
logos-scaffold build [project-path]
logos-scaffold setup
logos-scaffold localnet start
logos-scaffold localnet stop
logos-scaffold localnet status
logos-scaffold localnet logs [--tail N]
logos-scaffold doctor
```

## Command Semantics

- `create` and `new` are aliases.
- `setup` does LSSA sync to pinned commit, standalone sequencer build, wallet install.
- `build [project-path]` runs `setup` and then `cargo build --workspace` in the same project path.
- `localnet start` runs standalone sequencer only.
- `localnet logs` reads sequencer logs (`--tail` default is `200`).
- `doctor` prints remediations for both `WARN` and `FAIL`, then shows summary and next steps.

## Doctor UX

`logos-scaffold doctor` now always prints actionable guidance:

- remediations on `WARN` and `FAIL`
- summary footer: `Summary: <pass> PASS, <warn> WARN, <fail> FAIL`
- overall status: `Ready`, `Needs attention`, or `Failing checks`
- deduplicated `Next steps` commands

When localnet is down, doctor explicitly tells you to run:

```bash
logos-scaffold localnet start
```

This is required before running example binaries.

## Pinned LSSA Commit

Scaffold enforces this commit for standalone mode:

- `dee3f7fa6f2bf63abef00828f246ddacade9cdaf`

## Typical Flow

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

If tail-call examples fail with `InvalidProgramBehavior`, update
`methods/guest/src/bin/simple_tail_call.rs` constant
`HELLO_WORLD_PROGRAM_ID_HEX` to the `hello_world.bin` ImageID from
the latest `cargo risczero build` output, then rebuild methods.
