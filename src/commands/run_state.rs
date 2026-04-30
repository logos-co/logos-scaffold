//! Persisted run state used by `cmd_run` for deploy idempotence.
//!
//! The state file at `.scaffold/state/run_deploy.json` records the SHA-256
//! of every guest `.bin` that was last deployed. Before re-deploying,
//! `cmd_run` compares the current hashes against the stored ones and
//! skips the deploy step when they match. `--force-deploy` bypasses the
//! comparison.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::constants::GUEST_BIN_REL_PATH;
use crate::model::Project;
use crate::DynResult;

const RUN_DEPLOY_STATE_REL: &str = ".scaffold/state/run_deploy.json";

#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct RunDeployState {
    pub(crate) program_hashes: BTreeMap<String, String>,
}

pub(crate) fn compute_program_hashes(project: &Project) -> DynResult<BTreeMap<String, String>> {
    let programs_dir = project.root.join("methods/guest/src/bin");
    let bin_dir = project.root.join(GUEST_BIN_REL_PATH);
    let mut out = BTreeMap::new();
    if !programs_dir.exists() {
        return Ok(out);
    }
    for entry in std::fs::read_dir(&programs_dir)
        .with_context(|| format!("read {}", programs_dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let bin_path = bin_dir.join(format!("{stem}.bin"));
        if !bin_path.exists() {
            // Missing artifact — caller treats this as "must deploy".
            continue;
        }
        let bytes = std::fs::read(&bin_path)
            .with_context(|| format!("read {} for hashing", bin_path.display()))?;
        out.insert(stem.to_string(), hex_encode(&Sha256::digest(&bytes)));
    }
    Ok(out)
}

pub(crate) fn load_state(project: &Project) -> RunDeployState {
    let path = state_path(&project.root);
    let Ok(bytes) = std::fs::read(&path) else {
        return RunDeployState::default();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

pub(crate) fn save_state(project: &Project, state: &RunDeployState) -> DynResult<()> {
    let path = state_path(&project.root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let bytes = serde_json::to_vec_pretty(state).context("serialize run_deploy.json")?;
    std::fs::write(&path, bytes).with_context(|| format!("write {}", path.display()))
}

/// `Some(true)` → all known programs hashed and match prior; safe to skip.
/// `Some(false)` → at least one differs / new / removed; must deploy.
/// Empty `current` is treated as "must deploy" (nothing built yet).
pub(crate) fn deploy_can_be_skipped(
    current: &BTreeMap<String, String>,
    prior: &BTreeMap<String, String>,
) -> bool {
    if current.is_empty() || prior.is_empty() {
        return false;
    }
    current == prior
}

fn state_path(project_root: &Path) -> std::path::PathBuf {
    project_root.join(RUN_DEPLOY_STATE_REL)
}

fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deploy_skipped_when_hashes_match() {
        let mut a = BTreeMap::new();
        a.insert("hello".to_string(), "deadbeef".to_string());
        let b = a.clone();
        assert!(deploy_can_be_skipped(&a, &b));
    }

    #[test]
    fn deploy_not_skipped_when_hash_differs() {
        let mut a = BTreeMap::new();
        a.insert("hello".to_string(), "deadbeef".to_string());
        let mut b = BTreeMap::new();
        b.insert("hello".to_string(), "cafebabe".to_string());
        assert!(!deploy_can_be_skipped(&a, &b));
    }

    #[test]
    fn deploy_not_skipped_when_new_program_added() {
        let mut a = BTreeMap::new();
        a.insert("hello".to_string(), "h1".to_string());
        a.insert("counter".to_string(), "h2".to_string());
        let mut b = BTreeMap::new();
        b.insert("hello".to_string(), "h1".to_string());
        assert!(!deploy_can_be_skipped(&a, &b));
    }

    #[test]
    fn deploy_not_skipped_when_either_empty() {
        let a: BTreeMap<String, String> = BTreeMap::new();
        let mut b = BTreeMap::new();
        b.insert("hello".to_string(), "h1".to_string());
        assert!(!deploy_can_be_skipped(&a, &b));
        assert!(!deploy_can_be_skipped(&b, &a));
    }

    #[test]
    fn hex_encode_is_lowercase_padded() {
        assert_eq!(hex_encode(&[0x0a, 0xff, 0x00]), "0aff00");
    }
}
