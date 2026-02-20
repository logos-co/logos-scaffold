use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::constants::{LEGACY_WALLET_CONFIG_FILENAME, WALLET_CONFIG_FILENAME};
use crate::model::LocalnetState;
use crate::DynResult;

pub(crate) fn write_text(path: &Path, text: &str) -> DynResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, text)?;
    Ok(())
}

pub(crate) fn write_localnet_state(path: &Path, state: &LocalnetState) -> DynResult<()> {
    let mut content = String::new();
    if let Some(pid) = state.sequencer_pid {
        content.push_str(&format!("sequencer_pid={pid}\n"));
    }
    if let Some(runtime_config_path) = &state.runtime_config_path {
        content.push_str(&format!("runtime_config_path={runtime_config_path}\n"));
    }
    if let Some(runtime_home_dir) = &state.runtime_home_dir {
        content.push_str(&format!("runtime_home_dir={runtime_home_dir}\n"));
    }
    write_text(path, &content)
}

pub(crate) fn read_localnet_state(path: &Path) -> DynResult<LocalnetState> {
    let mut text = String::new();
    File::open(path)?.read_to_string(&mut text)?;

    let mut state = LocalnetState::default();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }

        if let Some(rest) = line.strip_prefix("sequencer_pid=") {
            let pid: u32 = rest.parse().map_err(|_| "invalid sequencer pid")?;
            state.sequencer_pid = Some(pid);
            continue;
        }

        if let Some(rest) = line.strip_prefix("runtime_config_path=") {
            if !rest.is_empty() {
                state.runtime_config_path = Some(rest.to_string());
            }
            continue;
        }

        if let Some(rest) = line.strip_prefix("runtime_home_dir=") {
            if !rest.is_empty() {
                state.runtime_home_dir = Some(rest.to_string());
            }
            continue;
        }
    }

    Ok(state)
}

pub(crate) fn wallet_config_path(wallet_home: &Path) -> PathBuf {
    wallet_home.join(WALLET_CONFIG_FILENAME)
}

pub(crate) fn legacy_wallet_config_path(wallet_home: &Path) -> PathBuf {
    wallet_home.join(LEGACY_WALLET_CONFIG_FILENAME)
}

pub(crate) fn prepare_wallet_home(lssa_repo: &Path, wallet_home: &Path) -> DynResult<()> {
    fs::create_dir_all(wallet_home)?;

    let cfg_dst = wallet_config_path(wallet_home);
    if cfg_dst.exists() {
        return Ok(());
    }

    let legacy_cfg = legacy_wallet_config_path(wallet_home);
    if legacy_cfg.exists() {
        fs::copy(legacy_cfg, cfg_dst)?;
        return Ok(());
    }

    let cfg_src = lssa_repo
        .join("wallet")
        .join("configs")
        .join("debug")
        .join(WALLET_CONFIG_FILENAME);
    if !cfg_src.exists() {
        return Err("missing wallet debug config in lssa repo".into());
    }

    fs::copy(cfg_src, cfg_dst)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        prepare_wallet_home, read_localnet_state, wallet_config_path, write_localnet_state,
        write_text,
    };
    use crate::model::LocalnetState;

    fn mk_temp_dir(suffix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "logos-scaffold-state-tests-{suffix}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("failed to create temp directory");
        path
    }

    fn mk_fake_lssa(root: &Path, cfg_content: &str) {
        let cfg = root.join("wallet/configs/debug/wallet_config.json");
        write_text(&cfg, cfg_content).expect("failed to write fake wallet config");
    }

    #[test]
    fn prepare_wallet_home_copies_wallet_config_on_first_run() {
        let temp = mk_temp_dir("copy");
        let lssa = temp.join("lssa");
        let wallet_home = temp.join("wallet-home");
        mk_fake_lssa(&lssa, "{\"sequencer_addr\":\"http://127.0.0.1:3040\"}\n");

        prepare_wallet_home(&lssa, &wallet_home).expect("prepare_wallet_home should succeed");

        let cfg = fs::read_to_string(wallet_config_path(&wallet_home))
            .expect("wallet_config.json should be copied");
        assert!(cfg.contains("127.0.0.1:3040"));

        fs::remove_dir_all(temp).expect("failed to cleanup temp dir");
    }

    #[test]
    fn prepare_wallet_home_migrates_legacy_config_json() {
        let temp = mk_temp_dir("migrate");
        let lssa = temp.join("lssa");
        let wallet_home = temp.join("wallet-home");
        mk_fake_lssa(&lssa, "{\"sequencer_addr\":\"http://127.0.0.1:1111\"}\n");
        write_text(
            &wallet_home.join("config.json"),
            "{\"sequencer_addr\":\"http://127.0.0.1:3040\"}\n",
        )
        .expect("failed to write legacy config.json");

        prepare_wallet_home(&lssa, &wallet_home).expect("prepare_wallet_home should succeed");

        let cfg = fs::read_to_string(wallet_config_path(&wallet_home))
            .expect("wallet_config.json should be written from legacy file");
        assert!(cfg.contains("127.0.0.1:3040"));

        fs::remove_dir_all(temp).expect("failed to cleanup temp dir");
    }

    #[test]
    fn prepare_wallet_home_is_idempotent_when_wallet_config_exists() {
        let temp = mk_temp_dir("idempotent");
        let lssa = temp.join("lssa");
        let wallet_home = temp.join("wallet-home");
        mk_fake_lssa(&lssa, "{\"sequencer_addr\":\"http://127.0.0.1:1111\"}\n");
        write_text(
            &wallet_config_path(&wallet_home),
            "{\"sequencer_addr\":\"http://127.0.0.1:3040\"}\n",
        )
        .expect("failed to write existing wallet_config.json");

        prepare_wallet_home(&lssa, &wallet_home).expect("prepare_wallet_home should succeed");

        let cfg = fs::read_to_string(wallet_config_path(&wallet_home))
            .expect("wallet_config.json should still exist");
        assert!(cfg.contains("127.0.0.1:3040"));
        assert!(!cfg.contains("127.0.0.1:1111"));

        fs::remove_dir_all(temp).expect("failed to cleanup temp dir");
    }

    #[test]
    fn localnet_state_roundtrip_keeps_extended_fields() {
        let temp = mk_temp_dir("localnet-roundtrip");
        let state_path = temp.join("localnet.state");

        let state = LocalnetState {
            sequencer_pid: Some(12345),
            runtime_config_path: Some("sequencer_runner/configs/debug".to_string()),
            runtime_home_dir: Some("/tmp/lssa".to_string()),
        };
        write_localnet_state(&state_path, &state).expect("failed writing localnet state");

        let parsed = read_localnet_state(&state_path).expect("failed reading localnet state");
        assert_eq!(parsed.sequencer_pid, Some(12345));
        assert_eq!(
            parsed.runtime_config_path.as_deref(),
            Some("sequencer_runner/configs/debug")
        );
        assert_eq!(parsed.runtime_home_dir.as_deref(), Some("/tmp/lssa"));

        fs::remove_dir_all(temp).expect("failed to cleanup temp dir");
    }

    #[test]
    fn localnet_state_reader_is_backward_compatible_with_pid_only_file() {
        let temp = mk_temp_dir("localnet-legacy");
        let state_path = temp.join("localnet.state");
        write_text(&state_path, "sequencer_pid=98765\n").expect("failed writing legacy state");

        let parsed = read_localnet_state(&state_path).expect("failed reading legacy state");
        assert_eq!(parsed.sequencer_pid, Some(98765));
        assert!(parsed.runtime_config_path.is_none());
        assert!(parsed.runtime_home_dir.is_none());

        fs::remove_dir_all(temp).expect("failed to cleanup temp dir");
    }
}
