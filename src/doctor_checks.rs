use std::env;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use crate::constants::DEFAULT_LEZ_PIN;
use crate::model::{CheckRow, CheckStatus};
use crate::process::{port_open, which};
use crate::repo::{git_clean, git_head_sha};

const LOGOS_BLOCKCHAIN_CIRCUITS_ENV: &str = "LOGOS_BLOCKCHAIN_CIRCUITS";
const EXPECTED_LOGOS_BLOCKCHAIN_CIRCUITS_VERSION: &str = "v0.4.2";

pub(crate) fn check_binary(binary: &str, required: bool) -> CheckRow {
    if let Some(path) = which(binary) {
        CheckRow {
            status: CheckStatus::Pass,
            name: format!("tool {binary}"),
            detail: format!("found {}", path.display()),
            remediation: None,
        }
    } else {
        CheckRow {
            status: if required {
                CheckStatus::Fail
            } else {
                CheckStatus::Warn
            },
            name: format!("tool {binary}"),
            detail: "not found on PATH".to_string(),
            remediation: Some(match binary {
                "wallet" => "Run `cargo install --path wallet --force`".to_string(),
                _ => format!("Install `{binary}`"),
            }),
        }
    }
}

pub(crate) fn check_container_runtime() -> CheckRow {
    container_runtime_row(which("docker"), which("podman"))
}

pub(crate) fn check_logos_blockchain_circuits() -> Vec<CheckRow> {
    let mut rows = Vec::new();
    let circuits_dir = match resolve_circuits_dir() {
        Some(path) => path,
        None => {
            rows.push(CheckRow {
                status: CheckStatus::Fail,
                name: "logos-blockchain-circuits directory".to_string(),
                detail: format!(
                    "{LOGOS_BLOCKCHAIN_CIRCUITS_ENV} is not set and ~/.logos-blockchain-circuits is missing"
                ),
                remediation: Some(format!(
                    "Install logos-blockchain-circuits and set `{LOGOS_BLOCKCHAIN_CIRCUITS_ENV}` to its directory"
                )),
            });
            return rows;
        }
    };

    rows.push(CheckRow {
        status: CheckStatus::Pass,
        name: "logos-blockchain-circuits directory".to_string(),
        detail: format!("found {}", circuits_dir.display()),
        remediation: None,
    });

    rows.push(check_circuits_version(&circuits_dir));
    rows.push(check_circuits_file(
        "circuits prover",
        &circuits_dir.join("prover"),
        "Install a logos-blockchain-circuits release with a root-level prover binary",
    ));
    rows.push(check_circuits_file(
        "zksign witness generator",
        &circuits_dir.join("zksign/witness_generator"),
        "Install a logos-blockchain-circuits release with zksign/witness_generator",
    ));
    rows.push(check_prover_executes(&circuits_dir.join("prover")));
    rows
}

fn resolve_circuits_dir() -> Option<PathBuf> {
    if let Ok(path) = env::var(LOGOS_BLOCKCHAIN_CIRCUITS_ENV) {
        let path = PathBuf::from(path);
        if path.is_dir() {
            return Some(path);
        }
        return None;
    }

    let home = env::var_os("HOME").map(PathBuf::from)?;
    let path = home.join(".logos-blockchain-circuits");
    path.is_dir().then_some(path)
}

fn check_circuits_version(circuits_dir: &Path) -> CheckRow {
    let version_path = circuits_dir.join("VERSION");
    match fs::read_to_string(&version_path) {
        Ok(version) => {
            let version = version.trim();
            let status = if version == EXPECTED_LOGOS_BLOCKCHAIN_CIRCUITS_VERSION {
                CheckStatus::Pass
            } else {
                CheckStatus::Warn
            };
            CheckRow {
                status,
                name: "logos-blockchain-circuits version".to_string(),
                detail: format!(
                    "configured={version} expected={EXPECTED_LOGOS_BLOCKCHAIN_CIRCUITS_VERSION}"
                ),
                remediation: if status == CheckStatus::Pass {
                    None
                } else {
                    Some(format!(
                        "Install logos-blockchain-circuits {EXPECTED_LOGOS_BLOCKCHAIN_CIRCUITS_VERSION}"
                    ))
                },
            }
        }
        Err(err) => CheckRow {
            status: CheckStatus::Warn,
            name: "logos-blockchain-circuits version".to_string(),
            detail: format!("failed to read {}: {err}", version_path.display()),
            remediation: Some("Install a complete logos-blockchain-circuits release".to_string()),
        },
    }
}

fn check_circuits_file(name: &str, path: &Path, remediation: &str) -> CheckRow {
    if path.is_file() {
        CheckRow {
            status: CheckStatus::Pass,
            name: name.to_string(),
            detail: format!("found {}", path.display()),
            remediation: None,
        }
    } else {
        CheckRow {
            status: CheckStatus::Fail,
            name: name.to_string(),
            detail: format!("missing {}", path.display()),
            remediation: Some(remediation.to_string()),
        }
    }
}

fn check_prover_executes(prover: &Path) -> CheckRow {
    if !prover.is_file() {
        return CheckRow {
            status: CheckStatus::Fail,
            name: "circuits prover execution".to_string(),
            detail: format!("missing {}", prover.display()),
            remediation: Some("Install a compatible logos-blockchain-circuits release".to_string()),
        };
    }

    match Command::new(prover).output() {
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if output.status.code() == Some(132)
                || stderr.to_lowercase().contains("illegal instruction")
            {
                return CheckRow {
                    status: CheckStatus::Fail,
                    name: "circuits prover execution".to_string(),
                    detail: format!(
                        "prover cannot execute on this CPU/runtime: {}",
                        one_line(&stderr)
                    ),
                    remediation: Some(
                        "Use a circuits/prover build compatible with the Docker CPU, or run dogfood on a compatible amd64 host"
                            .to_string(),
                    ),
                };
            }

            CheckRow {
                status: CheckStatus::Pass,
                name: "circuits prover execution".to_string(),
                detail: format!("prover started and exited with {}", output.status),
                remediation: None,
            }
        }
        Err(err) => CheckRow {
            status: CheckStatus::Fail,
            name: "circuits prover execution".to_string(),
            detail: format!("failed to execute {}: {err}", prover.display()),
            remediation: Some(
                "Install a logos-blockchain-circuits prover binary for this platform".to_string(),
            ),
        },
    }
}

fn container_runtime_row(docker: Option<PathBuf>, podman: Option<PathBuf>) -> CheckRow {
    match (docker, podman) {
        (Some(path), _) => CheckRow {
            status: CheckStatus::Pass,
            name: "container runtime".to_string(),
            detail: format!("found docker at {}", path.display()),
            remediation: None,
        },
        (None, Some(path)) => CheckRow {
            status: CheckStatus::Pass,
            name: "container runtime".to_string(),
            detail: format!("found podman at {}", path.display()),
            remediation: None,
        },
        (None, None) => CheckRow {
            status: CheckStatus::Warn,
            name: "container runtime".to_string(),
            detail: "neither docker nor podman found on PATH".to_string(),
            remediation: Some(
                "Install Docker or Podman (required for guest builds that use risc0 tooling)"
                    .to_string(),
            ),
        },
    }
}

pub(crate) fn check_repo(name: &str, path: &Path, pin: &str) -> CheckRow {
    if !path.exists() {
        return CheckRow {
            status: CheckStatus::Fail,
            name: format!("repo {name}"),
            detail: format!("missing {}", path.display()),
            remediation: Some("Run `logos-scaffold setup`".to_string()),
        };
    }

    match git_head_sha(path) {
        Ok(head) => {
            let mut status = if head == pin {
                CheckStatus::Pass
            } else {
                CheckStatus::Fail
            };

            let mut detail = format!("pin={pin}, head={head}");
            if let Ok(clean) = git_clean(path) {
                if !clean {
                    if status == CheckStatus::Pass {
                        status = CheckStatus::Warn;
                    }
                    detail.push_str("; working tree dirty");
                }
            }

            CheckRow {
                status,
                name: format!("repo {name}"),
                detail,
                remediation: if status == CheckStatus::Fail {
                    Some("Run `logos-scaffold setup`".to_string())
                } else {
                    None
                },
            }
        }
        Err(err) => CheckRow {
            status: CheckStatus::Fail,
            name: format!("repo {name}"),
            detail: err.to_string(),
            remediation: Some("Ensure repo path is valid git repository".to_string()),
        },
    }
}

pub(crate) fn check_path(name: &str, path: &Path, remediation: &str) -> CheckRow {
    if path.exists() {
        CheckRow {
            status: CheckStatus::Pass,
            name: name.to_string(),
            detail: format!("found {}", path.display()),
            remediation: None,
        }
    } else {
        CheckRow {
            status: CheckStatus::Fail,
            name: name.to_string(),
            detail: format!("missing {}", path.display()),
            remediation: Some(remediation.to_string()),
        }
    }
}

pub(crate) fn check_port_warn(name: &str, addr: &str, remediation: &str) -> CheckRow {
    if port_open(addr) {
        CheckRow {
            status: CheckStatus::Pass,
            name: name.to_string(),
            detail: format!("{addr} reachable"),
            remediation: None,
        }
    } else {
        CheckRow {
            status: CheckStatus::Warn,
            name: name.to_string(),
            detail: format!("{addr} not reachable"),
            remediation: Some(remediation.to_string()),
        }
    }
}

pub(crate) fn check_standalone_support(lez_path: &Path) -> CheckRow {
    let files = [
        lez_path.join("Cargo.toml"),
        lez_path.join("sequencer/service/Cargo.toml"),
        lez_path.join("README.md"),
    ];

    for path in files {
        if let Ok(text) = fs::read_to_string(path) {
            if text.contains("standalone") {
                return CheckRow {
                    status: CheckStatus::Pass,
                    name: "standalone support marker".to_string(),
                    detail: "found `standalone` marker in lez repository".to_string(),
                    remediation: None,
                };
            }
        }
    }

    CheckRow {
        status: CheckStatus::Fail,
        name: "standalone support marker".to_string(),
        detail: "could not find `standalone` marker in lez repo".to_string(),
        remediation: Some(format!(
            "Use a logos-execution-zone source that contains standalone mode and pin {}",
            DEFAULT_LEZ_PIN
        )),
    }
}

pub(crate) fn print_rows(rows: &[CheckRow]) {
    println!("STATUS | CHECK | DETAILS");
    println!("-------|-------|--------");

    for row in rows {
        let status = match row.status {
            CheckStatus::Pass => "PASS",
            CheckStatus::Warn => "WARN",
            CheckStatus::Fail => "FAIL",
        };
        println!("{status} | {} | {}", row.name, one_line(&row.detail));
        if matches!(row.status, CheckStatus::Warn | CheckStatus::Fail) {
            if let Some(remediation) = &row.remediation {
                println!("  remediation: {remediation}");
            }
        }
    }
}

pub(crate) fn one_line(text: &str) -> String {
    text.replace('\n', " ").replace('\r', " ")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::model::CheckStatus;

    use super::container_runtime_row;

    #[test]
    fn container_runtime_row_prefers_docker() {
        let row = container_runtime_row(
            Some(PathBuf::from("/usr/local/bin/docker")),
            Some(PathBuf::from("/usr/local/bin/podman")),
        );
        assert_eq!(row.status, CheckStatus::Pass);
        assert!(row.detail.contains("docker"));
    }

    #[test]
    fn container_runtime_row_passes_with_podman_when_docker_missing() {
        let row = container_runtime_row(None, Some(PathBuf::from("/usr/local/bin/podman")));
        assert_eq!(row.status, CheckStatus::Pass);
        assert!(row.detail.contains("podman"));
    }

    #[test]
    fn container_runtime_row_warns_when_missing() {
        let row = container_runtime_row(None, None);
        assert_eq!(row.status, CheckStatus::Warn);
        assert!(row.detail.contains("neither docker nor podman"));
        assert!(row.remediation.is_some());
    }

    #[test]
    fn circuits_version_passes_when_expected() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(
            temp.path().join("VERSION"),
            super::EXPECTED_LOGOS_BLOCKCHAIN_CIRCUITS_VERSION,
        )
        .unwrap();

        let row = super::check_circuits_version(temp.path());
        assert_eq!(row.status, CheckStatus::Pass);
    }

    #[test]
    fn prover_execution_detects_healthy_executable() {
        let temp = tempfile::tempdir().unwrap();
        let prover = temp.path().join("prover");
        std::fs::write(&prover, "#!/bin/sh\necho usage >&2\nexit 1\n").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&prover).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&prover, perms).unwrap();
        }

        let row = super::check_prover_executes(&prover);
        assert_eq!(row.status, CheckStatus::Pass);
    }
}
