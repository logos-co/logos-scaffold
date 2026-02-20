use std::path::PathBuf;

#[derive(Clone, Debug)]
pub(crate) struct RepoRef {
    pub(crate) url: String,
    pub(crate) source: String,
    pub(crate) path: String,
    pub(crate) pin: String,
}

#[derive(Clone, Debug)]
pub(crate) struct FrameworkIdlConfig {
    pub(crate) spec: String,
    pub(crate) path: String,
}

#[derive(Clone, Debug)]
pub(crate) struct FrameworkConfig {
    pub(crate) kind: String,
    pub(crate) version: String,
    pub(crate) idl: FrameworkIdlConfig,
}

#[derive(Clone, Debug)]
pub(crate) struct Config {
    pub(crate) version: String,
    pub(crate) cache_root: String,
    pub(crate) lssa: RepoRef,
    pub(crate) wallet_binary: String,
    pub(crate) wallet_home_dir: String,
    pub(crate) framework: FrameworkConfig,
}

#[derive(Clone, Debug)]
pub(crate) struct Project {
    pub(crate) root: PathBuf,
    pub(crate) config: Config,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct LocalnetState {
    pub(crate) sequencer_pid: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CheckStatus {
    Pass,
    Warn,
    Fail,
}

#[derive(Clone, Debug)]
pub(crate) struct CheckRow {
    pub(crate) status: CheckStatus,
    pub(crate) name: String,
    pub(crate) detail: String,
    pub(crate) remediation: Option<String>,
}

pub(crate) struct Captured {
    pub(crate) status: std::process::ExitStatus,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}
