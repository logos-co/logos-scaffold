use std::time::{SystemTime, UNIX_EPOCH};

use crate::project::load_project;
use crate::state::write_text;
use crate::DynResult;

use super::deploy::deploy_hello_world;
use super::interact::interact_hello_world;
use super::localnet::cmd_localnet;
use super::setup::cmd_setup;
use super::verify::verify_hello_world;
use super::wallet::{ensure_wallet_initialized, topup_wallet};

pub(crate) fn cmd_slice(args: &[String]) -> DynResult<()> {
    if args.is_empty() {
        return Err(
            "usage: logos-scaffold slice run [--repeat N] [--account-id Public/...]".into(),
        );
    }

    match args[0].as_str() {
        "run" => cmd_slice_run(&args[1..]),
        other => Err(format!("unknown slice command: {other}").into()),
    }
}

fn cmd_slice_run(args: &[String]) -> DynResult<()> {
    let mut repeat: usize = 1;
    let mut account_id: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--repeat" => {
                let value = args.get(i + 1).ok_or("--repeat requires value")?;
                repeat = value
                    .parse::<usize>()
                    .map_err(|_| "--repeat expects a positive integer")?;
                if repeat == 0 {
                    return Err("--repeat must be >= 1".into());
                }
                i += 2;
            }
            "--account-id" => {
                let value = args.get(i + 1).ok_or("--account-id requires value")?;
                account_id = Some(value.clone());
                i += 2;
            }
            other => return Err(format!("unknown flag for slice run: {other}").into()),
        }
    }

    let project = load_project()?;

    cmd_setup(&[])?;
    let localnet_start = vec!["start".to_string()];
    cmd_localnet(&localnet_start)?;

    let default_account = ensure_wallet_initialized(&project)?;
    let account_id = account_id.unwrap_or(default_account);
    if !account_id.starts_with("Public/") {
        return Err("slice run currently expects a Public/<account_id>".into());
    }

    for n in 1..=repeat {
        println!("=== slice iteration {n}/{repeat} ===");

        let _ = topup_wallet(&project, &account_id)?;
        let deploy = deploy_hello_world(&project)?;
        interact_hello_world(&project, &account_id)?;
        let verify = verify_hello_world(&project, &account_id)?;

        let state_json = format!(
            concat!(
                "{{\n",
                "  \"network\": \"local\",\n",
                "  \"iteration\": {},\n",
                "  \"repeat\": {},\n",
                "  \"account_id\": \"{}\",\n",
                "  \"program\": \"{}\",\n",
                "  \"tx_hash\": {},\n",
                "  \"verified\": {},\n",
                "  \"timestamp_unix\": {}\n",
                "}}\n"
            ),
            n,
            repeat,
            json_escape(&account_id),
            json_escape(&deploy.program),
            deploy
                .tx_hash
                .as_deref()
                .map(|v| format!("\"{}\"", json_escape(v)))
                .unwrap_or_else(|| "null".to_string()),
            verify.marker_found,
            now_unix_seconds()
        );

        write_text(&slice_last_state_path(&project), &state_json)?;
    }

    println!("slice run complete: {} successful iteration(s)", repeat);
    Ok(())
}

fn slice_last_state_path(project: &crate::model::Project) -> std::path::PathBuf {
    project.root.join(".scaffold/state/slice-last.json")
}

fn json_escape(input: &str) -> String {
    input.replace('\\', "\\\\").replace('"', "\\\"")
}

fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_secs()
}
