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
- `build [project-path]` runs `setup` and then `cargo build` in the same project path.
- `localnet start` runs standalone sequencer only.
- `localnet logs` reads sequencer logs (`--tail` default is `200`).

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
cargo risczero build --manifest-path methods/guest/Cargo.toml
export EXAMPLE_PROGRAMS_BUILD_DIR=$(pwd)/target/riscv32im-risc0-zkvm-elf/docker
wallet check-health
```

Run examples with `cargo run --bin ...` directly from generated project.

If tail-call examples fail with `InvalidProgramBehavior`, update
`methods/guest/src/bin/simple_tail_call.rs` constant
`HELLO_WORLD_PROGRAM_ID_HEX` to the `hello_world.bin` ImageID from
the latest `cargo risczero build` output, then rebuild methods.

## Generated Project

Generated projects:

- copy from `lssa/examples/program_deployment`
- rewrite root `Cargo.toml` to pin workspace deps to `lssa` git `rev`
- include `scaffold.toml` (LSSA-only schema)
- include a scaffold-focused `README.md`
- do not include `GETTING_STARTED.md`
