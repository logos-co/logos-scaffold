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
    pub(crate) run: RunConfig,
    pub(crate) basecamp: Option<BasecampConfig>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ModuleRole {
    /// Module the developer is building and shipping. `build-portable` operates
    /// only on these (attr-swap to `#lgx-portable`).
    Project,
    /// Runtime companion resolved from another source's `metadata.json`
    /// `dependencies` array or declared explicitly by the developer.
    /// `build-portable` skips these — the target AppImage provides its own.
    Dependency,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ModuleEntry {
    pub(crate) flake: String,
    pub(crate) role: ModuleRole,
}

#[derive(Clone, Debug)]
pub(crate) struct BasecampConfig {
    pub(crate) pin: String,
    pub(crate) source: String,
    pub(crate) lgpm_flake: String,
    pub(crate) port_base: u16,
    pub(crate) port_stride: u16,
    /// Captured module set keyed by `module_name` (matches `metadata.json`
    /// `name` for the declaring source, and `dependencies` entries for
    /// anything that references it). Parsed from `[basecamp.modules.<name>]`
    /// sub-sections. Sole source of truth for what `install` / `launch` build.
    pub(crate) modules: std::collections::BTreeMap<String, ModuleEntry>,
}

impl Default for BasecampConfig {
    fn default() -> Self {
        Self {
            pin: String::new(),
            source: "https://github.com/logos-co/logos-basecamp".to_string(),
            lgpm_flake: String::new(),
            port_base: 60000,
            port_stride: 10,
            modules: std::collections::BTreeMap::new(),
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

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum BasecampSource {
    Path(String),
    Flake(String),
}

#[derive(Clone, Debug, Default)]
pub(crate) struct BasecampState {
    pub(crate) pin: String,
    pub(crate) basecamp_bin: String,
    pub(crate) lgpm_bin: String,
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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct RunProfile {
    pub(crate) restart_localnet: bool,
    pub(crate) reset_localnet: bool,
    pub(crate) post_deploy: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct RunConfig {
    /// Inline `[run]` values — the unnamed/legacy profile, used when no
    /// `--profile` flag and no `default_profile` is set.
    pub(crate) restart_localnet: bool,
    pub(crate) reset_localnet: bool,
    pub(crate) post_deploy: Vec<String>,
    /// Name of a profile in `profiles` to use when no `--profile` flag is
    /// passed. If `Some` and the named profile exists, it shadows the
    /// inline values above.
    pub(crate) default_profile: Option<String>,
    /// Named profiles parsed from `[run.profiles.<name>]` sub-sections.
    pub(crate) profiles: std::collections::BTreeMap<String, RunProfile>,
}

impl RunConfig {
    /// Resolve the effective `RunProfile` for a given `--profile` selector.
    /// `Some(name)` errors if the profile is absent. `None` falls back to
    /// `default_profile` if set, else the inline values.
    pub(crate) fn resolve_profile(&self, selector: Option<&str>) -> anyhow::Result<RunProfile> {
        if let Some(name) = selector {
            return self.profiles.get(name).cloned().ok_or_else(|| {
                let known: Vec<&str> = self.profiles.keys().map(String::as_str).collect();
                anyhow::anyhow!(
                    "scaffold.toml has no [run.profiles.{name}] section. Known profiles: [{}]",
                    known.join(", ")
                )
            });
        }
        if let Some(name) = self.default_profile.as_deref() {
            return self.profiles.get(name).cloned().ok_or_else(|| {
                anyhow::anyhow!(
                    "scaffold.toml `[run].default_profile = \"{name}\"` but no matching [run.profiles.{name}] section"
                )
            });
        }
        Ok(RunProfile {
            restart_localnet: self.restart_localnet,
            reset_localnet: self.reset_localnet,
            post_deploy: self.post_deploy.clone(),
        })
    }
}
