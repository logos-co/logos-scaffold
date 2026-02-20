use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::model::Project;
use crate::project::load_project;
use crate::state::write_text;
use crate::DynResult;

use super::wallet::{ensure_wallet_initialized, run_wallet_capture};

pub(crate) fn cmd_deploy(args: &[String]) -> DynResult<()> {
    if args.is_empty() {
        return Err("usage: logos-scaffold deploy <hello-world>".into());
    }

    let project = load_project()?;

    match args[0].as_str() {
        "hello-world" => {
            if args.len() != 1 {
                return Err("usage: logos-scaffold deploy hello-world".into());
            }
            let state = deploy_hello_world(&project)?;
            println!(
                "deploy complete: program={} binary={} tx_hash={}",
                state.program,
                state.binary_path,
                state.tx_hash.as_deref().unwrap_or("unknown")
            );
            Ok(())
        }
        other => Err(format!("unknown deploy target: {other}").into()),
    }
}

pub(crate) fn deploy_hello_world(project: &Project) -> DynResult<DeployState> {
    let _ = ensure_wallet_initialized(project)?;

    let hello_world_bin = locate_hello_world_binary(&project.root)?.ok_or_else(|| {
        "could not find hello_world.bin under project target directory. Run `logos-scaffold build` (and if needed `cargo risczero build --manifest-path methods/guest/Cargo.toml`) first.".to_string()
    })?;

    let out = run_wallet_capture(
        project,
        &[
            "deploy-program",
            hello_world_bin
                .to_str()
                .ok_or("hello_world.bin path is not valid UTF-8")?,
        ],
        "wallet deploy-program hello_world.bin",
    )?;

    let merged = format!("{}\n{}", out.stdout, out.stderr);
    let tx_hash = parse_tx_hash(&merged);

    let state = DeployState {
        program: "hello-world".to_string(),
        binary_path: hello_world_bin.display().to_string(),
        tx_hash,
        deployed_at_unix: now_unix_seconds(),
    };

    write_deploy_state(&deploy_state_path(project), &state)?;
    Ok(state)
}

#[derive(Clone, Debug)]
pub(crate) struct DeployState {
    pub(crate) program: String,
    pub(crate) binary_path: String,
    pub(crate) tx_hash: Option<String>,
    pub(crate) deployed_at_unix: u64,
}

#[cfg(test)]
pub(crate) fn read_deploy_state(path: &Path) -> DynResult<Option<DeployState>> {
    if !path.exists() {
        return Ok(None);
    }

    let text = fs::read_to_string(path)?;
    let mut program = None;
    let mut binary_path = None;
    let mut tx_hash = None;
    let mut deployed_at_unix = None;

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let mut parts = line.splitn(2, '=');
        let key = parts.next().unwrap_or("").trim();
        let value = parts.next().unwrap_or("").trim();

        match key {
            "program" => {
                if !value.is_empty() {
                    program = Some(value.to_string());
                }
            }
            "binary_path" => {
                if !value.is_empty() {
                    binary_path = Some(value.to_string());
                }
            }
            "tx_hash" => {
                if !value.is_empty() {
                    tx_hash = Some(value.to_string());
                }
            }
            "deployed_at_unix" => {
                if let Ok(v) = value.parse::<u64>() {
                    deployed_at_unix = Some(v);
                }
            }
            _ => {}
        }
    }

    let Some(program) = program else {
        return Ok(None);
    };
    let Some(binary_path) = binary_path else {
        return Ok(None);
    };
    let Some(deployed_at_unix) = deployed_at_unix else {
        return Ok(None);
    };

    Ok(Some(DeployState {
        program,
        binary_path,
        tx_hash,
        deployed_at_unix,
    }))
}

pub(crate) fn deploy_state_path(project: &Project) -> PathBuf {
    project.root.join(".scaffold/state/deploy.state")
}

pub(crate) fn write_deploy_state(path: &Path, state: &DeployState) -> DynResult<()> {
    let mut out = String::new();
    out.push_str(&format!("program={}\n", state.program));
    out.push_str(&format!("binary_path={}\n", state.binary_path));
    if let Some(tx_hash) = &state.tx_hash {
        out.push_str(&format!("tx_hash={}\n", tx_hash));
    }
    out.push_str(&format!("deployed_at_unix={}\n", state.deployed_at_unix));
    write_text(path, &out)
}

fn locate_hello_world_binary(project_root: &Path) -> DynResult<Option<PathBuf>> {
    if let Ok(dir) = env::var("EXAMPLE_PROGRAMS_BUILD_DIR") {
        let candidate = PathBuf::from(dir).join("hello_world.bin");
        if candidate.exists() {
            return Ok(Some(candidate));
        }
    }

    let known_candidates = [
        project_root.join("target/riscv32im-risc0-zkvm-elf/docker/hello_world.bin"),
        project_root.join(
            "target/riscv-guest/example_program_deployment_methods/example_program_deployment_programs/riscv32im-risc0-zkvm-elf/release/hello_world.bin",
        ),
    ];

    for candidate in known_candidates {
        if candidate.exists() {
            return Ok(Some(candidate));
        }
    }

    let target_root = project_root.join("target");
    if !target_root.exists() {
        return Ok(None);
    }

    let mut found = Vec::new();
    collect_named_files(&target_root, "hello_world.bin", &mut found)?;

    found.sort_by_key(|p| p.to_string_lossy().len());
    Ok(found.into_iter().next())
}

fn collect_named_files(root: &Path, file_name: &str, found: &mut Vec<PathBuf>) -> DynResult<()> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_named_files(&path, file_name, found)?;
        } else if path
            .file_name()
            .and_then(|v| v.to_str())
            .map(|v| v == file_name)
            .unwrap_or(false)
        {
            found.push(path);
        }
    }

    Ok(())
}

pub(crate) fn parse_tx_hash(text: &str) -> Option<String> {
    let marker = "tx_hash:";
    let idx = text.find(marker)?;
    let after = &text[idx + marker.len()..];

    let open = after.find('[')?;
    let after_open = &after[open + 1..];
    let close = after_open.find(']')?;
    let payload = &after_open[..close];

    let cleaned = payload
        .split(',')
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
        .collect::<Vec<_>>()
        .join(",");

    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
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

    use super::{parse_tx_hash, read_deploy_state, write_deploy_state, DeployState};

    fn mk_temp_dir(suffix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "logos-scaffold-deploy-tests-{suffix}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("failed to create temp directory");
        path
    }

    #[test]
    fn parse_tx_hash_extracts_numeric_payload() {
        let sample = "Results of tx send are SendTxResponse { tx_hash: HashType([1, 2, 3, 4]) }";
        assert_eq!(parse_tx_hash(sample).as_deref(), Some("1,2,3,4"));
    }

    #[test]
    fn deploy_state_roundtrip() {
        let temp = mk_temp_dir("roundtrip");
        let path = temp.join("deploy.state");

        let state = DeployState {
            program: "hello-world".to_string(),
            binary_path: "/tmp/hello_world.bin".to_string(),
            tx_hash: Some("1,2,3".to_string()),
            deployed_at_unix: 42,
        };
        write_deploy_state(&path, &state).expect("failed writing deploy.state");
        let parsed = read_deploy_state(&path)
            .expect("failed reading deploy.state")
            .expect("state should parse");

        assert_eq!(parsed.program, "hello-world");
        assert_eq!(parsed.binary_path, "/tmp/hello_world.bin");
        assert_eq!(parsed.tx_hash.as_deref(), Some("1,2,3"));
        assert_eq!(parsed.deployed_at_unix, 42);

        fs::remove_dir_all(temp).expect("failed to cleanup temp dir");
    }
}
