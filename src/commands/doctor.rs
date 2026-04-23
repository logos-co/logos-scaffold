use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::bail;

use super::wallet_support::wallet_password;
use crate::commands::basecamp::{
    check_manifest_variants, compute_module_drift, flake_ref as basecamp_flake_ref,
    platform_dev_variant_key,
};
use crate::commands::wallet_support::WALLET_CONFIG_PRIMARY;
use crate::constants::{
    BASECAMP_PROFILES_REL, BASECAMP_PROFILE_ALICE, BASECAMP_PROFILE_BOB, DEFAULT_LEZ_PIN,
    SEQUENCER_BIN_REL_PATH, WALLET_BIN_REL_PATH,
};
use crate::doctor_checks::{
    check_binary, check_container_runtime, check_path, check_port_warn, check_repo,
    check_standalone_support, one_line, print_rows,
};
use crate::model::{BasecampSource, Project};
use crate::model::{CheckRow, CheckStatus, DoctorReport, DoctorSummary};
use crate::process::{pid_running, run_capture, run_with_stdin, set_command_echo};
use crate::project::load_project;
use crate::state::{read_basecamp_state, read_localnet_state};
use crate::DynResult;

const STEP_SETUP: &str = "logos-scaffold setup";
const STEP_LOCALNET_START: &str = "logos-scaffold localnet start";
const STEP_EXPORT_WALLET_HOME: &str = "export NSSA_WALLET_HOME_DIR=$(pwd)/.scaffold/wallet";
const STEP_DOCTOR: &str = "logos-scaffold doctor";

pub(crate) fn cmd_doctor(as_json: bool) -> DynResult<()> {
    if as_json {
        set_command_echo(false);
    }

    let result = cmd_doctor_inner(as_json);

    if as_json {
        set_command_echo(true);
    }

    result
}

fn cmd_doctor_inner(as_json: bool) -> DynResult<()> {
    let report = build_doctor_report()?;

    if as_json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        if report.summary.fail > 0 {
            bail!("doctor reported FAIL checks");
        }
        return Ok(());
    }

    print_rows(&report.checks);
    println!(
        "Summary: {} PASS, {} WARN, {} FAIL",
        report.summary.pass, report.summary.warn, report.summary.fail
    );
    println!("Doctor status: {}", report.status);

    if !report.next_steps.is_empty() {
        println!("Next steps:");
        for step in report.next_steps {
            println!("- {step}");
        }
    }

    if report.summary.fail > 0 {
        bail!("doctor reported FAIL checks");
    }

    Ok(())
}

pub(crate) fn build_doctor_report() -> DynResult<DoctorReport> {
    let project = load_project()?;
    let lez = PathBuf::from(&project.config.lez.path);
    let wallet_home = project.root.join(&project.config.wallet_home_dir);
    let localnet_state_path = project.root.join(".scaffold/state/localnet.state");

    let mut rows = Vec::new();

    rows.push(check_binary("git", true));
    rows.push(check_binary("rustc", true));
    rows.push(check_binary("cargo", true));
    rows.push(check_binary("lsof", true));
    rows.push(check_binary("ps", true));
    rows.push(check_binary("kill", true));
    rows.push(check_container_runtime());

    rows.push(check_repo("lez", &lez, &project.config.lez.pin));

    rows.push(CheckRow {
        status: if project.config.lez.pin == DEFAULT_LEZ_PIN {
            CheckStatus::Pass
        } else {
            CheckStatus::Warn
        },
        name: "lez standalone pin".to_string(),
        detail: format!(
            "configured pin={} expected={}",
            project.config.lez.pin, DEFAULT_LEZ_PIN
        ),
        remediation: if project.config.lez.pin == DEFAULT_LEZ_PIN {
            None
        } else {
            Some(format!(
                "Set repos.lez.pin in scaffold.toml to {} and run `{}`",
                DEFAULT_LEZ_PIN, STEP_SETUP
            ))
        },
    });

    rows.push(check_standalone_support(&lez));

    rows.push(check_path(
        "sequencer binary",
        &lez.join(SEQUENCER_BIN_REL_PATH),
        "Run `logos-scaffold setup`",
    ));

    let wallet_binary_path = lez.join(WALLET_BIN_REL_PATH);
    rows.push(check_path(
        "wallet binary",
        &wallet_binary_path,
        "Run `logos-scaffold setup`",
    ));

    rows.push(check_port_warn(
        "sequencer port 3040",
        "127.0.0.1:3040",
        "Run `logos-scaffold localnet start` (required before running example binaries)",
    ));

    if localnet_state_path.exists() {
        match read_localnet_state(&localnet_state_path) {
            Ok(state) => {
                let (status, detail, remediation) = match state.sequencer_pid {
                    Some(pid) => {
                        let running = pid_running(pid);
                        let status = if running {
                            CheckStatus::Pass
                        } else {
                            CheckStatus::Warn
                        };
                        let remediation = if running {
                            None
                        } else {
                            Some("Run `logos-scaffold localnet start` (required before running example binaries)".to_string())
                        };
                        (status, format!("sequencer pid={pid} running={running}"), remediation)
                    }
                    None => (
                        CheckStatus::Warn,
                        "state file present but sequencer pid missing".to_string(),
                        Some("Run `logos-scaffold localnet start` (required before running example binaries)".to_string()),
                    ),
                };

                rows.push(CheckRow {
                    status,
                    name: "runtime state file".to_string(),
                    detail,
                    remediation,
                });
            }
            Err(err) => rows.push(CheckRow {
                status: CheckStatus::Warn,
                name: "runtime state file".to_string(),
                detail: err.to_string(),
                remediation: Some(
                    "Recreate state via `logos-scaffold localnet start` (required before running example binaries)"
                        .to_string(),
                ),
            }),
        }
    } else {
        rows.push(CheckRow {
            status: CheckStatus::Warn,
            name: "runtime state file".to_string(),
            detail: "missing .scaffold/state/localnet.state".to_string(),
            remediation: Some(
                "Run `logos-scaffold localnet start` (required before running example binaries)"
                    .to_string(),
            ),
        });
    }

    let wallet_cfg = wallet_home.join(WALLET_CONFIG_PRIMARY);
    if wallet_cfg.exists() {
        let cfg_text = fs::read_to_string(&wallet_cfg)?;
        if cfg_text.contains("127.0.0.1:3040") || cfg_text.contains("localhost:3040") {
            rows.push(CheckRow {
                status: CheckStatus::Pass,
                name: "wallet network config".to_string(),
                detail: "wallet points to local sequencer".to_string(),
                remediation: None,
            });
        } else {
            rows.push(CheckRow {
                status: CheckStatus::Warn,
                name: "wallet network config".to_string(),
                detail: "wallet may point to non-local sequencer".to_string(),
                remediation: Some(
                    "Set .scaffold/wallet/wallet_config.json sequencer_addr=http://127.0.0.1:3040"
                        .to_string(),
                ),
            });
        }
    } else {
        rows.push(CheckRow {
            status: CheckStatus::Warn,
            name: "wallet network config".to_string(),
            detail: "missing .scaffold/wallet/wallet_config.json".to_string(),
            remediation: Some("Run `logos-scaffold setup`".to_string()),
        });
    }

    if wallet_binary_path.exists() {
        let mut version_cmd = Command::new(&wallet_binary_path);
        version_cmd.arg("--version");
        match run_capture(&mut version_cmd, "wallet --version") {
            Ok(out) => rows.push(CheckRow {
                status: CheckStatus::Pass,
                name: "wallet version".to_string(),
                detail: one_line(&out.stdout),
                remediation: None,
            }),
            Err(err) => rows.push(CheckRow {
                status: CheckStatus::Warn,
                name: "wallet version".to_string(),
                detail: err.to_string(),
                remediation: Some("Ensure wallet binary is healthy".to_string()),
            }),
        }

        let mut health_cmd = Command::new(&wallet_binary_path);
        health_cmd
            .env("NSSA_WALLET_HOME_DIR", wallet_home.display().to_string())
            .arg("check-health")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        match run_with_stdin(health_cmd, format!("{}\n", wallet_password())) {
            Ok(out) => {
                if out.status.success() {
                    rows.push(CheckRow {
                        status: CheckStatus::Pass,
                        name: "wallet usability".to_string(),
                        detail: "wallet check-health succeeded".to_string(),
                        remediation: None,
                    });
                } else if is_localnet_connectivity_failure(&out.stdout, &out.stderr) {
                    rows.push(CheckRow {
                        status: CheckStatus::Warn,
                        name: "wallet usability".to_string(),
                        detail: "wallet cannot reach local sequencer at http://127.0.0.1:3040"
                            .to_string(),
                        remediation: Some(
                            "Run `logos-scaffold localnet start` (required before running example binaries), then `logos-scaffold doctor`"
                                .to_string(),
                        ),
                    });
                } else {
                    rows.push(CheckRow {
                        status: CheckStatus::Fail,
                        name: "wallet usability".to_string(),
                        detail: one_line(&out.stderr),
                        remediation: Some(
                            "Verify wallet config and run `export NSSA_WALLET_HOME_DIR=$(pwd)/.scaffold/wallet`, then `logos-scaffold doctor`"
                                .to_string(),
                        ),
                    });
                }
            }
            Err(err) => rows.push(CheckRow {
                status: CheckStatus::Fail,
                name: "wallet usability".to_string(),
                detail: err.to_string(),
                remediation: Some(
                    "Verify wallet binary and run `export NSSA_WALLET_HOME_DIR=$(pwd)/.scaffold/wallet`, then `logos-scaffold doctor`"
                        .to_string(),
                ),
            }),
        }
    }

    push_basecamp_rows(&project, &mut rows);

    let summary = DoctorSummary {
        pass: rows
            .iter()
            .filter(|r| matches!(r.status, CheckStatus::Pass))
            .count(),
        warn: rows
            .iter()
            .filter(|r| matches!(r.status, CheckStatus::Warn))
            .count(),
        fail: rows
            .iter()
            .filter(|r| matches!(r.status, CheckStatus::Fail))
            .count(),
    };

    let doctor_status = if summary.fail > 0 {
        "Failing checks"
    } else if summary.warn > 0 {
        "Needs attention"
    } else {
        "Ready"
    };

    let next_steps = derive_next_steps(&rows);

    Ok(DoctorReport {
        status: doctor_status.to_string(),
        summary,
        checks: rows,
        next_steps,
    })
}

fn is_localnet_connectivity_failure(stdout: &str, stderr: &str) -> bool {
    let text = format!("{stdout}\n{stderr}").to_lowercase();
    text.contains("connection refused")
        || text.contains("connecterror")
        || text.contains("127.0.0.1:3040")
        || text.contains("localhost:3040")
}

fn derive_next_steps(rows: &[CheckRow]) -> Vec<String> {
    let mut has_warn_or_fail = false;
    let mut include_setup = false;
    let mut include_localnet_start = false;
    let mut include_wallet_home = false;

    for row in rows {
        if !matches!(row.status, CheckStatus::Warn | CheckStatus::Fail) {
            continue;
        }

        has_warn_or_fail = true;

        let remediation = row.remediation.as_deref().unwrap_or("");
        if remediation.contains(STEP_SETUP) {
            include_setup = true;
        }
        if remediation.contains(STEP_LOCALNET_START) {
            include_localnet_start = true;
        }
        if remediation.contains(STEP_EXPORT_WALLET_HOME)
            || remediation.contains("NSSA_WALLET_HOME_DIR")
        {
            include_wallet_home = true;
        }
    }

    let mut out = Vec::new();
    if include_setup {
        out.push(STEP_SETUP.to_string());
    }
    if include_localnet_start {
        out.push(STEP_LOCALNET_START.to_string());
    }
    if include_wallet_home {
        out.push(STEP_EXPORT_WALLET_HOME.to_string());
    }
    if has_warn_or_fail {
        out.push(STEP_DOCTOR.to_string());
    }
    out
}

/// Append basecamp-specific doctor rows: captured modules summary (each dep's
/// flake ref + commit/tag + api.h header paths), manifest variant checks per
/// seeded profile, and a module-set drift check against auto-discovery.
///
/// No-op when `basecamp.state` is absent — the user hasn't set up basecamp.
fn push_basecamp_rows(project: &Project, rows: &mut Vec<CheckRow>) {
    let state_path = project.root.join(".scaffold/state/basecamp.state");
    let Ok(state) = read_basecamp_state(&state_path) else {
        return; // no state file → basecamp not set up; nothing to report
    };
    if state.basecamp_bin.is_empty() && state.lgpm_bin.is_empty() && state.total_sources() == 0 {
        return;
    }

    // Captured modules summary — one row per project source, one per dep.
    // Label each with its flake ref + a parsed tag/commit + any `*.h` header
    // files found inside the installed alice profile's module dir (if the
    // module name is inferable and `install` has been run).
    let profiles_root = project.root.join(BASECAMP_PROFILES_REL);
    let alice_modules = profiles_root
        .join(BASECAMP_PROFILE_ALICE)
        .join("xdg-data")
        .join(crate::constants::BASECAMP_XDG_APP_SUBPATH)
        .join("modules");

    for src in &state.project_sources {
        rows.push(captured_source_row("basecamp module", src, &alice_modules));
    }
    for src in &state.dependencies {
        rows.push(captured_source_row("basecamp dep", src, &alice_modules));
    }

    // Manifest variant check: each seeded profile's installed modules must
    // expose the current platform's `-dev` key under manifest `main`.
    if let Some(expected_dev) = platform_dev_variant_key() {
        for profile in [BASECAMP_PROFILE_ALICE, BASECAMP_PROFILE_BOB] {
            let profile_dir = profiles_root.join(profile);
            if !profile_dir.is_dir() {
                continue;
            }
            for issue in check_manifest_variants(&profile_dir, profile, expected_dev) {
                rows.push(CheckRow {
                    status: CheckStatus::Warn,
                    name: format!(
                        "basecamp variant: {} in profile {}",
                        issue.module_name, issue.profile
                    ),
                    detail: format!(
                        "main=[{}] missing expected `{}`; plugin will hang on click",
                        issue.available_variants.join(","),
                        expected_dev
                    ),
                    remediation: Some(format!(
                        "rebuild `{}` so its manifest.json `main.{}` key is populated \
                         (upstream logos-module-builder issue); then re-run `basecamp install`",
                        issue.module_name, expected_dev
                    )),
                });
            }
        }
    }

    // Drift: what `basecamp modules` auto-discover would capture today vs.
    // what state actually records.
    match compute_module_drift(project) {
        Ok(drift) if !drift.is_empty() => {
            for src in &drift.discovered_not_captured {
                rows.push(CheckRow {
                    status: CheckStatus::Warn,
                    name: "basecamp drift: uncaptured".to_string(),
                    detail: format!(
                        "discovered `{}` but not captured in basecamp.state",
                        basecamp_flake_ref(src)
                    ),
                    remediation: Some(
                        "run `logos-scaffold basecamp modules` to refresh capture".to_string(),
                    ),
                });
            }
            for src in &drift.captured_not_discovered {
                rows.push(CheckRow {
                    status: CheckStatus::Warn,
                    name: "basecamp drift: stale".to_string(),
                    detail: format!(
                        "captured `{}` no longer discoverable (may be stale)",
                        basecamp_flake_ref(src)
                    ),
                    remediation: Some(
                        "run `logos-scaffold basecamp modules` to refresh; \
                         or `--flake <ref>#lgx` / `--path <file.lgx>` to capture explicitly"
                            .to_string(),
                    ),
                });
            }
        }
        _ => {}
    }
}

/// One doctor row per captured source. Shows the flake ref verbatim plus —
/// when inferable — a "tag" or "commit" annotation and any `*.h` headers
/// already installed under alice's profile (so the dev knows where to
/// `#include <…>` from).
fn captured_source_row(
    label: &str,
    src: &BasecampSource,
    alice_modules: &std::path::Path,
) -> CheckRow {
    let ref_text = basecamp_flake_ref(src);
    let mut detail = ref_text.clone();

    if let BasecampSource::Flake(flake_ref) = src {
        if let Some(label) = github_ref_part_label(flake_ref) {
            detail.push_str(&format!("  ({label})"));
        }
        if let Some(module_name) = infer_module_name_from_flake_ref(flake_ref) {
            let headers = collect_api_headers(alice_modules, &module_name);
            if !headers.is_empty() {
                detail.push_str(&format!(
                    "\n    api headers: {}",
                    headers
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        }
    }

    CheckRow {
        status: CheckStatus::Pass,
        name: label.to_string(),
        detail,
        remediation: None,
    }
}

/// Parse a `github:owner/repo/<ref>#attr` flake ref and label the middle
/// segment as either `tag` (non-hex) or `commit` (≥7 hex chars). Returns
/// `None` for non-github refs or refs without a ref segment.
fn github_ref_part_label(flake_ref: &str) -> Option<String> {
    let rest = flake_ref.strip_prefix("github:")?;
    let before_frag = rest.split_once('#').map_or(rest, |(b, _)| b);
    let parts: Vec<&str> = before_frag.split('/').collect();
    if parts.len() < 3 {
        return None; // no ref segment (defaulted to HEAD)
    }
    let ref_part = parts[2];
    let looks_like_commit = ref_part.len() >= 7
        && ref_part.len() <= 40
        && ref_part.chars().all(|c| c.is_ascii_hexdigit());
    if looks_like_commit {
        Some(format!("commit {}", &ref_part[..ref_part.len().min(12)]))
    } else {
        Some(format!("tag {ref_part}"))
    }
}

/// Extract the likely module name from a flake ref. Uses the repo name's
/// last `-module` suffix heuristic for `github:logos-co/logos-delivery-module/…#lgx`
/// → `delivery_module`, or the path's basename for `path:/abs/tictactoe#lgx`
/// → `tictactoe`. Returns `None` for refs we can't name-map confidently.
fn infer_module_name_from_flake_ref(flake_ref: &str) -> Option<String> {
    let before_frag = flake_ref.split_once('#').map_or(flake_ref, |(b, _)| b);
    if let Some(rest) = before_frag.strip_prefix("github:") {
        let repo = rest.split('/').nth(1)?;
        // Strip "logos-" prefix and any version suffix like "/1.0.0" already gone.
        // Convert dashes to underscores to match module names used in metadata.
        let trimmed = repo.trim_start_matches("logos-");
        return Some(trimmed.replace('-', "_"));
    }
    if let Some(rest) = before_frag.strip_prefix("path:") {
        return Some(
            std::path::Path::new(rest)
                .file_name()?
                .to_string_lossy()
                .replace('-', "_"),
        );
    }
    None
}

/// Return paths to any `*.h` or `*.hpp` files under
/// `<alice_modules>/<module_name>/` — useful for dev awareness of which API
/// headers ship with each installed module. Returns empty if the module
/// isn't installed yet or the directory has no headers.
fn collect_api_headers(alice_modules: &std::path::Path, module_name: &str) -> Vec<PathBuf> {
    let module_dir = alice_modules.join(module_name);
    if !module_dir.is_dir() {
        return Vec::new();
    }
    let mut out = Vec::new();
    // Walk one level deep; typical layouts put headers at the top of the
    // module dir or under an `include/` / `interfaces/` subdir.
    let candidates = [
        module_dir.clone(),
        module_dir.join("include"),
        module_dir.join("interfaces"),
    ];
    for dir in candidates {
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                        if ext.eq_ignore_ascii_case("h") || ext.eq_ignore_ascii_case("hpp") {
                            out.push(path);
                        }
                    }
                }
            }
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod basecamp_doctor_tests {
    use super::*;

    #[test]
    fn github_ref_part_label_recognizes_commit() {
        assert_eq!(
            github_ref_part_label("github:owner/repo/a746cdbc521f72ee22c5a4856fd17a9802bb9d69#lgx"),
            Some("commit a746cdbc521f".to_string())
        );
    }

    #[test]
    fn github_ref_part_label_recognizes_tag() {
        assert_eq!(
            github_ref_part_label("github:logos-co/logos-delivery-module/1.0.0#lgx"),
            Some("tag 1.0.0".to_string())
        );
        assert_eq!(
            github_ref_part_label("github:logos-co/logos-delivery-module/tutorial-v1#lgx"),
            Some("tag tutorial-v1".to_string())
        );
    }

    #[test]
    fn github_ref_part_label_returns_none_for_non_github() {
        assert_eq!(github_ref_part_label("path:/abs/sub#lgx"), None);
        assert_eq!(github_ref_part_label("git+https://example#lgx"), None);
    }

    #[test]
    fn infer_module_name_from_github() {
        assert_eq!(
            infer_module_name_from_flake_ref("github:logos-co/logos-delivery-module/1.0.0#lgx"),
            Some("delivery_module".to_string())
        );
    }

    #[test]
    fn infer_module_name_from_path() {
        assert_eq!(
            infer_module_name_from_flake_ref("path:/abs/tictactoe-ui-cpp#lgx"),
            Some("tictactoe_ui_cpp".to_string())
        );
    }
}
