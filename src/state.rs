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
    // The state file is a line-oriented key=value format. A newline or CR embedded
    // in a value would split the record and silently corrupt state on the next read.
    check_state_value("pin", &state.pin)?;
    check_state_value("basecamp_bin", &state.basecamp_bin)?;
    check_state_value("lgpm_bin", &state.lgpm_bin)?;
    for source in state
        .project_sources
        .iter()
        .chain(state.dependencies.iter())
    {
        let (key, value) = match source {
            BasecampSource::Path(p) => ("project:path", p.as_str()),
            BasecampSource::Flake(f) => ("project:flake", f.as_str()),
        };
        check_state_value(key, value)?;
    }

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
    for source in &state.project_sources {
        match source {
            BasecampSource::Path(p) => content.push_str(&format!("project:path={p}\n")),
            BasecampSource::Flake(f) => content.push_str(&format!("project:flake={f}\n")),
        }
    }
    for source in &state.dependencies {
        match source {
            BasecampSource::Path(p) => content.push_str(&format!("dep:path={p}\n")),
            BasecampSource::Flake(f) => content.push_str(&format!("dep:flake={f}\n")),
        }
    }
    write_text(path, &content)
}

fn check_state_value(key: &str, value: &str) -> DynResult<()> {
    if value.contains(['\n', '\r']) {
        bail!(
            "basecamp state value for `{key}` contains a newline/CR which would corrupt \
             the line-oriented state file: {value:?}"
        );
    }
    Ok(())
}

pub(crate) fn read_basecamp_state(path: &Path) -> DynResult<BasecampState> {
    let mut text = String::new();
    File::open(path)?.read_to_string(&mut text)?;

    let mut state = BasecampState::default();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }

        if let Some(rest) = line.strip_prefix("pin=") {
            state.pin = rest.to_string();
        } else if let Some(rest) = line.strip_prefix("basecamp_bin=") {
            state.basecamp_bin = rest.to_string();
        } else if let Some(rest) = line.strip_prefix("lgpm_bin=") {
            state.lgpm_bin = rest.to_string();
        } else if let Some(rest) = line.strip_prefix("project:path=") {
            state
                .project_sources
                .push(BasecampSource::Path(rest.to_string()));
        } else if let Some(rest) = line.strip_prefix("project:flake=") {
            state
                .project_sources
                .push(BasecampSource::Flake(rest.to_string()));
        } else if let Some(rest) = line.strip_prefix("dep:path=") {
            state
                .dependencies
                .push(BasecampSource::Path(rest.to_string()));
        } else if let Some(rest) = line.strip_prefix("dep:flake=") {
            state
                .dependencies
                .push(BasecampSource::Flake(rest.to_string()));
        } else if let Some(rest) = line.strip_prefix("source:path=") {
            // Legacy key from pre-split state; migrate into project_sources.
            state
                .project_sources
                .push(BasecampSource::Path(rest.to_string()));
        } else if let Some(rest) = line.strip_prefix("source:flake=") {
            // Legacy key from pre-split state; migrate into project_sources.
            state
                .project_sources
                .push(BasecampSource::Flake(rest.to_string()));
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
            project_sources: vec![
                BasecampSource::Flake("path:/abs/tictactoe#lgx".to_string()),
                BasecampSource::Path("/abs/path/to/foo.lgx".to_string()),
            ],
            dependencies: vec![BasecampSource::Flake(
                "github:logos-co/logos-delivery-module/1.0.0#lgx".to_string(),
            )],
        };

        write_basecamp_state(&path, &state).expect("write");
        let loaded = read_basecamp_state(&path).expect("read");

        assert_eq!(loaded.pin, state.pin);
        assert_eq!(loaded.basecamp_bin, state.basecamp_bin);
        assert_eq!(loaded.lgpm_bin, state.lgpm_bin);
        assert_eq!(loaded.project_sources, state.project_sources);
        assert_eq!(loaded.dependencies, state.dependencies);
    }

    #[test]
    fn basecamp_state_handles_empty_sources_and_partial_fields() {
        let tmp = tempdir().expect("tempdir");
        let path = tmp.path().join("basecamp.state");

        let state = BasecampState {
            pin: "sha1".to_string(),
            basecamp_bin: String::new(),
            lgpm_bin: String::new(),
            project_sources: vec![],
            dependencies: vec![],
        };

        write_basecamp_state(&path, &state).expect("write");
        let content = fs::read_to_string(&path).expect("read raw");
        assert!(!content.contains("basecamp_bin="));
        assert!(!content.contains("project:"));
        assert!(!content.contains("dep:"));

        let loaded = read_basecamp_state(&path).expect("read");
        assert_eq!(loaded.pin, "sha1");
        assert!(loaded.project_sources.is_empty());
        assert!(loaded.dependencies.is_empty());
    }

    #[test]
    fn basecamp_state_rejects_newline_in_source_value() {
        let tmp = tempdir().expect("tempdir");
        let path = tmp.path().join("basecamp.state");
        let state = BasecampState {
            pin: "sha1".to_string(),
            basecamp_bin: "/bin/bc".to_string(),
            lgpm_bin: "/bin/lgpm".to_string(),
            project_sources: vec![BasecampSource::Flake("a\nb#lgx".to_string())],
            dependencies: vec![],
        };
        let err = write_basecamp_state(&path, &state).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("newline") && msg.contains("project:flake"),
            "expected newline-rejection error, got: {msg}"
        );
        assert!(
            !path.exists(),
            "state file must not be written on validation failure"
        );
    }

    #[test]
    fn basecamp_state_accepts_legacy_source_keys_as_project_sources() {
        // Pre-split state files have `source:flake=` / `source:path=`.
        // Preserve backward-compat: treat them as project_sources on read.
        let tmp = tempdir().expect("tempdir");
        let path = tmp.path().join("basecamp.state");
        fs::write(
            &path,
            "pin=abc\nsource:flake=path:/p#lgx\nsource:path=/m.lgx\n",
        )
        .unwrap();
        let loaded = read_basecamp_state(&path).expect("read legacy");
        assert_eq!(loaded.pin, "abc");
        assert_eq!(
            loaded.project_sources,
            vec![
                BasecampSource::Flake("path:/p#lgx".to_string()),
                BasecampSource::Path("/m.lgx".to_string()),
            ]
        );
        assert!(loaded.dependencies.is_empty());
    }

    #[test]
    fn basecamp_state_separates_project_and_dep_lines() {
        let tmp = tempdir().expect("tempdir");
        let path = tmp.path().join("basecamp.state");
        let state = BasecampState {
            pin: "abc".to_string(),
            basecamp_bin: String::new(),
            lgpm_bin: String::new(),
            project_sources: vec![BasecampSource::Flake("path:/p#lgx".to_string())],
            dependencies: vec![BasecampSource::Flake(
                "github:logos-co/logos-delivery-module/1.0.0#lgx".to_string(),
            )],
        };
        write_basecamp_state(&path, &state).expect("write");
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("project:flake=path:/p#lgx"));
        assert!(content.contains("dep:flake=github:logos-co/logos-delivery-module/1.0.0#lgx"));
    }
}
