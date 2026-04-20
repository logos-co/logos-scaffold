use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context};

use crate::constants::{
    BASECAMP_PROFILE_ALICE, BASECAMP_PROFILE_BOB, BASECAMP_URL, BASECAMP_XDG_APP_SUBPATH,
    DEFAULT_BASECAMP_PIN, DEFAULT_LGPM_FLAKE,
};
use crate::model::{BasecampState, Project, RepoRef};
use crate::process::run_checked;
use crate::project::{load_project, save_project_config};
use crate::repo::{sync_repo_to_pin, RepoSyncOptions};
use crate::state::{read_basecamp_state, write_basecamp_state};
use crate::DynResult;

#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields wired up in later phases
pub(crate) enum BasecampAction {
    Setup,
    Install {
        path: Option<PathBuf>,
        flake: Option<String>,
        profile: Option<String>,
    },
    Launch {
        profile: String,
        no_clean: bool,
    },
    ProfileList {
        json: bool,
    },
}

pub(crate) fn cmd_basecamp(action: BasecampAction) -> DynResult<()> {
    let project = load_project().context(
        "This command must be run inside a logos-scaffold project.\nNext step: cd into your scaffolded project directory and retry.",
    )?;

    match action {
        BasecampAction::Setup => cmd_basecamp_setup(project),
        BasecampAction::Install { .. } => bail!("basecamp install is not yet implemented"),
        BasecampAction::Launch { .. } => bail!("basecamp launch is not yet implemented"),
        BasecampAction::ProfileList { .. } => bail!("basecamp profile list is not yet implemented"),
    }
}

fn cmd_basecamp_setup(mut project: Project) -> DynResult<()> {
    let mut bc = project.config.basecamp.clone().unwrap_or_default();
    if bc.source.is_empty() {
        bc.source = BASECAMP_URL.to_string();
    }
    if bc.pin.is_empty() {
        bc.pin = DEFAULT_BASECAMP_PIN.to_string();
    }
    if bc.pin.is_empty() {
        bail!(
            "basecamp pin is not set.\n\
             Next step: add `pin = \"<commit-sha>\"` under `[basecamp]` in scaffold.toml \
             (see docs/specs/basecamp-profiles.md §3.3) and retry."
        );
    }

    let cache_root = project.root.join(&project.config.cache_root);
    let basecamp_repo_path = cache_root.join("repos/basecamp").join(&bc.pin);

    println!("cloning basecamp at {}", &bc.pin);
    let mut repo_ref = RepoRef {
        url: bc.source.clone(),
        source: bc.source.clone(),
        path: basecamp_repo_path.display().to_string(),
        pin: bc.pin.clone(),
    };
    sync_repo_to_pin(
        &mut repo_ref,
        "basecamp",
        RepoSyncOptions::auto_reclone_cache_repo(),
    )?;
    bc.pin = repo_ref.pin.clone();

    let pin_artifacts = cache_root.join("basecamp").join(&bc.pin);
    fs::create_dir_all(&pin_artifacts)
        .with_context(|| format!("create {}", pin_artifacts.display()))?;

    let basecamp_bin = build_basecamp_app(&basecamp_repo_path, &pin_artifacts)?;
    let lgpm_bin = build_lgpm(&pin_artifacts, bc.lgpm_flake.as_str())?;

    let profiles_root = project.root.join(".scaffold/basecamp/profiles");
    let seeded = seed_profiles(
        &profiles_root,
        &[BASECAMP_PROFILE_ALICE, BASECAMP_PROFILE_BOB],
    )?;
    println!("seeded profiles: {}", seeded.join(", "));

    let state_path = project.root.join(".scaffold/state/basecamp.state");
    let existing = read_basecamp_state(&state_path).unwrap_or_default();
    let state = BasecampState {
        pin: bc.pin.clone(),
        basecamp_bin: basecamp_bin.display().to_string(),
        lgpm_bin: lgpm_bin.display().to_string(),
        sources: existing.sources,
    };
    write_basecamp_state(&state_path, &state)?;

    project.config.basecamp = Some(bc);
    save_project_config(&project)?;

    println!("setup complete");
    Ok(())
}

fn build_basecamp_app(repo: &Path, out_dir: &Path) -> DynResult<PathBuf> {
    println!("building basecamp");
    let link = out_dir.join("app-result");
    run_checked(
        Command::new("nix")
            .current_dir(repo)
            .arg("build")
            .arg(".#app")
            .arg("--out-link")
            .arg(&link),
        "nix build .#app (basecamp)",
    )?;
    Ok(resolve_basecamp_binary(&link)?)
}

fn build_lgpm(out_dir: &Path, override_flake: &str) -> DynResult<PathBuf> {
    println!("building lgpm");
    let link = out_dir.join("lgpm-result");
    let flake_ref = if override_flake.is_empty() {
        DEFAULT_LGPM_FLAKE.to_string()
    } else {
        override_flake.to_string()
    };
    run_checked(
        Command::new("nix")
            .arg("build")
            .arg(&flake_ref)
            .arg("--out-link")
            .arg(&link),
        &format!("nix build {flake_ref} (lgpm)"),
    )?;
    Ok(link.join("bin/lgpm"))
}

fn resolve_basecamp_binary(app_link: &Path) -> DynResult<PathBuf> {
    // v0.1.1 layout: bin/logos-basecamp (Linux); macOS app bundle ships under Applications/.
    for rel in ["bin/logos-basecamp", "bin/LogosBasecamp", "bin/basecamp"] {
        let candidate = app_link.join(rel);
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    bail!(
        "could not locate basecamp binary inside nix build result {}",
        app_link.display()
    )
}

/// Create XDG-rooted profile dirs under `profiles_root` for every named profile.
/// Returns the list of profile names that now exist (idempotent).
fn seed_profiles(profiles_root: &Path, names: &[&str]) -> DynResult<Vec<String>> {
    let mut seeded = Vec::new();
    for name in names {
        let profile_dir = profiles_root.join(name);
        for xdg in ["xdg-config", "xdg-data", "xdg-cache"] {
            let path = profile_dir.join(xdg).join(BASECAMP_XDG_APP_SUBPATH);
            fs::create_dir_all(&path)
                .with_context(|| format!("create profile dir {}", path.display()))?;
        }
        seeded.push(name.to_string());
    }
    Ok(seeded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn seed_profiles_creates_xdg_subdirs_for_each_name() {
        let tmp = tempdir().expect("tempdir");
        let root = tmp.path().join("profiles");

        let names = ["alice", "bob"];
        let seeded = seed_profiles(&root, &names).expect("seed");
        assert_eq!(seeded, vec!["alice".to_string(), "bob".to_string()]);

        for name in names {
            for xdg in ["xdg-config", "xdg-data", "xdg-cache"] {
                let dir = root
                    .join(name)
                    .join(xdg)
                    .join(BASECAMP_XDG_APP_SUBPATH);
                assert!(
                    dir.is_dir(),
                    "expected XDG subdir at {}",
                    dir.display()
                );
            }
        }
    }

    #[test]
    fn seed_profiles_is_idempotent() {
        let tmp = tempdir().expect("tempdir");
        let root = tmp.path().join("profiles");
        seed_profiles(&root, &["alice"]).expect("first");
        // Drop a sentinel file inside the xdg-data dir; a second seed must not delete it.
        let sentinel = root
            .join("alice/xdg-data")
            .join(BASECAMP_XDG_APP_SUBPATH)
            .join("keep-me.txt");
        fs::write(&sentinel, b"hi").expect("write sentinel");
        seed_profiles(&root, &["alice"]).expect("second");
        assert!(sentinel.exists(), "second seed must not scrub existing contents");
    }
}
