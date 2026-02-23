use std::process::Command;

use anyhow::{bail, Context};

use crate::constants::DEFAULT_WALLET_PASSWORD;
use crate::process::{render_command, run_forwarded, run_with_stdin};
use crate::project::load_project;
use crate::state::write_text;
use crate::DynResult;

use super::wallet_support::{
    extract_tx_identifier, is_connectivity_failure, load_wallet_runtime, normalize_address_ref,
    read_default_wallet_address, resolve_wallet_address, sequencer_unreachable_hint,
    summarize_command_failure,
};

#[derive(Debug, Clone)]
pub(crate) enum WalletAction {
    List {
        long: bool,
    },
    Proxy {
        args: Vec<String>,
    },
    Topup {
        address: Option<String>,
        dry_run: bool,
    },
    DefaultSet {
        address: String,
    },
}

pub(crate) fn cmd_wallet(action: WalletAction) -> DynResult<()> {
    let project = load_project().context(
        "This command must be run inside a logos-scaffold project.\nNext step: cd into your scaffolded project directory and retry.",
    )?;

    match action {
        WalletAction::List { long } => cmd_wallet_list(&project, long),
        WalletAction::Proxy { args } => cmd_wallet_proxy(&project, &args),
        WalletAction::Topup { address, dry_run } => cmd_wallet_topup(&project, address, dry_run),
        WalletAction::DefaultSet { address } => cmd_wallet_default_set(&project, &address),
    }
}

fn cmd_wallet_list(project: &crate::model::Project, long: bool) -> DynResult<()> {
    let wallet = load_wallet_runtime(project)?;

    let mut command = Command::new(&wallet.wallet_binary);
    command
        .env(
            "NSSA_WALLET_HOME_DIR",
            wallet.wallet_home.as_os_str().to_string_lossy().to_string(),
        )
        .arg("account")
        .arg("list");

    if long {
        command.arg("--long");
    }

    run_forwarded(&mut command, "wallet account list")
        .context("failed to execute wallet list command")?;

    Ok(())
}

fn cmd_wallet_proxy(project: &crate::model::Project, args: &[String]) -> DynResult<()> {
    if args.is_empty() {
        bail!("wallet passthrough requires at least one argument after `--`. Example: `logos-scaffold wallet -- account list`");
    }

    let wallet = load_wallet_runtime(project)?;

    let mut command = Command::new(&wallet.wallet_binary);
    command.env(
        "NSSA_WALLET_HOME_DIR",
        wallet.wallet_home.as_os_str().to_string_lossy().to_string(),
    );
    for arg in args {
        command.arg(arg);
    }

    run_forwarded(&mut command, "wallet passthrough command")
        .context("wallet passthrough command failed")?;

    Ok(())
}

fn cmd_wallet_topup(
    project: &crate::model::Project,
    address: Option<String>,
    dry_run: bool,
) -> DynResult<()> {
    let wallet = load_wallet_runtime(project)?;
    let default_address = read_default_wallet_address(&project.root)?;
    let resolved_to = resolve_wallet_address(address.as_deref(), default_address.as_deref())?;
    let sequencer_addr = wallet
        .sequencer_addr
        .clone()
        .unwrap_or_else(|| "http://127.0.0.1:3040".to_string());

    let mut command = Command::new(&wallet.wallet_binary);
    command
        .env(
            "NSSA_WALLET_HOME_DIR",
            wallet.wallet_home.as_os_str().to_string_lossy().to_string(),
        )
        .arg("pinata")
        .arg("claim")
        .arg("--to")
        .arg(&resolved_to);

    if dry_run {
        println!("dry-run: wallet topup command will not be executed");
        println!(
            "NSSA_WALLET_HOME_DIR={}",
            wallet.wallet_home.as_os_str().to_string_lossy()
        );
        println!("$ {}", render_command(&command));
        println!("planned wallet: {resolved_to}");
        println!("planned method: pinata faucet claim");
        println!("planned network: local sequencer ({sequencer_addr})");
        return Ok(());
    }

    let output = run_with_stdin(command, format!("{DEFAULT_WALLET_PASSWORD}\n"))
        .context("failed to execute wallet topup command")?;

    if !output.status.success() {
        let summary = summarize_command_failure(&output.stdout, &output.stderr);
        let combined = format!("{}\n{}", output.stdout, output.stderr);
        if is_connectivity_failure(&combined) {
            bail!(
                "wallet topup failed: {summary}\n{}",
                sequencer_unreachable_hint(&sequencer_addr)
            );
        }
        bail!(
            "wallet topup failed: {summary}\nHint: run `logos-scaffold wallet list` to inspect addresses, then retry with `--address` or set a default wallet."
        );
    }

    println!("wallet topup complete");
    println!("  Address: {resolved_to}");
    println!("  Method: pinata faucet claim");
    println!("  Network: local sequencer ({sequencer_addr})");
    if let Some(tx) = extract_tx_identifier(&output.stdout, &output.stderr) {
        println!("  Tx: {tx}");
    }

    Ok(())
}

fn cmd_wallet_default_set(project: &crate::model::Project, address: &str) -> DynResult<()> {
    let normalized_address = normalize_address_ref(address)?;
    let state_path = project.root.join(".scaffold/state/wallet.state");
    write_text(
        &state_path,
        &format!("default_address={normalized_address}\n"),
    )?;

    println!("default wallet updated");
    println!("  Address: {normalized_address}");
    println!("  State file: {}", state_path.display());

    Ok(())
}
