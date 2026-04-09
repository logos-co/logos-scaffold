use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Context};

use crate::error::LocalnetError;
use crate::model::{LocalnetOwnership, LocalnetState, LocalnetStatusReport, Project};
use crate::process::{listener_pid, pid_alive, pid_command, pid_running, port_open, spawn_to_log};
use crate::project::{ensure_dir_exists, find_project_root, load_project};
use crate::state::{read_localnet_state, write_localnet_state};
use crate::DynResult;

// LOCALNET_ADDR is now read from project config (localnet.port)

#[derive(Debug, Clone, Copy)]
pub(crate) enum LocalnetAction {
    Start { timeout_sec: u64 },
    Stop,
    Status { json: bool },
    Logs { tail: usize },
}

pub(crate) fn cmd_localnet(action: LocalnetAction) -> DynResult<()> {
    match action {
        LocalnetAction::Stop => {
            let cwd = env::current_dir()?;
            if find_project_root(cwd).is_some() {
                let project = load_project()?;
                cmd_localnet_in_project(&project, action)
            } else {
                cmd_localnet_stop_outside_project()
            }
        }
        _ => {
            let project = load_project()?;
            cmd_localnet_in_project(&project, action)
        }
    }
}

pub(crate) fn build_localnet_status_for_project(project: &Project) -> LocalnetStatusReport {
    let state_path = project.root.join(".scaffold/state/localnet.state");
    let log_path = project.root.join(".scaffold/logs/sequencer.log");
    build_status_report(
        &state_path,
        &log_path,
        &format!("127.0.0.1:{}", project.config.localnet.port),
        project.config.localnet.port,
    )
}

fn cmd_localnet_in_project(project: &Project, action: LocalnetAction) -> DynResult<()> {
    let localnet_port = project.config.localnet.port;
    let risc0_dev_mode = project.config.localnet.risc0_dev_mode;
    let sequencer_binary = &project.config.localnet.sequencer_binary;
    let sequencer_config_path = &project.config.localnet.sequencer_config_path;
    let localnet_addr = format!("127.0.0.1:{localnet_port}");
    let lssa = PathBuf::from(&project.config.lssa.path);
    let state_path = project.root.join(".scaffold/state/localnet.state");
    let logs_dir = project.root.join(".scaffold/logs");
    let log_path = logs_dir.join("sequencer.log");
    fs::create_dir_all(&logs_dir)?;

    match action {
        LocalnetAction::Start { timeout_sec } => cmd_localnet_start(
            &lssa,
            &state_path,
            &log_path,
            timeout_sec,
            localnet_port,
            risc0_dev_mode,
            sequencer_binary,
            sequencer_config_path,
            &localnet_addr,
        ),
        LocalnetAction::Stop => cmd_localnet_stop(&state_path, localnet_port),
        LocalnetAction::Status { json } => {
            cmd_localnet_status(&state_path, &log_path, json, &localnet_addr, localnet_port)
        }
        LocalnetAction::Logs { tail } => cmd_localnet_logs(&log_path, tail),
    }
}

fn cmd_localnet_stop_outside_project() -> DynResult<()> {
    let default_addr = "127.0.0.1:3040";
    let default_port: u16 = 3040;
    if !port_open(default_addr) {
        println!("localnet not running (no listener on {default_addr})");
        return Ok(());
    }

    let listener_pid = listener_pid(default_port);
    let pid_text = listener_pid
        .map(|pid| pid.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    println!("listener detected on {default_addr} (pid={pid_text})");
    println!(
        "This command is running outside a logos-scaffold project; it will not stop unmanaged processes automatically."
    );
    println!(
        "This may be a sequencer started from another project and may not match your current workspace."
    );

    if let Some(pid) = listener_pid {
        if let Some(command) = pid_command(pid) {
            println!("listener process: {command}");
        }
        println!("Try: kill {pid}");
    } else {
        println!("Try: lsof -nP -iTCP:{default_port} -sTCP:LISTEN");
    }

    Ok(())
}

fn cmd_localnet_start(
    lssa: &Path,
    state_path: &Path,
    log_path: &Path,
    timeout_sec: u64,
    localnet_port: u16,
    risc0_dev_mode: bool,
    sequencer_binary: &str,
    sequencer_config_path: &str,
    localnet_addr: &str,
) -> DynResult<()> {
    ensure_dir_exists(lssa, "lssa")?;
    let sequencer_bin = lssa.join("target/release").join(sequencer_binary);
    if !sequencer_bin.exists() {
        return Err(LocalnetError::MissingSequencerBinary {
            path: sequencer_bin.display().to_string(),
        }
        .into());
    }

    let mut state = read_localnet_state(state_path).unwrap_or_default();
    if let Some(pid) = state.sequencer_pid {
        if pid_running(pid) {
            wait_for_readiness(pid, timeout_sec, log_path, localnet_addr)?;
            println!("localnet ready (sequencer pid={pid})");
            return Ok(());
        }

        if state_path.exists() {
            fs::remove_file(state_path)?;
        }
        state = LocalnetState::default();
    }

    let existing_listener_pid = listener_pid(localnet_port);
    if port_open(localnet_addr) {
        let mut message = match existing_listener_pid {
            Some(pid) => {
                format!("cannot start localnet: port {localnet_port} is already in use (pid={pid})")
            }
            None => format!(
                "cannot start localnet: port {localnet_port} is already in use (pid=unknown)"
            ),
        };
        message.push_str(
            "\nThis may be a sequencer started from another project and may not work with the current project.",
        );
        message.push_str("\nStop that process and retry `logos-scaffold localnet start`.");
        if let Some(pid) = existing_listener_pid {
            message.push_str(&format!("\nTry: kill {pid}"));
        }
        bail!("{message}");
    }

    let sequencer_pid = spawn_to_log(
        Command::new(sequencer_bin)
            .current_dir(lssa)
            .arg(sequencer_config_path)
            .arg("--port")
            .arg(localnet_port.to_string())
            .env("RUST_LOG", "info")
            .env("RISC0_DEV_MODE", if risc0_dev_mode { "1" } else { "0" }),
        log_path,
    )?;

    state.sequencer_pid = Some(sequencer_pid);
    write_localnet_state(state_path, &state)?;

    if let Err(err) = wait_for_readiness(sequencer_pid, timeout_sec, log_path, localnet_addr) {
        if pid_alive(sequencer_pid) {
            let _ = Command::new("kill").arg(sequencer_pid.to_string()).status();
        }
        if state_path.exists() {
            let _ = fs::remove_file(state_path);
        }
        return Err(err);
    }

    println!("localnet ready (sequencer pid={sequencer_pid})");
    Ok(())
}

fn wait_for_readiness(
    pid: u32,
    timeout_sec: u64,
    log_path: &Path,
    localnet_addr: &str,
) -> DynResult<()> {
    let deadline = Instant::now() + Duration::from_secs(timeout_sec.max(1));

    loop {
        let running = pid_running(pid);
        let ready = running && port_open(localnet_addr);
        if ready {
            return Ok(());
        }

        if !running {
            return Err(LocalnetError::ExitedBeforeReady {
                pid,
                log_tail: read_log_tail(log_path, 60),
            }
            .into());
        }

        if Instant::now() >= deadline {
            return Err(LocalnetError::StartTimeout {
                timeout_sec,
                pid,
                log_tail: read_log_tail(log_path, 60),
            }
            .into());
        }

        thread::sleep(Duration::from_millis(200));
    }
}

fn cmd_localnet_stop(state_path: &Path, localnet_port: u16) -> DynResult<()> {
    let localnet_addr = format!("127.0.0.1:{localnet_port}");
    let report = build_status_report(
        state_path,
        Path::new(".scaffold/logs/sequencer.log"),
        &localnet_addr,
        localnet_port,
    );
    if let Some(pid) = report.tracked_pid {
        if report.tracked_running {
            println!("$ kill {pid} # sequencer");
            let _ = Command::new("kill").arg(pid.to_string()).status();
        } else {
            println!("sequencer state is stale (pid={pid} not running)");
        }

        if state_path.exists() {
            fs::remove_file(state_path)?;
        }
        println!("localnet stopped");
        return Ok(());
    }

    if report.listener_present {
        let pid_text = report
            .listener_pid
            .map(|pid| pid.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        println!(
            "foreign listener detected on {localnet_addr} (pid={pid_text}); not stopping unmanaged process"
        );
        return Ok(());
    }

    println!("localnet not running");
    Ok(())
}

fn cmd_localnet_status(
    state_path: &Path,
    log_path: &Path,
    as_json: bool,
    localnet_addr: &str,
    localnet_port: u16,
) -> DynResult<()> {
    let report = build_status_report(state_path, log_path, localnet_addr, localnet_port);

    if as_json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    if let Some(pid) = report.tracked_pid {
        println!(
            "tracked sequencer: pid={pid} running={}",
            report.tracked_running
        );
    } else {
        println!("tracked sequencer: not tracked");
    }

    if report.listener_present {
        let pid_text = report
            .listener_pid
            .map(|pid| pid.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        println!("listener {localnet_addr}: reachable (pid={pid_text})");
    } else {
        println!("listener {localnet_addr}: not reachable");
    }

    println!("ownership: {}", ownership_label(report.ownership));
    println!("ready: {}", report.ready);
    if !report.remediation.is_empty() {
        println!("next steps:");
        for step in &report.remediation {
            println!("- {step}");
        }
    }

    Ok(())
}

fn ownership_label(ownership: LocalnetOwnership) -> &'static str {
    match ownership {
        LocalnetOwnership::Managed => "managed",
        LocalnetOwnership::Foreign => "foreign",
        LocalnetOwnership::StaleState => "stale_state",
        LocalnetOwnership::ManagedNotReady => "managed_not_ready",
        LocalnetOwnership::Stopped => "stopped",
    }
}

fn cmd_localnet_logs(log_path: &Path, tail: usize) -> DynResult<()> {
    if !log_path.exists() {
        println!("log file does not exist yet: {}", log_path.display());
        return Ok(());
    }

    let content = fs::read_to_string(log_path)
        .with_context(|| format!("failed to read log file {}", log_path.display()))?;

    if content.trim().is_empty() {
        println!("log file is empty: {}", log_path.display());
        return Ok(());
    }

    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(tail);
    for line in &lines[start..] {
        println!("{line}");
    }

    Ok(())
}

fn build_status_report(
    state_path: &Path,
    log_path: &Path,
    localnet_addr: &str,
    localnet_port: u16,
) -> LocalnetStatusReport {
    let state = read_localnet_state(state_path).unwrap_or_default();
    let tracked_pid = state.sequencer_pid;
    let tracked_running = tracked_pid.map(pid_running).unwrap_or(false);
    let listener_present = port_open(localnet_addr);
    let listener_pid = if listener_present {
        listener_pid(localnet_port)
    } else {
        None
    };

    let ownership = match (tracked_pid, tracked_running, listener_present) {
        (Some(pid), true, true) => match listener_pid {
            Some(listener) if listener == pid => LocalnetOwnership::Managed,
            Some(_) => LocalnetOwnership::Foreign,
            None => LocalnetOwnership::ManagedNotReady,
        },
        (Some(_), true, false) => LocalnetOwnership::ManagedNotReady,
        (Some(_), false, _) => LocalnetOwnership::StaleState,
        (None, _, true) => LocalnetOwnership::Foreign,
        (None, _, false) => LocalnetOwnership::Stopped,
    };

    let ready = tracked_running && listener_present;
    let remediation = match ownership {
        LocalnetOwnership::Managed if ready => vec![],
        LocalnetOwnership::Managed => {
            vec!["Wait a moment and re-run `logos-scaffold localnet status`".to_string()]
        }
        LocalnetOwnership::ManagedNotReady => vec![
            "Run `logos-scaffold localnet logs --tail 200` to inspect startup issues".to_string(),
            "Run `logos-scaffold localnet stop` then `logos-scaffold localnet start`".to_string(),
        ],
        LocalnetOwnership::StaleState => vec![
            "Run `logos-scaffold localnet stop` to clean stale state".to_string(),
            "Run `logos-scaffold localnet start` to restart localnet".to_string(),
        ],
        LocalnetOwnership::Foreign => vec![
            format!("Stop the external listener on {localnet_addr} or choose a clean environment"),
            "Then run `logos-scaffold localnet start`".to_string(),
        ],
        LocalnetOwnership::Stopped => vec!["Run `logos-scaffold localnet start`".to_string()],
    };

    LocalnetStatusReport {
        tracked_pid,
        tracked_running,
        listener_present,
        listener_pid,
        ownership,
        ready,
        log_path: log_path.display().to_string(),
        remediation,
    }
}

fn read_log_tail(log_path: &Path, tail: usize) -> String {
    let Ok(content) = fs::read_to_string(log_path) else {
        return format!("<log file missing: {}>", log_path.display());
    };

    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return "<no log output yet>".to_string();
    }

    let start = lines.len().saturating_sub(tail);
    lines[start..].join("\n")
}
