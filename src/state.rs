use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use anyhow::{anyhow, bail};

use crate::commands::wallet_support::WALLET_CONFIG_PRIMARY;
use crate::model::{BasecampSource, BasecampState, LocalnetState};
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
            let pid: u32 = rest.parse().map_err(|_| anyhow!("invalid sequencer pid"))?;
            state.sequencer_pid = Some(pid);
        }
    }

    Ok(state)
}

pub(crate) fn write_basecamp_state(path: &Path, state: &BasecampState) -> DynResult<()> {
    let mut content = String::new();
    if !state.pin.is_empty() {
        content.push_str(&format!("pin={}\n", state.pin));
    }
    if !state.basecamp_bin.is_empty() {
        content.push_str(&format!("basecamp_bin={}\n", state.basecamp_bin));
    }
    if !state.lgpm_bin.is_empty() {
        content.push_str(&format!("lgpm_bin={}\n", state.lgpm_bin));
    }
    for source in &state.sources {
        match source {
            BasecampSource::Path(p) => content.push_str(&format!("source:path={p}\n")),
            BasecampSource::Flake(f) => content.push_str(&format!("source:flake={f}\n")),
        }
    }
    write_text(path, &content)
}

pub(crate) fn read_basecamp_state(path: &Path) -> DynResult<BasecampState> {
    let mut text = String::new();
    File::open(path)?.read_to_string(&mut text)?;

    let mut state = BasecampState::default();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some(rest) = line.strip_prefix("pin=") {
            state.pin = rest.to_string();
        } else if let Some(rest) = line.strip_prefix("basecamp_bin=") {
            state.basecamp_bin = rest.to_string();
        } else if let Some(rest) = line.strip_prefix("lgpm_bin=") {
            state.lgpm_bin = rest.to_string();
        } else if let Some(rest) = line.strip_prefix("source:path=") {
            state.sources.push(BasecampSource::Path(rest.to_string()));
        } else if let Some(rest) = line.strip_prefix("source:flake=") {
            state.sources.push(BasecampSource::Flake(rest.to_string()));
        }
    }

    Ok(state)
}

pub(crate) fn prepare_wallet_home(lez_repo: &Path, wallet_home: &Path) -> DynResult<()> {
    fs::create_dir_all(wallet_home)?;
    let cfg_dst = wallet_home.join(WALLET_CONFIG_PRIMARY);
    if !cfg_dst.exists() {
        let cfg_src = lez_repo.join("wallet/configs/debug/wallet_config.json");
        if !cfg_src.exists() {
            bail!("missing wallet debug config in lez repo");
        }
        fs::copy(cfg_src, cfg_dst)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn basecamp_state_roundtrips_all_fields() {
        let tmp = tempdir().expect("tempdir");
        let path = tmp.path().join("basecamp.state");

        let state = BasecampState {
            pin: "deadbeef".to_string(),
            basecamp_bin: "/nix/store/abc/bin/basecamp".to_string(),
            lgpm_bin: "/nix/store/def/bin/lgpm".to_string(),
            sources: vec![
                BasecampSource::Flake(".#lgx".to_string()),
                BasecampSource::Flake("./tictactoe#lgx".to_string()),
                BasecampSource::Path("/abs/path/to/foo.lgx".to_string()),
            ],
        };

        write_basecamp_state(&path, &state).expect("write");
        let loaded = read_basecamp_state(&path).expect("read");

        assert_eq!(loaded.pin, state.pin);
        assert_eq!(loaded.basecamp_bin, state.basecamp_bin);
        assert_eq!(loaded.lgpm_bin, state.lgpm_bin);
        assert_eq!(loaded.sources, state.sources);
    }

    #[test]
    fn basecamp_state_handles_empty_sources_and_partial_fields() {
        let tmp = tempdir().expect("tempdir");
        let path = tmp.path().join("basecamp.state");

        let state = BasecampState {
            pin: "sha1".to_string(),
            basecamp_bin: String::new(),
            lgpm_bin: String::new(),
            sources: vec![],
        };

        write_basecamp_state(&path, &state).expect("write");
        let content = fs::read_to_string(&path).expect("read raw");
        assert!(!content.contains("basecamp_bin="));
        assert!(!content.contains("source:"));

        let loaded = read_basecamp_state(&path).expect("read");
        assert_eq!(loaded.pin, "sha1");
        assert!(loaded.sources.is_empty());
    }
}
