# LSSA-Lang Template

This project was generated with:

```bash
logos-scaffold new <name> --template lssa-lang
```

It is a token-pilot-first scaffold focused on contract developer experience:

- annotation-driven contract metadata
- deterministic IDL generation
- generated Rust client integration with `wallet::WalletCore`

## First-Time Setup

```bash
logos-scaffold setup
logos-scaffold localnet start
export NSSA_WALLET_HOME_DIR=$(pwd)/.scaffold/wallet
```

## Build + Generate (Main Flow)

```bash
logos-scaffold build
```

For `lssa-lang`, `logos-scaffold build` runs:

1. `logos-scaffold setup`
2. `cargo build --workspace`
3. `logos-scaffold idl build`
4. `logos-scaffold client build`

You can also run generation steps separately:

```bash
logos-scaffold idl build
logos-scaffold client build
```

## Token Pilot Quickstart

1. Create accounts:

```bash
wallet account new public
wallet account new public
wallet account new private
```

2. Initialize a target account:

```bash
cargo run --bin run_token_pilot -- init --to <public_account_id>
```

3. Public transfer:

```bash
cargo run --bin run_token_pilot -- transfer --from <public_sender_id> --to <public_recipient_id> --amount 10
```

4. Public -> private-owned transfer:

```bash
cargo run --bin run_token_pilot -- transfer --from <public_sender_id> --to Private/<private_recipient_id> --amount 7
wallet account sync-private
```

## How IDL + Client Generation Works

1. You define instruction and account metadata in Rust annotations.
2. `logos-scaffold idl build` extracts metadata and writes deterministic JSON to `idl/`.
3. `logos-scaffold client build` reads `idl/*.json` and generates typed Rust clients in `src/generated/`.

## Macro Guide

`lssa-lang` v1 adds three core macros for contract DX:

1. `#[derive(LssaInstruction)]`: applied to the instruction enum. It extracts instruction variant names and argument types for IDL metadata.
2. `#[derive(LssaAccounts)]`: applied to account-context structs. It captures account fields plus LSSA account annotations from `#[lssa(...)]` such as `mut`, `auth`, `claim_if_default`, and `visibility = "public|private_owned"`.
3. `#[lssa_program(name = "...", version = "...")]`: applied to an inline module containing `#[lssa_instruction(...)]` registrations. It binds instruction enum variants to account structs and emits program-level IDL helpers used by `logos-scaffold idl build`.

Typical usage flow:

- Define instruction enum + derive `LssaInstruction`.
- Define account structs + derive `LssaAccounts` with `#[lssa(...)]` field metadata.
- Register handlers in a `#[lssa_program]` module via `#[lssa_instruction(instruction = ..., accounts = ..., name = ..., execution = ...)]`.
- Run `logos-scaffold idl build` and `logos-scaffold client build`.

## Daily Developer Loop

1. Edit contract code in `methods/guest/src/bin/token_pilot.rs`.
2. Run `logos-scaffold build`.
3. Run or update runner usage in `src/bin/run_token_pilot.rs`.
4. Re-run token-pilot flow against localnet.

## Useful Paths

- Program: `methods/guest/src/bin/token_pilot.rs`
- Generated IDL: `idl/token_pilot.json`
- Generated client: `src/generated/token_pilot_client.rs`
- Runner: `src/bin/run_token_pilot.rs`
