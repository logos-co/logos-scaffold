# Default Template (Hello World)

This project was generated with:

```bash
logos-scaffold new <name> --template default
```

Use this template when you want the baseline `program_deployment` hello-world flows.

## Prerequisites

- `git`, `rustc`, `cargo`
- Docker (required for guest method builds)

## First-Time Setup

```bash
logos-scaffold setup
logos-scaffold localnet start
export NSSA_WALLET_HOME_DIR=$(pwd)/.scaffold/wallet
logos-scaffold build
```

After build, guest binaries are in:

```bash
export EXAMPLE_PROGRAMS_BUILD_DIR=$(pwd)/target/riscv-guest/example_program_deployment_methods/example_program_deployment_programs/riscv32im-risc0-zkvm-elf/release
```

## First Successful Transaction

1. Create a public account:

```bash
wallet account new public
```

2. Run hello world:

```bash
cargo run --bin run_hello_world -- <public_account_id>
```

## Other Included Runners

```bash
cargo run --bin run_hello_world_private -- <private_account_id>
cargo run --bin run_hello_world_with_authorization -- <public_account_id>
cargo run --bin run_hello_world_with_move_function -- write-public <public_account_id> <text>
cargo run --bin run_hello_world_through_tail_call -- <public_account_id>
cargo run --bin run_hello_world_through_tail_call_private -- <private_account_id>
cargo run --bin run_hello_world_with_authorization_through_tail_call_with_pda
```

## Optional Program Path Overrides

```bash
cargo run --bin run_hello_world -- --program-path "$EXAMPLE_PROGRAMS_BUILD_DIR/hello_world.bin" <public_account_id>
cargo run --bin run_hello_world_through_tail_call_private -- --simple-tail-call-path "$EXAMPLE_PROGRAMS_BUILD_DIR/simple_tail_call.bin" --hello-world-path "$EXAMPLE_PROGRAMS_BUILD_DIR/hello_world.bin" <private_account_id>
```

## Troubleshooting

```bash
logos-scaffold doctor
logos-scaffold localnet status
logos-scaffold localnet logs --tail 200
```
