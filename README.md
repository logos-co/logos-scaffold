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
logos-scaffold basecamp modules [--path PATH]... [--flake REF]... [--show]
logos-scaffold basecamp install [--print-output]
logos-scaffold basecamp launch <profile> [--no-clean]
logos-scaffold basecamp reset [--dry-run]
logos-scaffold basecamp build-portable
logos-scaffold basecamp doctor [--json]
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
- `basecamp setup` pins basecamp + `lgpm`, builds both (logged to `.scaffold/logs/<timestamp>-setup-*.log`), and seeds per-profile XDG directories for `alice` and `bob` under `.scaffold/basecamp/profiles/`.
- `basecamp modules` is the sole writer of the captured module set, which lives in `[basecamp.modules.<name>]` sub-sections of `scaffold.toml` (each with `flake` and `role = "project" | "dependency"`). Zero-arg runs auto-discovery: walks project flakes (root `.#lgx` first, else immediate sub-flakes), derives a `module_name` per source (from `metadata.json.name` for local paths; heuristic from the github repo slug for remote refs, with a one-line assumption note you can correct in `scaffold.toml`), then resolves each declared dep name by: (1) already keyed in `[basecamp.modules]`, (2) basecamp preinstall list, (3) the source's own `flake.lock`, (4) scaffold-default pin. Unresolved deps **fail fast** — no silent skip. `--flake <ref>` / `--path <file>` capture explicit project sources; `--show` prints the current set without mutating. Re-runs are idempotent: existing `[basecamp.modules]` entries are preserved so hand-edits survive. Project contract: see [docs/basecamp-module-requirements.md](./docs/basecamp-module-requirements.md).
- `basecamp install` is pure replay: builds every captured source (dependencies first, then project modules — fail-fast on a broken companion pin) and installs them into both `alice` and `bob` via `lgpm`. No source-set flags. If the state is empty on first call it transparently invokes `basecamp modules` in auto-discover mode, prints what was captured, and proceeds. Each nix build logs to `.scaffold/logs/<timestamp>-install.log` with a one-line progress status (duration on both success and failure); `--print-output` (or `LOGOS_SCAFFOLD_PRINT_OUTPUT=1`) opts back into streaming nix output directly for CI.
- `basecamp launch <profile>` scrubs the profile's data/cache, replays captured modules, assigns per-profile ports, and execs `basecamp` with the profile's XDG environment. Before exec, prints a one-line variant-check summary of installed modules so the freeze-on-first-click case (upstream manifest variant mismatch) is visible. Pass `--no-clean` to skip the scrub.
- `basecamp reset` kills any live basecamp tracked in `launch.state`, wipes `.scaffold/basecamp/profiles/`, clears `[basecamp.modules]` from scaffold.toml (preserving the pinned basecamp + lgpm binaries in `basecamp.state`), and re-seeds empty alice/bob profiles. Pass `--dry-run` to print the action plan without touching anything.
- `basecamp build-portable` rebuilds every `role = "project"` entry in `[basecamp.modules]` with attr-swapped `#lgx-portable` for hand-loading into a basecamp AppImage. Zero-arg: sources come from scaffold.toml (managed via `basecamp modules`). `role = "dependency"` entries are intentionally skipped — the target AppImage provides its own release companion modules via its Package Manager catalog. `nix build` uses its default symlink layout (`./result-lgx-portable` next to each flake); the command prints the absolute store paths so you can copy them into the AppImage yourself.
- `basecamp doctor` emits a basecamp-specific health report: captured modules summary (each entry's flake ref, parsed tag/commit annotation for github refs, and any API headers already installed in alice's profile), manifest variant check per seeded profile (flags modules whose `main` is missing the current-platform `-dev` key — the freeze-on-first-click failure mode), dep-pin drift (captured `role = "dependency"` rev vs. scaffold default), and auto-discovery drift (project sources discoverable today but absent from the captured set). `--json` for machine-readable output.
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
