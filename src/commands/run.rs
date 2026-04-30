use std::path::PathBuf;
use std::process::Command;

use anyhow::{bail, Context};

use crate::commands::build::cmd_build_shortcut;
use crate::commands::deploy::cmd_deploy;
use crate::commands::idl::build_idl_for_current_project;
use crate::commands::localnet::{
    build_localnet_status_for_project, cmd_localnet, cmd_localnet_reset, LocalnetAction,
};
use crate::commands::wallet::{cmd_wallet_topup_inner, TopupOutcome};
use crate::constants::GUEST_BIN_REL_PATH;
use crate::model::LocalnetOwnership;
use crate::project::load_project;
use crate::DynResult;

/// Number of seconds to wait for block production after a reset before
/// considering the freshly-started localnet healthy. Matches the upper
/// envelope of `cmd_localnet_reset`'s default verification timeout.
const RESET_VERIFY_TIMEOUT_SEC: u64 = 30;

pub(crate) fn cmd_run(
    profile: Option<String>,
    restart_localnet: Option<bool>,
    reset_localnet: Option<bool>,
    post_deploy_override: Option<Vec<String>>,
) -> DynResult<()> {
    let project = load_project()?;
    let resolved = project.config.run.resolve_profile(profile.as_deref())?;
    if let Some(name) = profile.as_deref() {
        println!("Using [run.profiles.{name}]");
    } else if let Some(name) = project.config.run.default_profile.as_deref() {
        println!("Using [run.profiles.{name}] (default_profile)");
    }
    let hooks = post_deploy_override.unwrap_or_else(|| resolved.post_deploy.clone());
    let has_hooks = !hooks.is_empty();
    // Steps: build, build idl, localnet, topup, deploy, [+1 if hooks]
    let total_steps: u32 = if has_hooks { 6 } else { 5 };
    let effective_restart = restart_localnet.unwrap_or(resolved.restart_localnet);
    let effective_reset = reset_localnet.unwrap_or(resolved.reset_localnet);

    // Step 1: Build (chains setup internally)
    println!("[1/{total_steps}] Building...");
    cmd_build_shortcut(None)?;

    // Step 2: Build IDL (no-op for non-lez-framework projects)
    println!("[2/{total_steps}] Building IDL...");
    build_idl_for_current_project()?;

    // Step 3: Reset OR ensure localnet. Reset and restart are orthogonal
    // inputs; when reset is true, restart's value is irrelevant because
    // cmd_localnet_reset already includes a stop+start. Not a conflict.
    if effective_reset {
        println!("[3/{total_steps}] Resetting localnet (wipes sequencer + wallet)...");
        reset_localnet_for_run(&project)?;
    } else {
        println!("[3/{total_steps}] Ensuring localnet...");
        ensure_localnet(&project, effective_restart)?;
    }

    // Step 4: Wallet topup
    println!("[4/{total_steps}] Topping up wallet...");
    let outcome = cmd_wallet_topup_inner(&project, None, false)?;
    if outcome == TopupOutcome::ConfirmationTimeout {
        bail!("wallet topup confirmation timed out; aborting run to avoid deploying with uncertain funding.\nHint: retry `logos-scaffold run` or run `logos-scaffold wallet topup` manually.");
    }

    // Step 5: Deploy
    println!("[5/{total_steps}] Deploying programs...");
    cmd_deploy(None, None, false)?;

    // Step 6: Post-deploy hooks (or summary)
    if has_hooks {
        let n = hooks.len();
        println!("[6/{total_steps}] Running {n} post-deploy hook(s)...");
        for (i, hook) in hooks.iter().enumerate() {
            println!("===> post_deploy[{}/{n}]: {hook}", i + 1);
            run_post_deploy_hook(&project, hook)?;
            println!("<=== post_deploy[{}/{n}] OK", i + 1);
        }
    } else {
        print_deploy_summary(&project)?;
    }

    Ok(())
}

fn reset_localnet_for_run(project: &crate::model::Project) -> DynResult<()> {
    let lez = PathBuf::from(&project.config.lez.path);
    let state_path = project.root.join(".scaffold/state/localnet.state");
    let log_path = project.root.join(".scaffold/logs/sequencer.log");
    let localnet_addr = format!("127.0.0.1:{}", project.config.localnet.port);
    cmd_localnet_reset(
        project,
        &lez,
        &state_path,
        &log_path,
        &localnet_addr,
        true, // reset_wallet — full wipe per design (see docs/specs/run-reset.md)
        RESET_VERIFY_TIMEOUT_SEC,
    )
}

fn ensure_localnet(project: &crate::model::Project, restart: bool) -> DynResult<()> {
    if restart {
        let _ = cmd_localnet(LocalnetAction::Stop);
        cmd_localnet(LocalnetAction::Start { timeout_sec: 20 })?;
        return Ok(());
    }

    let status = build_localnet_status_for_project(project);
    match status.ownership {
        LocalnetOwnership::Managed if status.ready => {
            let pid_display = status
                .tracked_pid
                .map(|p| p.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            println!("      localnet already running (sequencer pid={pid_display})");
            Ok(())
        }
        LocalnetOwnership::Foreign => {
            let pid_display = status
                .listener_pid
                .map(|p| p.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            bail!(
                "localnet port is in use by another process (pid={pid_display}).\n\
                 This may be a sequencer from another project.\n\
                 Stop it first or use `logos-scaffold run --restart-localnet`."
            );
        }
        _ => cmd_localnet(LocalnetAction::Start { timeout_sec: 20 }),
    }
}

fn print_deploy_summary(project: &crate::model::Project) -> DynResult<()> {
    let programs_dir = project.root.join("methods/guest/src/bin");
    if !programs_dir.exists() {
        return Ok(());
    }

    let guest_bin_rel = GUEST_BIN_REL_PATH;

    println!();
    println!("Deployed programs:");
    for entry in std::fs::read_dir(&programs_dir).context("failed to read programs directory")? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let binary_path = project.root.join(guest_bin_rel).join(format!("{stem}.bin"));
        if binary_path.exists() {
            println!("  {stem}");
            println!("    Binary: {}", binary_path.display());
        }
    }

    let port = project.config.localnet.port;
    println!();
    println!("Sequencer: http://127.0.0.1:{port}");

    Ok(())
}

fn build_hook_command(project: &crate::model::Project, hook_command: &str) -> Command {
    let port = project.config.localnet.port;
    let sequencer_url = format!("http://127.0.0.1:{port}");
    let wallet_home = project
        .root
        .join(&project.config.wallet_home_dir)
        .canonicalize()
        .unwrap_or_else(|_| project.root.join(&project.config.wallet_home_dir));
    let project_root = project
        .root
        .canonicalize()
        .unwrap_or_else(|_| project.root.clone());
    let idl_dir = project
        .root
        .join(&project.config.framework.idl.path)
        .canonicalize()
        .unwrap_or_else(|_| project.root.join(&project.config.framework.idl.path));

    let mut cmd = Command::new("sh");
    cmd.arg("-c")
        .arg(hook_command)
        .env("SEQUENCER_URL", &sequencer_url)
        .env("NSSA_WALLET_HOME_DIR", &wallet_home)
        .env("SCAFFOLD_PROJECT_ROOT", &project_root)
        .env("SCAFFOLD_IDL_DIR", &idl_dir)
        .current_dir(&project.root);
    cmd
}

fn run_post_deploy_hook(project: &crate::model::Project, hook_command: &str) -> DynResult<()> {
    let status = build_hook_command(project, hook_command)
        .status()
        .context("failed to execute post-deploy hook")?;

    if !status.success() {
        let code = status.code().unwrap_or(1);
        bail!("post-deploy hook exited with status {code}");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        Config, FrameworkConfig, FrameworkIdlConfig, LocalnetConfig, Project, RepoRef, RunConfig,
    };
    use std::path::PathBuf;

    fn make_test_project(root: PathBuf) -> Project {
        Project {
            root,
            config: Config {
                version: "0.1.0".to_string(),
                cache_root: ".scaffold/cache".to_string(),
                lez: RepoRef {
                    url: "https://example.com/lez.git".to_string(),
                    source: "lez".to_string(),
                    path: "lez".to_string(),
                    pin: "abc123".to_string(),
                },
                wallet_home_dir: ".scaffold/wallet".to_string(),
                framework: FrameworkConfig {
                    kind: "default".to_string(),
                    version: "0.1.0".to_string(),
                    idl: FrameworkIdlConfig {
                        spec: "lssa-idl/0.1.0".to_string(),
                        path: "idl".to_string(),
                    },
                },
                localnet: LocalnetConfig {
                    port: 3040,
                    risc0_dev_mode: true,
                },
                run: RunConfig::default(),
                basecamp: None,
            },
        }
    }

    #[test]
    fn hook_receives_sequencer_url_env() {
        let temp = tempfile::tempdir().expect("tempdir");
        let env_file = temp.path().join("env_out.txt");
        let project = make_test_project(temp.path().to_path_buf());

        let hook = format!("echo \"$SEQUENCER_URL\" > '{}'", env_file.display());
        run_post_deploy_hook(&project, &hook).expect("hook should succeed");

        let content = std::fs::read_to_string(&env_file).expect("read env output");
        assert_eq!(content.trim(), "http://127.0.0.1:3040");
    }

    #[test]
    fn hook_receives_wallet_home_dir_env() {
        let temp = tempfile::tempdir().expect("tempdir");
        let env_file = temp.path().join("env_out.txt");
        let project = make_test_project(temp.path().to_path_buf());

        let hook = format!("echo \"$NSSA_WALLET_HOME_DIR\" > '{}'", env_file.display());
        run_post_deploy_hook(&project, &hook).expect("hook should succeed");

        let content = std::fs::read_to_string(&env_file).expect("read env output");
        assert!(
            content.trim().ends_with(".scaffold/wallet"),
            "expected wallet home to end with .scaffold/wallet, got: {content}"
        );
    }

    #[test]
    fn hook_receives_project_root_env() {
        let temp = tempfile::tempdir().expect("tempdir");
        let env_file = temp.path().join("env_out.txt");
        let project = make_test_project(temp.path().to_path_buf());

        let hook = format!("echo \"$SCAFFOLD_PROJECT_ROOT\" > '{}'", env_file.display());
        run_post_deploy_hook(&project, &hook).expect("hook should succeed");

        let content = std::fs::read_to_string(&env_file).expect("read env output");
        let canonical = temp
            .path()
            .canonicalize()
            .unwrap_or_else(|_| temp.path().to_path_buf());
        assert_eq!(content.trim(), canonical.display().to_string());
    }

    #[test]
    fn hook_receives_idl_dir_env() {
        let temp = tempfile::tempdir().expect("tempdir");
        let env_file = temp.path().join("env_out.txt");
        let project = make_test_project(temp.path().to_path_buf());

        let hook = format!("echo \"$SCAFFOLD_IDL_DIR\" > '{}'", env_file.display());
        run_post_deploy_hook(&project, &hook).expect("hook should succeed");

        let content = std::fs::read_to_string(&env_file).expect("read env output");
        assert!(
            content.trim().ends_with("/idl"),
            "expected IDL dir to end with /idl, got: {content}"
        );
    }

    #[test]
    fn hook_uses_custom_port() {
        let temp = tempfile::tempdir().expect("tempdir");
        let env_file = temp.path().join("env_out.txt");
        let mut project = make_test_project(temp.path().to_path_buf());
        project.config.localnet.port = 9999;

        let hook = format!("echo \"$SEQUENCER_URL\" > '{}'", env_file.display());
        run_post_deploy_hook(&project, &hook).expect("hook should succeed");

        let content = std::fs::read_to_string(&env_file).expect("read env output");
        assert_eq!(content.trim(), "http://127.0.0.1:9999");
    }

    #[test]
    fn hook_failure_propagates_as_error() {
        let temp = tempfile::tempdir().expect("tempdir");
        let project = make_test_project(temp.path().to_path_buf());

        let result = run_post_deploy_hook(&project, "exit 42");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("42"),
            "expected exit code 42 in error, got: {msg}"
        );
    }

    #[test]
    fn hook_runs_in_project_root_directory() {
        let temp = tempfile::tempdir().expect("tempdir");
        let pwd_file = temp.path().join("pwd_out.txt");
        let project = make_test_project(temp.path().to_path_buf());

        let hook = format!("pwd > '{}'", pwd_file.display());
        run_post_deploy_hook(&project, &hook).expect("hook should succeed");

        let content = std::fs::read_to_string(&pwd_file).expect("read pwd output");
        let canonical = temp
            .path()
            .canonicalize()
            .unwrap_or_else(|_| temp.path().to_path_buf());
        assert_eq!(content.trim(), canonical.display().to_string());
    }

    #[test]
    fn print_deploy_summary_shows_programs() {
        let temp = tempfile::tempdir().expect("tempdir");
        let project = make_test_project(temp.path().to_path_buf());

        let programs_dir = temp.path().join("methods/guest/src/bin");
        std::fs::create_dir_all(&programs_dir).expect("create programs dir");
        std::fs::write(programs_dir.join("counter.rs"), "fn main() {}").expect("write source");

        let binary_dir = temp.path().join(GUEST_BIN_REL_PATH);
        std::fs::create_dir_all(&binary_dir).expect("create binary dir");
        std::fs::write(binary_dir.join("counter.bin"), b"fake binary").expect("write binary");

        print_deploy_summary(&project).expect("should succeed");
    }

    #[test]
    fn print_deploy_summary_skips_non_rs_files() {
        let temp = tempfile::tempdir().expect("tempdir");
        let project = make_test_project(temp.path().to_path_buf());

        let programs_dir = temp.path().join("methods/guest/src/bin");
        std::fs::create_dir_all(&programs_dir).expect("create programs dir");
        std::fs::write(programs_dir.join("README.md"), "# readme").expect("write non-rs file");

        print_deploy_summary(&project).expect("should succeed with no .rs files");
    }

    #[test]
    fn print_deploy_summary_returns_ok_when_no_programs_dir() {
        let temp = tempfile::tempdir().expect("tempdir");
        let project = make_test_project(temp.path().to_path_buf());

        print_deploy_summary(&project).expect("should succeed with missing dir");
    }
}
