use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context};
use serde_json::Value;

use crate::model::Project;
use crate::DynResult;

const WALLET_CONFIG_PRIMARY: &str = "wallet_config.json";
const WALLET_CONFIG_FALLBACK: &str = "config.json";

pub(crate) struct WalletRuntimeContext {
    pub(crate) wallet_home: PathBuf,
    pub(crate) wallet_binary: String,
    pub(crate) sequencer_addr: Option<String>,
}

pub(crate) fn load_wallet_runtime(project: &Project) -> DynResult<WalletRuntimeContext> {
    let wallet_home = project.root.join(&project.config.wallet_home_dir);
    if !wallet_home.exists() {
        bail!(
            "missing wallet home at {}. Run `logos-scaffold setup` first.",
            wallet_home.display()
        );
    }

    let (_, wallet_config) = read_wallet_config(&wallet_home)?;
    let sequencer_addr = wallet_config
        .get("sequencer_addr")
        .and_then(Value::as_str)
        .map(ToString::to_string);

    Ok(WalletRuntimeContext {
        wallet_home,
        wallet_binary: project.config.wallet_binary.clone(),
        sequencer_addr,
    })
}

fn read_wallet_config(wallet_home: &Path) -> DynResult<(PathBuf, Value)> {
    let candidates = [
        wallet_home.join(WALLET_CONFIG_PRIMARY),
        wallet_home.join(WALLET_CONFIG_FALLBACK),
    ];

    let path = candidates
        .iter()
        .find(|path| path.exists())
        .cloned()
        .ok_or_else(|| {
            anyhow::anyhow!(
                "missing wallet config. Expected `{}` or `{}` under {}. Run `logos-scaffold setup`.",
                WALLET_CONFIG_PRIMARY,
                WALLET_CONFIG_FALLBACK,
                wallet_home.display()
            )
        })?;

    let text = fs::read_to_string(&path)
        .with_context(|| format!("failed to read wallet config at {}", path.display()))?;
    let value = serde_json::from_str::<Value>(&text)
        .with_context(|| format!("failed to parse wallet config JSON at {}", path.display()))?;

    Ok((path, value))
}

pub(crate) fn read_default_wallet_address(project_root: &Path) -> DynResult<Option<String>> {
    let state_path = project_root.join(".scaffold/state/wallet.state");
    if !state_path.exists() {
        return Ok(None);
    }

    let text = fs::read_to_string(&state_path)
        .with_context(|| format!("failed to read {}", state_path.display()))?;

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("default_address=") {
            let value = rest.trim();
            if value.is_empty() {
                bail!(
                    "default wallet at {} is empty. Run `logos-scaffold wallet default set <address>`.",
                    state_path.display()
                );
            }
            return Ok(Some(value.to_string()));
        }
    }

    if text.trim().is_empty() {
        return Ok(None);
    }

    bail!(
        "wallet state at {} is malformed. Expected `default_address=<address>`. Run `logos-scaffold wallet default set <address>`.",
        state_path.display()
    )
}

pub(crate) fn resolve_wallet_address(
    explicit: Option<&str>,
    default_from_state: Option<&str>,
) -> DynResult<String> {
    if let Some(explicit) = explicit {
        return normalize_address_ref(explicit);
    }

    if let Some(default_address) = default_from_state {
        return normalize_address_ref(default_address);
    }

    bail!(
        "wallet topup requires a destination address.\nNext step: run `logos-scaffold wallet list` to inspect available wallets, then run `logos-scaffold wallet default set <address>` or pass `--address <address>`."
    )
}

pub(crate) fn normalize_address_ref(raw: &str) -> DynResult<String> {
    let input = raw.trim();
    if input.is_empty() {
        bail!(invalid_address_message(raw));
    }

    let (prefix, account_id) = if let Some(rest) = input.strip_prefix("Public/") {
        ("Public", rest)
    } else if let Some(rest) = input.strip_prefix("Private/") {
        ("Private", rest)
    } else {
        ("Public", input)
    };

    validate_base58_account_id(account_id)
        .map_err(|_| anyhow::anyhow!(invalid_address_message(raw)))?;

    Ok(format!("{prefix}/{account_id}"))
}

fn validate_base58_account_id(account_id: &str) -> DynResult<()> {
    let decoded = bs58::decode(account_id)
        .into_vec()
        .map_err(|_| anyhow::anyhow!("invalid base58 account id"))?;

    if decoded.len() != 32 {
        bail!("account id must decode to exactly 32 bytes");
    }

    Ok(())
}

fn invalid_address_message(raw: &str) -> String {
    format!(
        "invalid address format `{raw}`\nAccepted formats:\n- Public/<base58-account-id>\n- Private/<base58-account-id>\n- <base58-account-id> (treated as Public/<...>)\nExamples:\n- Public/6iArKUXxhUJqS7kCaPNhwMWt3ro71PDyBj7jwAyE2VQV\n- Private/2ECgkFTaXzwjJBXR7ZKmXYQtpHbvTTHK9Auma4NL9AUo\n- 6iArKUXxhUJqS7kCaPNhwMWt3ro71PDyBj7jwAyE2VQV"
    )
}

pub(crate) fn is_connectivity_failure(text: &str) -> bool {
    let lower = text.to_lowercase();
    [
        "connection refused",
        "connecterror",
        "failed to connect",
        "tcp connect error",
        "network is unreachable",
        "error sending request",
        "http error",
        "127.0.0.1:3040",
        "localhost:3040",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

pub(crate) fn summarize_command_failure(stdout: &str, stderr: &str) -> String {
    let stderr_line = stderr
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.to_string());
    if let Some(line) = stderr_line {
        return line;
    }

    let stdout_line = stdout
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.to_string());
    if let Some(line) = stdout_line {
        return line;
    }

    "command failed without stderr output".to_string()
}

pub(crate) fn extract_tx_identifier(stdout: &str, stderr: &str) -> Option<String> {
    let combined = format!("{stdout}\n{stderr}");
    for raw_line in combined.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some(rest) = line.split("tx_hash=").nth(1) {
            return Some(rest.trim().to_string());
        }
        if line.contains("tx_hash:") {
            return Some(line.to_string());
        }
        if line.contains("\"tx_hash\"") {
            return Some(line.to_string());
        }
    }

    None
}

#[derive(Debug, Clone)]
pub(crate) enum RpcReachabilityError {
    Connectivity(String),
    Other(String),
}

impl std::fmt::Display for RpcReachabilityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RpcReachabilityError::Connectivity(msg) => write!(f, "{msg}"),
            RpcReachabilityError::Other(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for RpcReachabilityError {}

pub(crate) fn rpc_get_last_block(sequencer_addr: &str) -> Result<u64, RpcReachabilityError> {
    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1_u64,
        "method": "get_last_block",
        "params": {}
    });

    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(1))
        .timeout_read(Duration::from_secs(2))
        .timeout_write(Duration::from_secs(2))
        .build();

    let response = agent
        .post(sequencer_addr)
        .set("content-type", "application/json")
        .send_json(payload)
        .map_err(map_ureq_error)?;

    let body: Value = response.into_json().map_err(|err| {
        RpcReachabilityError::Other(format!(
            "failed to decode get_last_block response from {sequencer_addr}: {err}"
        ))
    })?;

    body.get("result")
        .and_then(|result| result.get("last_block"))
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            RpcReachabilityError::Other(format!(
                "get_last_block response missing `result.last_block`: {}",
                one_line(&body.to_string())
            ))
        })
}

fn map_ureq_error(err: ureq::Error) -> RpcReachabilityError {
    match err {
        ureq::Error::Transport(transport) => {
            let msg = transport.to_string();
            if is_connectivity_failure(&msg) {
                RpcReachabilityError::Connectivity(msg)
            } else {
                RpcReachabilityError::Other(msg)
            }
        }
        ureq::Error::Status(code, response) => {
            let body = response.into_string().unwrap_or_default();
            RpcReachabilityError::Other(format!("HTTP {code}: {}", one_line(&body)))
        }
    }
}

pub(crate) fn sequencer_unreachable_hint(sequencer_addr: &str) -> String {
    format!(
        "sequencer appears unavailable at {sequencer_addr}\nRun `logos-scaffold localnet start`.\nAnother project's sequencer may already be running and may not match this project."
    )
}

fn one_line(text: &str) -> String {
    text.replace(['\n', '\r'], " ")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{
        extract_tx_identifier, normalize_address_ref, read_default_wallet_address,
        resolve_wallet_address,
    };

    const ACCOUNT_ID: &str = "6iArKUXxhUJqS7kCaPNhwMWt3ro71PDyBj7jwAyE2VQV";

    #[test]
    fn normalize_accepts_raw_account_id() {
        let normalized = normalize_address_ref(ACCOUNT_ID).expect("normalize");
        assert_eq!(normalized, format!("Public/{ACCOUNT_ID}"));
    }

    #[test]
    fn normalize_accepts_private_prefix() {
        let normalized =
            normalize_address_ref(&format!("Private/{ACCOUNT_ID}")).expect("normalize");
        assert_eq!(normalized, format!("Private/{ACCOUNT_ID}"));
    }

    #[test]
    fn normalize_rejects_invalid_address() {
        let err = normalize_address_ref("abc").expect_err("must reject invalid address");
        assert!(err.to_string().contains("invalid address format"));
    }

    #[test]
    fn read_default_wallet_address_returns_none_for_missing_state() {
        let temp = tempdir().expect("tempdir");
        let value = read_default_wallet_address(temp.path()).expect("read default");
        assert!(value.is_none());
    }

    #[test]
    fn read_default_wallet_address_parses_state_file() {
        let temp = tempdir().expect("tempdir");
        let state_path = temp.path().join(".scaffold/state/wallet.state");
        fs::create_dir_all(state_path.parent().expect("parent")).expect("mkdir");
        fs::write(
            &state_path,
            format!("default_address=Public/{ACCOUNT_ID}\n"),
        )
        .expect("write");

        let value = read_default_wallet_address(temp.path()).expect("read default");
        let expected = format!("Public/{ACCOUNT_ID}");
        assert_eq!(value.as_deref(), Some(expected.as_str()));
    }

    #[test]
    fn resolve_wallet_address_prefers_explicit_input() {
        let value = resolve_wallet_address(
            Some(ACCOUNT_ID),
            Some("Private/2ECgkFTaXzwjJBXR7ZKmXYQtpHbvTTHK9Auma4NL9AUo"),
        )
        .expect("resolve");
        assert_eq!(value, format!("Public/{ACCOUNT_ID}"));
    }

    #[test]
    fn resolve_wallet_address_uses_default_when_explicit_missing() {
        let value =
            resolve_wallet_address(None, Some(&format!("Public/{ACCOUNT_ID}"))).expect("resolve");
        assert_eq!(value, format!("Public/{ACCOUNT_ID}"));
    }

    #[test]
    fn resolve_wallet_address_errors_when_both_missing() {
        let err = resolve_wallet_address(None, None).expect_err("must fail");
        assert!(err
            .to_string()
            .contains("wallet topup requires a destination address"));
    }

    #[test]
    fn extract_tx_identifier_finds_tx_hash_key() {
        let tx = extract_tx_identifier("ok tx_hash=abc123", "");
        assert_eq!(tx.as_deref(), Some("abc123"));
    }
}
