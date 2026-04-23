use std::path::PathBuf;

use serde::Serialize;

#[derive(Clone, Debug)]
pub(crate) struct RepoRef {
    pub(crate) url: String,
    pub(crate) source: String,
    pub(crate) path: String,
    pub(crate) pin: String,
}

#[derive(Clone, Debug)]
pub(crate) struct LocalnetConfig {
    pub(crate) port: u16,
    pub(crate) risc0_dev_mode: bool,
}

impl Default for LocalnetConfig {
    fn default() -> Self {
        Self {
            port: 3040,
            risc0_dev_mode: true,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Config {
    pub(crate) version: String,
    pub(crate) cache_root: String,
    pub(crate) lez: RepoRef,
    pub(crate) wallet_home_dir: String,
    pub(crate) framework: FrameworkConfig,
    pub(crate) localnet: LocalnetConfig,
    pub(crate) basecamp: Option<BasecampConfig>,
}

#[derive(Clone, Debug)]
pub(crate) struct BasecampConfig {
    pub(crate) pin: String,
    pub(crate) source: String,
    pub(crate) lgpm_flake: String,
    pub(crate) port_base: u16,
    pub(crate) port_stride: u16,
    /// Per-project overrides for runtime companion dependency pins. Keyed by
    /// module name (matches `metadata.json` `dependencies` entries). Takes
    /// precedence over scaffold-level `BASECAMP_DEPENDENCIES` defaults.
    ///
    /// Parsed from `[basecamp.dependencies]` in `scaffold.toml`. Empty when
    /// omitted. Insertion order is stable to keep diagnostics reproducible
    /// (serialized back sorted by key on write).
    pub(crate) dependencies: std::collections::BTreeMap<String, String>,
}

impl Default for BasecampConfig {
    fn default() -> Self {
        Self {
            pin: String::new(),
            source: "https://github.com/logos-co/logos-basecamp".to_string(),
            lgpm_flake: String::new(),
            port_base: 60000,
            port_stride: 10,
            dependencies: std::collections::BTreeMap::new(),
        }
    }
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum BasecampSource {
    Path(String),
    Flake(String),
}

#[derive(Clone, Debug, Default)]
pub(crate) struct BasecampState {
    pub(crate) pin: String,
    pub(crate) basecamp_bin: String,
    pub(crate) lgpm_bin: String,
    /// Dev's own modules (what the project is building and shipping).
    /// `build-portable` operates only on these (attr-swap to `#lgx-portable`).
    pub(crate) project_sources: Vec<BasecampSource>,
    /// Runtime companion modules resolved from manifest `dependencies` against
    /// the scaffold-level pin table or `[basecamp.dependencies]` override.
    /// `install` installs both these and `project_sources`; `build-portable`
    /// ignores these (assumed present in the target AppImage).
    pub(crate) dependencies: Vec<BasecampSource>,
}

impl BasecampState {
    /// Iterate over every captured source (project + dependencies), in
    /// deps-first order. Used by `install` for replay; `build-portable`
    /// instead iterates `project_sources` alone.
    pub(crate) fn all_sources(&self) -> impl Iterator<Item = &BasecampSource> {
        self.dependencies.iter().chain(self.project_sources.iter())
    }

    pub(crate) fn total_sources(&self) -> usize {
        self.project_sources.len() + self.dependencies.len()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum CheckStatus {
    Pass,
    Warn,
    Fail,
}

#[derive(Clone, Debug, Serialize)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LocalnetOwnership {
    Managed,
    Foreign,
    StaleState,
    ManagedNotReady,
    Stopped,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct LocalnetStatusReport {
    pub(crate) tracked_pid: Option<u32>,
    pub(crate) tracked_running: bool,
    pub(crate) listener_present: bool,
    pub(crate) listener_pid: Option<u32>,
    pub(crate) ownership: LocalnetOwnership,
    pub(crate) ready: bool,
    pub(crate) log_path: String,
    pub(crate) remediation: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct DoctorSummary {
    pub(crate) pass: usize,
    pub(crate) warn: usize,
    pub(crate) fail: usize,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct DoctorReport {
    pub(crate) status: String,
    pub(crate) summary: DoctorSummary,
    pub(crate) checks: Vec<CheckRow>,
    pub(crate) next_steps: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct CollectedItem {
    pub(crate) path: String,
    pub(crate) source: String,
    pub(crate) notes: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct SkippedItem {
    pub(crate) path: String,
    pub(crate) reason: String,
}

#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct RedactionSummary {
    pub(crate) files_redacted: usize,
    pub(crate) replacements: usize,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ToolCommandResult {
    pub(crate) name: String,
    pub(crate) command: String,
    pub(crate) status: Option<i32>,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ReportManifest {
    pub(crate) generated_at_unix: u64,
    pub(crate) project_root: String,
    pub(crate) output_archive: String,
    pub(crate) include_count: usize,
    pub(crate) skip_count: usize,
    pub(crate) redaction: RedactionSummary,
    pub(crate) collected: Vec<CollectedItem>,
    pub(crate) skipped: Vec<SkippedItem>,
    pub(crate) warnings: Vec<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct FrameworkConfig {
    pub(crate) kind: String,
    pub(crate) version: String,
    pub(crate) idl: FrameworkIdlConfig,
}

#[derive(Clone, Debug)]
pub(crate) struct FrameworkIdlConfig {
    pub(crate) spec: String,
    pub(crate) path: String,
}
