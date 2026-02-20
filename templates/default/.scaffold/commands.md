# Command References

## High-Level Scaffold Flow

- bootstrap project: `logos-scaffold new <name> --bootstrap`
- local runtime doctor: `logos-scaffold doctor`
- run one local vertical slice: `logos-scaffold slice run`
- reliability pass (3x): `logos-scaffold slice run --repeat 3`

## Explicit Step-by-Step Flow

1. `logos-scaffold setup`
2. `logos-scaffold localnet start`
3. `logos-scaffold wallet init`
4. `logos-scaffold wallet topup --to <Public/...>`
5. `logos-scaffold deploy hello-world`
6. `logos-scaffold interact hello-world --account-id <Public/...>`
7. `logos-scaffold verify hello-world --account-id <Public/...>`

## Low-Level Equivalents

- standalone sequencer:
  - `RUST_LOG=info RISC0_DEV_MODE=1 target/release/sequencer_runner sequencer_runner/configs/debug`
- wallet install:
  - `cargo install --path wallet --force`
- wallet runtime home:
  - `export NSSA_WALLET_HOME_DIR=$(pwd)/.scaffold/wallet`
- local topup path:
  - `wallet auth-transfer init --account-id <Public/...>` (best effort)
  - `wallet pinata claim --to <Public/...>`

## Notes

- v0.1 is local-only; DevNet is deferred.
- Scaffold writes run artifacts under `.scaffold/state/`.
