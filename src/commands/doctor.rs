use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::bail;

use crate::constants::{DEFAULT_LSSA_PIN, DEFAULT_WALLET_PASSWORD};
use crate::doctor_checks::{
    check_binary, check_path, check_port_warn, check_repo, check_standalone_support, one_line,
    print_rows,
};
use crate::model::{CheckRow, CheckStatus, DoctorReport, DoctorSummary};
use crate::process::{pid_running, run_capture, run_with_stdin, set_command_echo, which};
use crate::project::load_project;
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
    let project = load_project()?;
    let lssa = PathBuf::from(&project.config.lssa.path);
    let wallet_home = project.root.join(&project.config.wallet_home_dir);
    let localnet_state_path = project.root.join(".scaffold/state/localnet.state");

    let mut rows = Vec::new();

    rows.push(check_binary("git", true));
    rows.push(check_binary("rustc", true));
    rows.push(check_binary("cargo", true));
    rows.push(check_binary(&project.config.wallet_binary, true));

    rows.push(check_repo("lssa", &lssa, &project.config.lssa.pin));

    rows.push(CheckRow {
        status: if project.config.lssa.pin == DEFAULT_LSSA_PIN {
            CheckStatus::Pass
        } else {
            CheckStatus::Warn
        },
        name: "lssa standalone pin".to_string(),
        detail: format!(
            "configured pin={} expected={}",
            project.config.lssa.pin, DEFAULT_LSSA_PIN
        ),
        remediation: if project.config.lssa.pin == DEFAULT_LSSA_PIN {
            None
        } else {
            Some(format!(
                "Set repos.lssa.pin in scaffold.toml to {} and run `{}`",
                DEFAULT_LSSA_PIN, STEP_SETUP
            ))
        },
    });

    rows.push(check_standalone_support(&lssa));

    rows.push(check_path(
        "sequencer binary",
        &lssa.join("target/release/sequencer_runner"),
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

    let wallet_cfg = wallet_home.join("config.json");
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
                    "Set .scaffold/wallet/config.json sequencer_addr=http://127.0.0.1:3040"
                        .to_string(),
                ),
            });
        }
    } else {
        rows.push(CheckRow {
            status: CheckStatus::Warn,
            name: "wallet network config".to_string(),
            detail: "missing .scaffold/wallet/config.json".to_string(),
            remediation: Some("Run `logos-scaffold setup`".to_string()),
        });
    }

    if which(&project.config.wallet_binary).is_some() {
        let mut version_cmd = Command::new(&project.config.wallet_binary);
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

        let mut health_cmd = Command::new(&project.config.wallet_binary);
        health_cmd
            .env("NSSA_WALLET_HOME_DIR", wallet_home.display().to_string())
            .arg("check-health")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        match run_with_stdin(health_cmd, format!("{DEFAULT_WALLET_PASSWORD}\n")) {
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

    if as_json {
        let report = DoctorReport {
            status: doctor_status.to_string(),
            summary,
            checks: rows,
            next_steps,
        };
        println!("{}", serde_json::to_string_pretty(&report)?);
        if report.summary.fail > 0 {
            bail!("doctor reported FAIL checks");
        }
        return Ok(());
    }

    print_rows(&rows);
    println!(
        "Summary: {} PASS, {} WARN, {} FAIL",
        summary.pass, summary.warn, summary.fail
    );
    println!("Doctor status: {doctor_status}");

    if !next_steps.is_empty() {
        println!("Next steps:");
        for step in next_steps {
            println!("- {step}");
        }
    }

    if summary.fail > 0 {
        bail!("doctor reported FAIL checks");
    }

    Ok(())
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
