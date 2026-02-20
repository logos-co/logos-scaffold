# Command References (`lssa-lang`)

- setup tools + sequencer + wallet: `logos-scaffold setup`
- start localnet sequencer: `logos-scaffold localnet start`
- stop localnet sequencer: `logos-scaffold localnet stop`
- check localnet status: `logos-scaffold localnet status`
- build workspace + IDL + client: `logos-scaffold build`
- regenerate IDL only: `logos-scaffold idl build`
- regenerate client only: `logos-scaffold client build`
- run token pilot init: `cargo run --bin run_token_pilot -- init --to <public_account_id>`
- run token transfer: `cargo run --bin run_token_pilot -- transfer --from <public_sender_id> --to <recipient_id> --amount <amount>`
- sync private account state: `wallet account sync-private`
- health diagnostics: `logos-scaffold doctor`
- wallet home env: `export NSSA_WALLET_HOME_DIR=$(pwd)/.scaffold/wallet`
