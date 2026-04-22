use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context};

use crate::process::run_with_stdin;
use crate::project::load_project;
use crate::DynResult;

use super::wallet_support::{
    extract_tx_identifier, is_connectivity_failure, load_wallet_runtime, rpc_get_last_block,
    rpc_get_program_ids, sequencer_unreachable_hint, summarize_command_failure, wallet_password,
    RpcReachabilityError,
};

const GUEST_BIN_REL_PATH: &str =
    "target/riscv-guest/example_program_deployment_methods/example_program_deployment_programs/riscv32im-risc0-zkvm-elf/release";

/// Dynamically discover the binary path for a program by searching methods/target/
/// for .bin files matching the program name. Falls back to GUEST_BIN_REL_PATH.
fn find_binary_path(project_root: &Path, program: &str) -> Option<PathBuf> {
    let methods_target = project_root.join("methods/target");
    if !methods_target.exists() {
        return None;
    }

    // Walk methods/target/ looking for <program>.bin in riscv32im paths
    let pattern = format!("{}.bin", program);
    let walker = walkdir::WalkDir::new(&methods_target)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file());

    for entry in walker {
        let path = entry.path();
        if path.file_name().and_then(|f| f.to_str()) == Some(pattern.as_str()) {
            if path.to_string_lossy().contains("riscv32im") {
                return Some(path.to_path_buf());
            }
        }
    }
    None
}

const DEFAULT_SEQUENCER_ADDR: &str = "http://127.0.0.1:3040";

pub(crate) fn cmd_deploy(
    program_name: Option<String>,
    program_path: Option<PathBuf>,
    json: bool,
) -> DynResult<()> {
    let project = load_project().context(
        "This command must be run inside a logos-scaffold project.\nNext step: cd into your scaffolded project directory and retry.",
    )?;
    let wallet = load_wallet_runtime(&project)?;

    let sequencer_addr = wallet
        .sequencer_addr
        .clone()
        .unwrap_or_else(|| DEFAULT_SEQUENCER_ADDR.to_string());

    // --program-path: deploy a single custom ELF directly, skip auto-discovery
    if let Some(custom_path) = program_path {
        if !custom_path.exists() {
            bail!("program binary not found at `{}`", custom_path.display());
        }
        let program_name = custom_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();
        return deploy_single_program(&wallet, &program_name, &custom_path, &sequencer_addr, json);
    }

    let available_programs = discover_deployable_programs(&project.root)?;
    if available_programs.is_empty() {
        bail!(
            "no deployable programs found in `{}`",
            project.root.join("methods/guest/src/bin").display()
        );
    }

    let selected_programs = resolve_selected_programs(program_name, &available_programs)?;
    let binaries_root = project.root.join(GUEST_BIN_REL_PATH);

    preflight_sequencer_reachability(&sequencer_addr)?;

    let mut results = Vec::new();
    for program in selected_programs {
        // Try dynamic discovery first, fall back to hardcoded path
        let binary_path = find_binary_path(&project.root, &program)
            .unwrap_or_else(|| binaries_root.join(format!("{program}.bin")));

        if !binary_path.exists() {
            println!("FAIL {program} deployment failed");
            println!("  Error: missing binary at {}", binary_path.display());
            println!("  Hint: run `logos-scaffold build` first.");
            results.push(DeployResult {
                program,
                status: DeployStatus::Failed,
                detail: "missing program binary".to_string(),
                tx: None,
            });
            continue;
        }

        let mut command = Command::new(&wallet.wallet_binary);
        command
            .env(
                "NSSA_WALLET_HOME_DIR",
                wallet.wallet_home.as_os_str().to_string_lossy().to_string(),
            )
            .arg("deploy-program")
            .arg(&binary_path);

        let output = match run_with_stdin(command, format!("{}\n", wallet_password())) {
            Ok(output) => output,
            Err(err) => {
                println!("FAIL {program} deployment failed");
                println!("  Error: failed to execute wallet command: {err}");
                results.push(DeployResult {
                    program,
                    status: DeployStatus::Failed,
                    detail: format!("wallet command invocation failed: {err}"),
                    tx: None,
                });
                continue;
            }
        };

        let tx = extract_tx_identifier(&output.stdout, &output.stderr);

        if !output.status.success() {
            let summary = summarize_command_failure(&output.stdout, &output.stderr);
            let combined = format!("{}\n{}", output.stdout, output.stderr);
            println!("FAIL {program} deployment failed");
            println!("  Error: {summary}");
            if is_connectivity_failure(&combined) {
                println!("  Hint: {}", sequencer_unreachable_hint(&sequencer_addr));
                results.push(DeployResult {
                    program,
                    status: DeployStatus::Failed,
                    detail: format!("{summary}; sequencer connectivity failure"),
                    tx,
                });
            } else {
                println!("  Hint: inspect sequencer logs and retry.");
                results.push(DeployResult {
                    program,
                    status: DeployStatus::Failed,
                    detail: summary,
                    tx,
                });
            }
            continue;
        }

        println!("OK  {program} submitted");
        if let Some(tx) = tx.clone() {
            println!("  Tx: {tx}");
        }

        results.push(DeployResult {
            program,
            status: DeployStatus::Submitted,
            detail: "wallet submission command exited successfully".to_string(),
            tx,
        });
    }

    // Fetch program IDs after all deployments complete
    let program_ids = rpc_get_program_ids(&sequencer_addr).unwrap_or_default();
    for result in &results {
        if matches!(result.status, DeployStatus::Submitted) {
            if let Some(id) = program_ids.get(&result.program) {
                println!("  {} → Program ID: {id}", result.program);
            }
        }
    }

    let success_count = results
        .iter()
        .filter(|result| matches!(result.status, DeployStatus::Submitted))
        .count();
    let failed_count = results
        .iter()
        .filter(|result| matches!(result.status, DeployStatus::Failed))
        .count();

    println!("Note: Submission confirmed by wallet exit status; deploy inclusion receipt is not currently exposed by LEZ wallet/RPC for scaffold.");
    println!("Summary:");
    println!("  Succeeded: {success_count}");
    println!("  Failed: {failed_count}");
    println!("  Results:");
    for result in &results {
        let mut line = format!("    {}: {}", result.program, result.status.label());
        if let Some(tx) = &result.tx {
            line.push_str(&format!(" (tx: {tx})"));
        }
        println!("{line}");
        println!("      {}", result.detail);
    }

    if failed_count > 0 {
        bail!("deploy completed with {failed_count} failed program(s)");
    }

    Ok(())
}

fn preflight_sequencer_reachability(sequencer_addr: &str) -> DynResult<()> {
    match rpc_get_last_block(sequencer_addr) {
        Ok(_) => Ok(()),
        Err(RpcReachabilityError::Connectivity(err)) => {
            bail!(
                "cannot deploy programs: {err}\n{}",
                sequencer_unreachable_hint(sequencer_addr)
            )
        }
        Err(err) => {
            println!(
                "warning: sequencer reachability probe failed ({err}); continuing with wallet submission mode"
            );
            Ok(())
        }
    }
}

fn discover_deployable_programs(project_root: &Path) -> DynResult<Vec<String>> {
    let programs_dir = project_root.join("methods/guest/src/bin");
    if !programs_dir.exists() {
        bail!(
            "missing deployable program directory at {}",
            programs_dir.display()
        );
    }

    let mut programs = Vec::new();
    for entry in fs::read_dir(&programs_dir)
        .with_context(|| format!("failed to read {}", programs_dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        programs.push(stem.to_string());
    }

    programs.sort();
    Ok(programs)
}

fn resolve_selected_programs(
    requested_program: Option<String>,
    available_programs: &[String],
) -> DynResult<Vec<String>> {
    if requested_program.is_none() {
        return Ok(available_programs.to_vec());
    }

    let raw = requested_program.unwrap_or_default();
    let candidate = raw.trim().trim_end_matches(".bin").to_string();
    if candidate.is_empty() {
        bail!("program name cannot be empty");
    }

    if available_programs
        .iter()
        .any(|program| program == &candidate)
    {
        return Ok(vec![candidate]);
    }

    bail!(
        "unknown program `{candidate}`. Available programs: {}",
        available_programs.join(", ")
    )
}

fn deploy_single_program(
    wallet: &super::wallet_support::WalletRuntimeContext,
    program_name: &str,
    binary_path: &Path,
    sequencer_addr: &str,
    json: bool,
) -> DynResult<()> {
    preflight_sequencer_reachability(sequencer_addr)?;

    let mut command = std::process::Command::new(&wallet.wallet_binary);
    command
        .env(
            "NSSA_WALLET_HOME_DIR",
            wallet.wallet_home.as_os_str().to_string_lossy().to_string(),
        )
        .arg("deploy-program")
        .arg(binary_path);

    let output = run_with_stdin(
        command,
        format!(
            "{}
",
            wallet_password()
        ),
    )
    .context("failed to execute wallet deploy-program command")?;

    let tx = extract_tx_identifier(&output.stdout, &output.stderr);

    if !output.status.success() {
        let summary = summarize_command_failure(&output.stdout, &output.stderr);
        if json {
            eprintln!(
                "{{\"status\":\"failed\",\"program\":\"{}\",\"error\":\"{}\"}}",
                program_name, summary
            );
        } else {
            println!("FAIL {program_name} deployment failed");
            println!("  Error: {summary}");
        }
        bail!("deploy failed: {summary}");
    }

    let program_id = rpc_get_program_ids(sequencer_addr)
        .and_then(|ids| ids.get(program_name).cloned());

    if json {
        let json_out = serde_json::json!({
            "status": "submitted",
            "program": program_name,
            "tx": tx.as_deref(),
            "program_id": program_id.as_deref(),
        });
        println!("{}", serde_json::to_string(&json_out).unwrap_or_default());
    } else {
        println!("OK  {program_name} submitted");
        println!("  Binary: {}", binary_path.display());
        if let Some(tx) = &tx {
            println!("  Tx: {tx}");
        }
        if let Some(id) = &program_id {
            println!("  Program ID: {id}");
        }
    }

    Ok(())
}

#[derive(Clone, Debug)]
struct DeployResult {
    program: String,
    status: DeployStatus,
    detail: String,
    tx: Option<String>,
}

#[derive(Clone, Debug)]
enum DeployStatus {
    Submitted,
    Failed,
}

impl DeployStatus {
    fn label(&self) -> &'static str {
        match self {
            DeployStatus::Submitted => "submitted",
            DeployStatus::Failed => "failed",
        }
    }
}

#[cfg(test)]
mod deploy_tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn find_binary_path_finds_bin_in_riscv32im_path() {
        let temp = tempdir().unwrap();
        let bin_dir = temp.path()
            .join("methods/target/riscv-guest/my_methods/my_programs/riscv32im-risc0-zkvm-elf/release");
        fs::create_dir_all(&bin_dir).unwrap();
        fs::write(bin_dir.join("my_program.bin"), b"fake elf").unwrap();

        let result = find_binary_path(temp.path(), "my_program");
        assert!(result.is_some(), "should find binary in riscv32im path");
        assert!(result.unwrap().ends_with("my_program.bin"));
    }

    #[test]
    fn find_binary_path_returns_none_when_methods_target_missing() {
        let temp = tempdir().unwrap();
        let result = find_binary_path(temp.path(), "my_program");
        assert!(result.is_none(), "should return None when methods/target missing");
    }

    #[test]
    fn find_binary_path_returns_none_when_no_matching_bin() {
        let temp = tempdir().unwrap();
        let bin_dir = temp.path()
            .join("methods/target/riscv-guest/my_methods/riscv32im-risc0-zkvm-elf/release");
        fs::create_dir_all(&bin_dir).unwrap();
        fs::write(bin_dir.join("other_program.bin"), b"fake elf").unwrap();

        let result = find_binary_path(temp.path(), "my_program");
        assert!(result.is_none(), "should return None when no matching bin");
    }

    #[test]
    fn find_binary_path_ignores_non_riscv32im_paths() {
        let temp = tempdir().unwrap();
        // Put binary in non-riscv32im path
        let bin_dir = temp.path().join("methods/target/debug");
        fs::create_dir_all(&bin_dir).unwrap();
        fs::write(bin_dir.join("my_program.bin"), b"fake elf").unwrap();

        let result = find_binary_path(temp.path(), "my_program");
        assert!(result.is_none(), "should ignore non-riscv32im paths");
    }
}
