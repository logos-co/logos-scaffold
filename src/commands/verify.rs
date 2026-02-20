use std::time::{SystemTime, UNIX_EPOCH};

use crate::constants::HELLO_WORLD_GREETING_B64;
use crate::model::Project;
use crate::project::load_project;
use crate::state::write_text;
use crate::DynResult;

use super::wallet::run_wallet_capture;

pub(crate) fn cmd_verify(args: &[String]) -> DynResult<()> {
    if args.is_empty() {
        return Err("usage: logos-scaffold verify <hello-world> --account-id <Public/...>".into());
    }

    let project = load_project()?;

    match args[0].as_str() {
        "hello-world" => {
            let account_id = parse_account_id_flag(args)?;
            let state = verify_hello_world(&project, &account_id)?;
            println!(
                "verify complete: target={} account={} marker_found={}",
                state.target, state.account_id, state.marker_found
            );
            Ok(())
        }
        other => Err(format!("unknown verify target: {other}").into()),
    }
}

#[derive(Clone, Debug)]
pub(crate) struct VerifyState {
    pub(crate) target: String,
    pub(crate) account_id: String,
    pub(crate) marker_found: bool,
    pub(crate) verified_at_unix: u64,
}

pub(crate) fn verify_hello_world(project: &Project, account_id: &str) -> DynResult<VerifyState> {
    if !account_id.starts_with("Public/") {
        return Err("verify hello-world currently expects --account-id Public/<id>".into());
    }

    let out = run_wallet_capture(
        project,
        &["account", "get", "--account-id", account_id],
        "wallet account get (verify hello-world)",
    )?;

    let merged = format!("{}\n{}", out.stdout, out.stderr);
    let marker_found = merged.contains(HELLO_WORLD_GREETING_B64);
    if !marker_found {
        return Err(format!(
            "verification failed for {account_id}: expected marker `{HELLO_WORLD_GREETING_B64}` not found"
        )
        .into());
    }

    let state = VerifyState {
        target: "hello-world".to_string(),
        account_id: account_id.to_string(),
        marker_found,
        verified_at_unix: now_unix_seconds(),
    };

    write_verify_state(&verify_state_path(project), &state)?;
    Ok(state)
}

fn parse_account_id_flag(args: &[String]) -> DynResult<String> {
    let mut account_id: Option<String> = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--account-id" => {
                let value = args.get(i + 1).ok_or("--account-id requires value")?;
                account_id = Some(value.clone());
                i += 2;
            }
            other => return Err(format!("unknown flag for verify hello-world: {other}").into()),
        }
    }

    match account_id {
        Some(v) => Ok(v),
        None => Err("usage: logos-scaffold verify hello-world --account-id <Public/...>".into()),
    }
}

pub(crate) fn verify_state_path(project: &Project) -> std::path::PathBuf {
    project.root.join(".scaffold/state/verify.state")
}

pub(crate) fn write_verify_state(path: &std::path::Path, state: &VerifyState) -> DynResult<()> {
    let text = format!(
        "target={}\naccount_id={}\nmarker_found={}\nverified_at_unix={}\n",
        state.target, state.account_id, state.marker_found, state.verified_at_unix
    );
    write_text(path, &text)
}

fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::parse_account_id_flag;

    #[test]
    fn parse_account_id_flag_accepts_valid_input() {
        let args = vec![
            "hello-world".to_string(),
            "--account-id".to_string(),
            "Public/abc".to_string(),
        ];
        let parsed = parse_account_id_flag(&args).expect("should parse account flag");
        assert_eq!(parsed, "Public/abc");
    }

    #[test]
    fn parse_account_id_flag_rejects_unknown_flag() {
        let args = vec![
            "hello-world".to_string(),
            "--wrong".to_string(),
            "Public/abc".to_string(),
        ];
        assert!(parse_account_id_flag(&args).is_err());
    }
}
