use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::bail;

use super::wallet_support::wallet_password;
use crate::commands::wallet_support::WALLET_CONFIG_PRIMARY;
use crate::constants::{DEFAULT_LEZ_PIN, SEQUENCER_BIN_REL_PATH, WALLET_BIN_REL_PATH};
use crate::doctor_checks::{
    check_binary, check_container_runtime, check_path, check_port_warn, check_repo,
    check_standalone_support, one_line, print_rows,
};
use crate::model::{CheckRow, CheckStatus, DoctorReport, DoctorSummary};
use crate::process::{pid_running, run_capture, run_with_stdin, set_command_echo};
use crate::project::{ensure_lez_project, load_project, require_lez_repo};
use crate::state::read_localnet_state;
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
    ensure_lez_project(&project, "logos-scaffold doctor")?;
    let lez_repo = require_lez_repo(&project, "logos-scaffold doctor")?;
    let lez = PathBuf::from(&lez_repo.path);
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

    rows.push(check_repo("lez", &lez, &lez_repo.pin));

    rows.push(CheckRow {
        status: if lez_repo.pin == DEFAULT_LEZ_PIN {
            CheckStatus::Pass
        } else {
            CheckStatus::Warn
        },
        name: "lez standalone pin".to_string(),
        detail: format!(
            "configured pin={} expected={}",
            lez_repo.pin, DEFAULT_LEZ_PIN
        ),
        remediation: if lez_repo.pin == DEFAULT_LEZ_PIN {
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
