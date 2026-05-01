# LEZ Framework Template

This project was generated with:

```bash
logos-scaffold new <name> --template lez-framework
```

It uses the [LEZ Framework](https://github.com/jimmy-claw/lez-framework) for an
ergonomic developer experience similar to Anchor on Solana:

- `#[lez_program]` macro eliminates boilerplate
- `#[instruction]` attribute marks instruction handlers
- `#[account(...)]` annotations for account constraints and PDA derivation
- Compile-time IDL generation via `PROGRAM_IDL_JSON`

## First-Time Setup

```bash
export LOGOS_BLOCKCHAIN_CIRCUITS=/path/to/logos-blockchain-circuits # if not installed at ~/.logos-blockchain-circuits
logos-scaffold setup
logos-scaffold localnet start
logos-scaffold doctor
```

Docker must be reachable for RISC Zero guest builds. Generated projects use
RISC Zero's Docker guest builder by default instead of requiring a host-local
RISC Zero Rust toolchain from `rzup install`.

## Build

```bash
logos-scaffold build
```

## IDL

```bash
logos-scaffold build idl [--timeout-sec 1800]
```

## Diagnostics Bundle

```bash
logos-scaffold report [--out PATH] [--tail N]
```

Inspect the generated archive before attaching it to public issues.

## Project Structure

- Program: `methods/guest/src/bin/lez_counter.rs`
- Generated IDL: `idl/lez_counter.json`
- Runner: `src/bin/run_lez_counter.rs`

## Writing Programs

```rust
#[lez_program]
mod my_program {
    #[instruction]
    pub fn my_handler(
        #[account(init, pda = literal("state"))]
        state: AccountWithMetadata,
        #[account(signer)]
        authority: AccountWithMetadata,
    ) -> LezResult {
        // your logic here
    }
}
```
