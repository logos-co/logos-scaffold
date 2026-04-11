# Command References

- standalone sequencer: `RUST_LOG=info target/release/sequencer_service sequencer/service/configs/debug/sequencer_config.json`
- lez standalone docs: `https://github.com/logos-blockchain/logos-execution-zone/tree/main?tab=readme-ov-file#standalone-mode`
- wallet install: `cargo install --path wallet --force`
- wallet home env: `export NSSA_WALLET_HOME_DIR=$(pwd)/.scaffold/wallet`
- localnet json status: `logos-scaffold localnet status --json`
- doctor json status: `logos-scaffold doctor --json`
- diagnostics bundle for issue reports: `logos-scaffold report --tail 500`
