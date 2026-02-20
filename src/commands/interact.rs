use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::model::Project;
use crate::process::run_checked;
use crate::project::load_project;
use crate::state::write_text;
use crate::DynResult;

pub(crate) fn cmd_interact(args: &[String]) -> DynResult<()> {
    if args.is_empty() {
        return Err(
            "usage: logos-scaffold interact <hello-world> --account-id <Public/...>".into(),
        );
    }

    let project = load_project()?;

    match args[0].as_str() {
        "hello-world" => {
            let account_id = parse_account_id_flag(args)?;
            interact_hello_world(&project, &account_id)?;
            println!("interaction complete for account {account_id}");
            Ok(())
        }
        other => Err(format!("unknown interact target: {other}").into()),
    }
}

pub(crate) fn interact_hello_world(project: &Project, account_id: &str) -> DynResult<()> {
    if !account_id.starts_with("Public/") {
        return Err("interact hello-world currently expects --account-id Public/<id>".into());
    }

    run_checked(
        Command::new("cargo")
            .current_dir(&project.root)
            .arg("run")
            .arg("--bin")
            .arg("run_hello_world")
            .arg("--")
            .arg(account_id),
        "cargo run --bin run_hello_world",
    )?;

    let state = format!(
        "target=hello-world\naccount_id={}\ninteracted_at_unix={}\n",
        account_id,
        now_unix_seconds()
    );
    write_text(&interact_state_path(project), &state)?;

    Ok(())
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
            other => return Err(format!("unknown flag for interact hello-world: {other}").into()),
        }
    }

    match account_id {
        Some(v) => Ok(v),
        None => Err("usage: logos-scaffold interact hello-world --account-id <Public/...>".into()),
    }
}

pub(crate) fn interact_state_path(project: &Project) -> std::path::PathBuf {
    project.root.join(".scaffold/state/interact.state")
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
        let parsed = parse_account_id_flag(&args).expect("should parse --account-id");
        assert_eq!(parsed, "Public/abc");
    }

    #[test]
    fn parse_account_id_flag_rejects_missing_value() {
        let args = vec!["hello-world".to_string(), "--account-id".to_string()];
        assert!(parse_account_id_flag(&args).is_err());
    }
}
