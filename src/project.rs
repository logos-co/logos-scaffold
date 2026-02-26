use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail};

use crate::config::{parse_config, serialize_config};
use crate::model::Project;
use crate::state::write_text;
use crate::DynResult;

pub(crate) fn load_project() -> DynResult<Project> {
    let cwd = env::current_dir()?;
    let root = find_project_root(cwd.clone()).ok_or_else(|| {
        anyhow!(
            "Not a logos-scaffold project at {}. Run `logos-scaffold create <name>` (or `logos-scaffold new <name>`) first.",
            cwd.display()
        )
    })?;

    let config_path = root.join("scaffold.toml");
    let cfg_text = fs::read_to_string(&config_path)?;
    let cfg = parse_config(&cfg_text)?;
    Ok(Project { root, config: cfg })
}

pub(crate) fn run_in_project_dir(
    path: Option<&Path>,
    op: impl FnOnce() -> DynResult<()>,
) -> DynResult<()> {
    let original = env::current_dir()?;
    if let Some(path) = path {
        env::set_current_dir(path)?;
    }
    let result = op();
    let _ = env::set_current_dir(original);
    result
}

pub(crate) fn save_project_config(project: &Project) -> DynResult<()> {
    write_text(
        &project.root.join("scaffold.toml"),
        &serialize_config(&project.config),
    )
}

pub(crate) fn find_project_root(mut dir: PathBuf) -> Option<PathBuf> {
    loop {
        if dir.join("scaffold.toml").exists() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

pub(crate) fn default_cache_root() -> DynResult<PathBuf> {
    let home = home_dir()?;
    if cfg!(target_os = "macos") {
        return Ok(home.join("Library/Caches/logos-scaffold"));
    }

    if cfg!(target_os = "windows") {
        if let Ok(local_app_data) = env::var("LOCALAPPDATA") {
            return Ok(PathBuf::from(local_app_data).join("logos-scaffold/Cache"));
        }
    }

    if let Ok(xdg) = env::var("XDG_CACHE_HOME") {
        return Ok(PathBuf::from(xdg).join("logos-scaffold"));
    }

    Ok(home.join(".cache/logos-scaffold"))
}

pub(crate) fn home_dir() -> DynResult<PathBuf> {
    if let Ok(home) = env::var("HOME") {
        return Ok(PathBuf::from(home));
    }
    bail!("HOME is not set")
}

pub(crate) fn ensure_dir_exists(path: &Path, label: &str) -> DynResult<()> {
    if !path.exists() {
        bail!("missing {label} at {}", path.display());
    }
    Ok(())
}
