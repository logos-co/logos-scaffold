use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::Duration;

use crate::model::LocalnetState;
use crate::process::{pid_alive, port_open, spawn_to_log};
use crate::project::{ensure_dir_exists, load_project};
use crate::state::{read_localnet_state, write_localnet_state};
use crate::DynResult;

pub(crate) fn cmd_localnet(args: &[String]) -> DynResult<()> {
    if args.is_empty() {
        return Err("usage: logos-scaffold localnet <start|stop|status|logs> ...".into());
    }

    let project = load_project()?;
    let lssa = PathBuf::from(&project.config.lssa.path);
    let state_path = project.root.join(".scaffold/state/localnet.state");
    let logs_dir = project.root.join(".scaffold/logs");
    fs::create_dir_all(&logs_dir)?;

    match args[0].as_str() {
        "start" => {
            if args.len() != 1 {
                return Err("usage: logos-scaffold localnet start".into());
            }

            ensure_dir_exists(&lssa, "lssa")?;
            let sequencer_bin = lssa.join("target/release/sequencer_runner");
            if !sequencer_bin.exists() {
                return Err(format!(
                    "missing sequencer binary {}; run `logos-scaffold setup`",
                    sequencer_bin.display()
                )
                .into());
            }

            let state = read_localnet_state(&state_path).unwrap_or_default();
            if let Some(pid) = state.sequencer_pid {
                if pid_alive(pid) {
                    println!("sequencer already running with pid={pid}");
                    return Ok(());
                }
            }

            let sequencer_pid = spawn_to_log(
                Command::new(sequencer_bin)
                    .current_dir(&lssa)
                    .arg("sequencer_runner/configs/debug")
                    .env("RUST_LOG", "info")
                    .env("RISC0_DEV_MODE", "1"),
                &logs_dir.join("sequencer.log"),
            )?;

            let state = LocalnetState {
                sequencer_pid: Some(sequencer_pid),
            };
            write_localnet_state(&state_path, &state)?;

            thread::sleep(Duration::from_secs(2));
            println!("localnet start requested (sequencer pid={sequencer_pid})");
        }
        "stop" => {
            if args.len() != 1 {
                return Err("usage: logos-scaffold localnet stop".into());
            }

            let state = read_localnet_state(&state_path).unwrap_or_default();
            if let Some(pid) = state.sequencer_pid {
                println!("$ kill {pid} # sequencer");
                let _ = Command::new("kill").arg(pid.to_string()).status();
            } else {
                println!("no localnet state found");
            }

            if state_path.exists() {
                fs::remove_file(state_path)?;
            }
        }
        "status" => {
            if args.len() != 1 {
                return Err("usage: logos-scaffold localnet status".into());
            }

            let state = read_localnet_state(&state_path).unwrap_or_default();
            if let Some(pid) = state.sequencer_pid {
                println!("sequencer: pid={pid} running={}", pid_alive(pid));
            } else {
                println!("sequencer: not tracked (state missing)");
            }
            println!("port 3040 sequencer: {}", port_open("127.0.0.1:3040"));
        }
        "logs" => {
            let mut tail: usize = 200;
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--tail" => {
                        let value = args.get(i + 1).ok_or("--tail requires value")?;
                        tail = value
                            .parse::<usize>()
                            .map_err(|_| "--tail expects positive integer")?;
                        i += 2;
                    }
                    other => return Err(format!("unknown flag for localnet logs: {other}").into()),
                }
            }

            let log_path = logs_dir.join("sequencer.log");
            if !log_path.exists() {
                return Err(format!("missing log file: {}", log_path.display()).into());
            }

            let content = fs::read_to_string(log_path)?;
            let lines: Vec<&str> = content.lines().collect();
            let start = lines.len().saturating_sub(tail);
            for line in &lines[start..] {
                println!("{line}");
            }
        }
        other => return Err(format!("unknown localnet command: {other}").into()),
    }

    Ok(())
}
