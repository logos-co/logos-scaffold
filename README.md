# logos-scaffold

`logos-scaffold` is a Rust CLI scaffold for Logos v0.1 focused on one end-to-end public flow:
- public wallet-to-wallet transaction
- public program build, deploy, run, modify

## What it does

- Bootstraps a project with `scaffold.toml` and `.scaffold` runtime dirs.
- Bootstraps directly from `lssa/examples/program_deployment` as the project template.
- Uses a cache-first dependency layout by default:
  - macOS: `~/Library/Caches/logos-scaffold`
  - Linux: `$XDG_CACHE_HOME/logos-scaffold` or `~/.cache/logos-scaffold`
  - Windows: `%LOCALAPPDATA%/logos-scaffold/Cache`
- Supports vendor mode (`--vendor-deps`) for local reproducibility.
- Pins explicit SHAs for `lssa` and `logos-blockchain` in `scaffold.toml`.
- Manages localnet start/stop/status/logs in docker or manual mode.
- Runs a simple program deployment vertical slice.
- Provides `doctor` with PASS/WARN/FAIL checks and remediations.

## Build

```bash
cargo build
```

## v0.1 commands

```bash
scaffold new app
cd app
scaffold deps sync
scaffold doctor
scaffold localnet start
scaffold example program-deployment run
```

### Dependency commands

```bash
scaffold deps sync
scaffold deps sync --update-pins
scaffold deps build
scaffold deps build --reset-circuits --yes
scaffold deps circuits import --from-global
scaffold deps circuits import /path/to/logos-blockchain-circuits --force
```

### Localnet commands

```bash
scaffold localnet start --mode docker
scaffold localnet start --mode manual
scaffold localnet status
scaffold localnet logs
scaffold localnet logs sequencer
scaffold localnet stop
```

## Example behavior

`scaffold example program-deployment run` does:
1. Wallet health check.
2. Build example guest methods (`cargo risczero build ...`).
3. Deploy `hello_world.bin` publicly.
4. Create public account and run `run_hello_world` (public execution).
5. Run a public wallet-to-wallet transfer flow (`auth-transfer init/send`).
6. Run `run_hello_world` again to modify public account state.
7. Write artifact to `.scaffold/state/program_deployment_success.json`.

The command runs against the bootstrapped project itself (its local `methods/` and `src/bin`), so generated apps can run the example flow from their own tree.

## Notes

- `doctor` includes wallet install remediation command:
  - `cargo install --path wallet --force`
- circuits reset is guarded:
  - only `scaffold deps build --reset-circuits --yes` removes `~/.logos-blockchain-circuits`
- offline circuits workflow:
  - `scaffold deps circuits import --from-global`
  - or `scaffold deps circuits import <path>`
