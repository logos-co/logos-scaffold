# Command References (`spel`)

- setup tools + sequencer + wallet: `logos-scaffold setup`
- start localnet sequencer: `logos-scaffold localnet start`
- stop localnet sequencer: `logos-scaffold localnet stop`
- check localnet status: `logos-scaffold localnet status`
- build workspace + IDL + generate clients (`src/generated`): `logos-scaffold build`
- regenerate IDL only (no client regeneration): `logos-scaffold build idl`
- run counter init: `cargo run --bin run_lez_counter -- init --to <account_id>`
- run counter increment: `cargo run --bin run_lez_counter -- increment --counter <id> --authority <id> --amount <n>`
- health diagnostics: `logos-scaffold doctor`
- diagnostics bundle for issue reports: `logos-scaffold report --tail 500`
- wallet home env: `export NSSA_WALLET_HOME_DIR=$(pwd)/.scaffold/wallet`