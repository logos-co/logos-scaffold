# logos-scaffold

`logos-scaffold` is a Rust CLI for bootstrapping and validating a local-only LSSA `program_deployment` vertical slice.

## v0.1 Scope (Local-Only)

- Single external dependency: [lssa](https://github.com/logos-blockchain/lssa/)
- Local sequencer runtime only (`http://127.0.0.1:3040`)
- Deterministic wallet bootstrap + local topup flow
- One end-to-end hello-world path: deploy, interact, verify
- No `logos-blockchain` dependency
- No DevNet runtime in v0.1

## Deferred to v0.2

- DevNet vertical slice and multi-network profile support
- DevNet-oriented wallet/RPC safety controls beyond local defaults

## Build

```bash
cargo build
```

## CLI

```bash
logos-scaffold create <name> [--vendor-deps] [--lssa-path PATH] [--cache-root PATH] [--bootstrap]
logos-scaffold new <name> [--vendor-deps] [--lssa-path PATH] [--cache-root PATH] [--bootstrap]
logos-scaffold build [project-path]
logos-scaffold setup
logos-scaffold localnet start
logos-scaffold localnet stop
logos-scaffold localnet status
logos-scaffold localnet logs [--tail N]
logos-scaffold localnet reset
logos-scaffold wallet init
logos-scaffold wallet topup --to <Public/...>
logos-scaffold deploy hello-world
logos-scaffold interact hello-world --account-id <Public/...>
logos-scaffold verify hello-world --account-id <Public/...>
logos-scaffold slice run [--repeat N]
logos-scaffold doctor
```

## Normative Success Path

```bash
logos-scaffold new my-app --bootstrap
```

`--bootstrap` runs:

1. `logos-scaffold setup`
2. `logos-scaffold localnet start`
3. `logos-scaffold wallet init`
4. `logos-scaffold slice run`

## Manual Vertical Slice

```bash
logos-scaffold new my-app
cd my-app
logos-scaffold setup
logos-scaffold localnet start
logos-scaffold wallet init
logos-scaffold wallet topup --to <Public/account_id>
logos-scaffold deploy hello-world
logos-scaffold interact hello-world --account-id <Public/account_id>
logos-scaffold verify hello-world --account-id <Public/account_id>
```

## Reliability Check (3x)

```bash
logos-scaffold slice run --repeat 3
```

Latest slice artifact is written to `.scaffold/state/slice-last.json`.

## Wallet Config Compatibility

v0.1 standardizes wallet config at `.scaffold/wallet/wallet_config.json`.

Legacy `.scaffold/wallet/config.json` is still supported and migrated by `logos-scaffold wallet init`.

## Pinned LSSA Commit

Scaffold enforces:

- `dee3f7fa6f2bf63abef00828f246ddacade9cdaf`
