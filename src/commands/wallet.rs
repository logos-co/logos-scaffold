use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::constants::{DEFAULT_WALLET_PASSWORD, DEFAULT_WALLET_PASSWORD_ENV};
use crate::model::{Captured, Project};
use crate::process::{run_capture, run_with_stdin};
use crate::project::{ensure_dir_exists, load_project};
use crate::state::{prepare_wallet_home, write_text};
use crate::DynResult;

pub(crate) fn cmd_wallet(args: &[String]) -> DynResult<()> {
    if args.is_empty() {
        return Err("usage: logos-scaffold wallet <init|topup> ...".into());
    }

    let project = load_project()?;

    match args[0].as_str() {
        "init" => {
            if args.len() != 1 {
                return Err("usage: logos-scaffold wallet init".into());
            }

            let account_id = ensure_wallet_initialized(&project)?;
            println!("wallet init complete (default public account: {account_id})");
            Ok(())
        }
        "topup" => {
            let mut to: Option<String> = None;
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--to" => {
                        let value = args.get(i + 1).ok_or("--to requires value")?;
                        to = Some(value.clone());
                        i += 2;
                    }
                    other => return Err(format!("unknown flag for wallet topup: {other}").into()),
                }
            }

            let to = to.ok_or("usage: logos-scaffold wallet topup --to <Public/...>")?;
            if !to.starts_with("Public/") {
                return Err("wallet topup currently requires a Public/<account_id> target".into());
            }

            let (before, after) = topup_wallet(&project, &to)?;
            println!("wallet topup complete for {to} (balance {before} -> {after})");
            Ok(())
        }
        other => Err(format!("unknown wallet command: {other}").into()),
    }
}

pub(crate) fn ensure_wallet_initialized(project: &Project) -> DynResult<String> {
    let lssa = PathBuf::from(&project.config.lssa.path);
    ensure_dir_exists(&lssa, "lssa")?;

    let wallet_home = wallet_home(project);
    prepare_wallet_home(&lssa, &wallet_home)?;

    let mut health = wallet_command(project);
    health
        .arg("check-health")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let out = run_with_stdin(health, format!("{}\n", wallet_password()))?;
    if !out.status.success() {
        let detail = one_line(&format!("{}\n{}", out.stdout, out.stderr));
        return Err(format!(
            "wallet init failed. Ensure localnet is running and wallet config is valid. details: {detail}"
        )
        .into());
    }

    let mut state = read_wallet_state(&wallet_state_path(project))?;
    if state.initialized_at_unix.is_none() {
        state.initialized_at_unix = Some(now_unix_seconds());
    }

    if state.default_public_account_id.is_none() {
        let out = run_wallet_capture(
            project,
            &["account", "new", "public"],
            "wallet account new public",
        )?;
        let merged = format!("{}\n{}", out.stdout, out.stderr);
        let account_id = parse_generated_account_id(&merged)
            .ok_or("failed to parse generated Public/<account_id> from wallet output")?;
        state.default_public_account_id = Some(account_id);
    }

    state.wallet_home_dir = Some(wallet_home.display().to_string());
    write_wallet_state(&wallet_state_path(project), &state)?;

    Ok(state
        .default_public_account_id
        .unwrap_or_else(|| "Public/<unknown>".to_string()))
}

pub(crate) fn topup_wallet(project: &Project, account_id: &str) -> DynResult<(u128, u128)> {
    let _ = ensure_wallet_initialized(project)?;

    let before = run_wallet_capture(
        project,
        &["account", "get", "--account-id", account_id],
        "wallet account get (before topup)",
    )?;

    let before_text = format!("{}\n{}", before.stdout, before.stderr);
    let before_balance = parse_balance(&before_text).unwrap_or(0);
    if is_uninitialized_account_output(&before_text) {
        let init_out = run_wallet_allow_failure(
            project,
            &["auth-transfer", "init", "--account-id", account_id],
            "wallet auth-transfer init",
        )?;

        if !init_out.status.success() {
            let detail = one_line(&format!("{}\n{}", init_out.stdout, init_out.stderr));
            println!(
                "warning: auth-transfer init returned non-zero; continuing with pinata claim ({detail})"
            );
        }
    }

    run_wallet_capture(
        project,
        &["pinata", "claim", "--to", account_id],
        "wallet pinata claim",
    )?;

    let after = run_wallet_capture(
        project,
        &["account", "get", "--account-id", account_id],
        "wallet account get (after topup)",
    )?;

    let after_text = format!("{}\n{}", after.stdout, after.stderr);
    let after_balance = parse_balance(&after_text).unwrap_or(0);

    if after_balance <= before_balance {
        return Err(format!(
            "wallet topup did not increase balance for {account_id} (before={before_balance}, after={after_balance})"
        )
        .into());
    }

    let mut state = read_wallet_state(&wallet_state_path(project))?;
    state.last_topup_account_id = Some(account_id.to_string());
    state.last_topup_before_balance = Some(before_balance);
    state.last_topup_after_balance = Some(after_balance);
    state.last_topup_at_unix = Some(now_unix_seconds());
    if state.default_public_account_id.is_none() && account_id.starts_with("Public/") {
        state.default_public_account_id = Some(account_id.to_string());
    }
    write_wallet_state(&wallet_state_path(project), &state)?;

    Ok((before_balance, after_balance))
}

pub(crate) fn run_wallet_capture(
    project: &Project,
    args: &[&str],
    label: &str,
) -> DynResult<Captured> {
    let mut cmd = wallet_command(project);
    for arg in args {
        cmd.arg(arg);
    }
    run_capture(&mut cmd, label)
}

fn run_wallet_allow_failure(project: &Project, args: &[&str], _label: &str) -> DynResult<Captured> {
    let mut cmd = wallet_command(project);
    for arg in args {
        cmd.arg(arg);
    }
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    run_with_stdin(cmd, String::new())
}

fn wallet_command(project: &Project) -> Command {
    let mut cmd = Command::new(&project.config.wallet_binary);
    cmd.env("NSSA_WALLET_HOME_DIR", wallet_home(project));
    cmd
}

fn wallet_home(project: &Project) -> PathBuf {
    project.root.join(&project.config.wallet_home_dir)
}

fn wallet_state_path(project: &Project) -> PathBuf {
    project.root.join(".scaffold/state/wallet.state")
}

fn wallet_password() -> String {
    env::var(DEFAULT_WALLET_PASSWORD_ENV).unwrap_or_else(|_| DEFAULT_WALLET_PASSWORD.to_string())
}

#[derive(Clone, Debug, Default)]
struct WalletState {
    initialized_at_unix: Option<u64>,
    wallet_home_dir: Option<String>,
    default_public_account_id: Option<String>,
    last_topup_account_id: Option<String>,
    last_topup_before_balance: Option<u128>,
    last_topup_after_balance: Option<u128>,
    last_topup_at_unix: Option<u64>,
}

fn read_wallet_state(path: &Path) -> DynResult<WalletState> {
    if !path.exists() {
        return Ok(WalletState::default());
    }

    let text = fs::read_to_string(path)?;
    let mut state = WalletState::default();

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let mut parts = line.splitn(2, '=');
        let key = parts.next().unwrap_or("").trim();
        let value = parts.next().unwrap_or("").trim();

        match key {
            "initialized_at_unix" => {
                if let Ok(v) = value.parse::<u64>() {
                    state.initialized_at_unix = Some(v);
                }
            }
            "wallet_home_dir" => {
                if !value.is_empty() {
                    state.wallet_home_dir = Some(value.to_string());
                }
            }
            "default_public_account_id" => {
                if !value.is_empty() {
                    state.default_public_account_id = Some(value.to_string());
                }
            }
            "last_topup_account_id" => {
                if !value.is_empty() {
                    state.last_topup_account_id = Some(value.to_string());
                }
            }
            "last_topup_before_balance" => {
                if let Ok(v) = value.parse::<u128>() {
                    state.last_topup_before_balance = Some(v);
                }
            }
            "last_topup_after_balance" => {
                if let Ok(v) = value.parse::<u128>() {
                    state.last_topup_after_balance = Some(v);
                }
            }
            "last_topup_at_unix" => {
                if let Ok(v) = value.parse::<u64>() {
                    state.last_topup_at_unix = Some(v);
                }
            }
            _ => {}
        }
    }

    Ok(state)
}

fn write_wallet_state(path: &Path, state: &WalletState) -> DynResult<()> {
    let mut out = String::new();

    if let Some(v) = state.initialized_at_unix {
        out.push_str(&format!("initialized_at_unix={v}\n"));
    }
    if let Some(v) = &state.wallet_home_dir {
        out.push_str(&format!("wallet_home_dir={v}\n"));
    }
    if let Some(v) = &state.default_public_account_id {
        out.push_str(&format!("default_public_account_id={v}\n"));
    }
    if let Some(v) = &state.last_topup_account_id {
        out.push_str(&format!("last_topup_account_id={v}\n"));
    }
    if let Some(v) = state.last_topup_before_balance {
        out.push_str(&format!("last_topup_before_balance={v}\n"));
    }
    if let Some(v) = state.last_topup_after_balance {
        out.push_str(&format!("last_topup_after_balance={v}\n"));
    }
    if let Some(v) = state.last_topup_at_unix {
        out.push_str(&format!("last_topup_at_unix={v}\n"));
    }

    write_text(path, &out)
}

fn parse_generated_account_id(text: &str) -> Option<String> {
    for token in text.split_whitespace() {
        let cleaned = token.trim_end_matches(|c: char| c == ',' || c == ';' || c == '.');
        if cleaned.starts_with("Public/") {
            return Some(cleaned.to_string());
        }
    }
    None
}

pub(crate) fn parse_balance(text: &str) -> Option<u128> {
    let marker = "\"balance\"";
    let idx = text.find(marker)?;
    let after_marker = &text[idx + marker.len()..];
    let colon = after_marker.find(':')?;
    let after_colon = after_marker[colon + 1..].trim_start();

    let mut digits = String::new();
    for ch in after_colon.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
        } else {
            break;
        }
    }

    if digits.is_empty() {
        None
    } else {
        digits.parse::<u128>().ok()
    }
}

pub(crate) fn is_uninitialized_account_output(text: &str) -> bool {
    text.contains("Account is Uninitialized")
}

fn one_line(text: &str) -> String {
    text.replace('\n', " ").replace('\r', " ")
}

fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_secs()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        parse_balance, parse_generated_account_id, read_wallet_state, write_wallet_state,
        WalletState,
    };

    fn mk_temp_dir(suffix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "logos-scaffold-wallet-tests-{suffix}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("failed to create temp directory");
        path
    }

    #[test]
    fn parse_generated_account_id_reads_public_id() {
        let raw = "Generated new account with account_id Public/AbCdEf123 at path /0";
        assert_eq!(
            parse_generated_account_id(raw).as_deref(),
            Some("Public/AbCdEf123")
        );
    }

    #[test]
    fn parse_balance_reads_balance_field() {
        let raw = "Account owned by authenticated transfer program\n{\"balance\":150}";
        assert_eq!(parse_balance(raw), Some(150));
    }

    #[test]
    fn parse_balance_handles_extended_json_shape() {
        let raw = r#"{"balance":150,"program_owner_b64":"x","data_b64":"SG9sYSBtdW5kbyE="}"#;
        assert_eq!(parse_balance(raw), Some(150));
    }

    #[test]
    fn wallet_state_roundtrip_keeps_values() {
        let temp = mk_temp_dir("roundtrip");
        let state_path = temp.join("wallet.state");

        let state = WalletState {
            initialized_at_unix: Some(1),
            wallet_home_dir: Some("/tmp/wallet".to_string()),
            default_public_account_id: Some("Public/abc".to_string()),
            last_topup_account_id: Some("Public/abc".to_string()),
            last_topup_before_balance: Some(10),
            last_topup_after_balance: Some(20),
            last_topup_at_unix: Some(2),
        };

        write_wallet_state(&state_path, &state).expect("failed writing wallet.state");
        let parsed = read_wallet_state(&state_path).expect("failed reading wallet.state");

        assert_eq!(parsed.initialized_at_unix, Some(1));
        assert_eq!(parsed.wallet_home_dir.as_deref(), Some("/tmp/wallet"));
        assert_eq!(
            parsed.default_public_account_id.as_deref(),
            Some("Public/abc")
        );
        assert_eq!(parsed.last_topup_before_balance, Some(10));
        assert_eq!(parsed.last_topup_after_balance, Some(20));

        fs::remove_dir_all(temp).expect("failed to cleanup temp dir");
    }
}
