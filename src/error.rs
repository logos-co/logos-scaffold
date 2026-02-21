use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum SetupError {
    #[error("wallet binary `{binary}` is not on PATH and `--wallet-install never` was requested")]
    WalletMissing { binary: String },
}

#[derive(Debug, Error)]
pub(crate) enum LocalnetError {
    #[error("missing sequencer binary at {path}; run `logos-scaffold setup`")]
    MissingSequencerBinary { path: String },

    #[error("sequencer process exited before becoming ready (pid={pid})\nlast logs:\n{log_tail}")]
    ExitedBeforeReady { pid: u32, log_tail: String },

    #[error("localnet start timed out after {timeout_sec}s (pid={pid})\nlast logs:\n{log_tail}")]
    StartTimeout {
        timeout_sec: u64,
        pid: u32,
        log_tail: String,
    },
}
