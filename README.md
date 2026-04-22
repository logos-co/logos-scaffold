# logos-scaffold

`logos-scaffold` is a Rust CLI for bootstrapping LEZ (Logos Execution Zone) `program_deployment` projects in standalone mode.

## Documentation

- [FURPS+](FURPS.md) — Functional and non-functional requirements
- [ADR](ADR.md) — Architecture Decision Records

## Platform

The CLI is currently Unix-only.
Localnet and process/port detection rely on Unix tools (lsof, ps, kill).

## Scope

- Single external dependency: [LEZ](https://github.com/logos-blockchain/logos-execution-zone/)
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

This installs two binaries on your PATH: `logos-scaffold` and the shorter
alias `lgs`. They are functionally identical; use either.

### Shell completions

`lgs completions <shell>` prints a completion script to stdout. The script
completes both `lgs` and `logos-scaffold`.

Per-shell install instructions live in the CLI help itself:

```bash
lgs completions bash --help
lgs completions zsh --help
```

## DOGFOODING

Canonical dogfooding scenarios live in [DOGFOODING.md](./DOGFOODING.md).
Keep that runbook updated whenever first-class commands, templates, or supported workflows change.

## CLI

All commands below also work under the `lgs` alias (e.g. `lgs setup`).

```bash
logos-scaffold create <name> [--vendor-deps] [--lez-path PATH] [--cache-root PATH]
logos-scaffold new <name> [--vendor-deps] [--lez-path PATH] [--cache-root PATH]
logos-scaffold init
logos-scaffold setup
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
logos-scaffold basecamp setup
logos-scaffold basecamp install [--path PATH]... [--flake REF]... [--profile NAME]
logos-scaffold basecamp launch <profile> [--no-clean]
logos-scaffold doctor [--json]
logos-scaffold report [--out PATH] [--tail N]
logos-scaffold completions <bash|zsh>
logos-scaffold help
```

## Command Semantics

- `create` and `new` are aliases.
- `init` writes `scaffold.toml` with defaults into the current directory so an existing project can use the scaffold workflow. It creates `.scaffold/{state,logs}` and appends `.scaffold` to `.gitignore`. It refuses to overwrite an existing `scaffold.toml`. Run `setup` next.
- `setup` syncs LEZ to pinned commit, builds the standalone `sequencer_service` and `wallet` binaries locally inside the LEZ tree, and seeds a deterministic default wallet from preconfigured public accounts when none is set. Wallet binaries are project-local and are not installed to PATH — use `logos-scaffold wallet ...` commands to interact with the wallet.
- `build [project-path]` runs `setup` and then `cargo build --workspace`.
- `deploy [program-name]` deploys one or all guest programs discovered in `methods/guest/src/bin/*.rs` using prebuilt `.bin` artifacts.
- `localnet start` waits until localnet is actually ready (`pid alive` + `127.0.0.1:3040` reachable), otherwise fails with diagnostics.
- `localnet status` distinguishes managed process, stale state, and foreign listeners.
- `wallet list` shows known wallet accounts (`wallet account list`).
- `wallet topup` checks account state first (`wallet account get --account-id ...`), runs `wallet auth-transfer init --account-id ...` only when the destination is uninitialized, then performs Piñata faucet claim (`wallet pinata claim --to ...`). If address is omitted, scaffold uses project default wallet from `.scaffold/state/wallet.state`.
- `wallet default set` stores a project-scoped default wallet address in `.scaffold/state/wallet.state`.
- `wallet -- ...` forwards raw wallet CLI arguments to the project-local wallet binary while preserving project wallet environment.
- `basecamp setup` pins basecamp + `lgpm`, builds both, and seeds per-profile XDG directories for `alice` and `bob` under `.scaffold/basecamp/profiles/`.
- `basecamp install` resolves the project's `.lgx` sources (auto-discovers `packages.<system>.lgx` from the root flake or immediate sub-flakes), builds each via `nix build`, and installs them into every seeded profile via `lgpm`. Project requirements are documented in [docs/basecamp-module-requirements.md](./docs/basecamp-module-requirements.md); pass `--path`/`--flake` to override auto-discovery.
- `basecamp launch <profile>` scrubs the profile's data/cache, replays `install` to rebuild against current sources, assigns per-profile ports, and execs `basecamp` with the profile's XDG environment. Pass `--no-clean` to skip the scrub.
- `basecamp reset` kills any live basecamp tracked in `launch.state`, wipes `.scaffold/basecamp/profiles/`, clears recorded sources in `basecamp.state` (preserving the pinned basecamp + lgpm binaries), and re-seeds empty alice/bob profiles. Pass `--dry-run` to print the action plan without touching anything.
- `basecamp build-portable` builds the project's `.#lgx-portable` artefacts for hand-loading into a basecamp AppImage. Uses the same auto-discovery and `--path` / `--flake` overrides as `install`, but targets the `lgx-portable` flake output. Profile trees and `basecamp.state` are untouched; `nix build` uses its default symlink layout (`./result-lgx-portable` next to the flake), and the command prints the absolute store paths for the built artefacts so you can copy them into your AppImage yourself.
- `doctor` prints actionable checks and next steps; `--json` is for CI/machine parsing.
- `report` creates a `.tar.gz` diagnostics bundle for GitHub issues using strict allowlist collection with redaction and explicit skip reporting.
- `completions <shell>` prints a shell completion script to stdout. Supported shells: `bash`, `zsh`. The generated script covers both `lgs` and `logos-scaffold`.
- Wallet-facing commands accept `LOGOS_SCAFFOLD_WALLET_PASSWORD` for password override (fallback: local dev default).

## Pinned LEZ Commit

Scaffold enforces this commit for standalone mode (v0.2.0-rc1):

- `35d8df0d031315219f94d1546ceb862b0e5b208f`

## First Success Path

```bash
lgs new my-app
cd my-app
lgs setup
lgs localnet start
lgs build
lgs deploy
lgs wallet topup
lgs wallet -- check-health
```

### Adopt scaffold in an existing project

If you already have a Rust/LEZ project, add scaffold to it without regenerating:

```bash
cd my-existing-project
lgs init
lgs setup
```

`init` only writes `scaffold.toml` and creates `.scaffold/` directories.
It does not touch your `Cargo.toml` or `src/`. Edit `scaffold.toml` if you
need non-default framework settings (e.g. `lez-framework`).

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
