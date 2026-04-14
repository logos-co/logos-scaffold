use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, bail};
use serde::Deserialize;

use crate::constants::BASECAMP_RUNTIME_PORTABLE;
use crate::process::{run_checked, which};
use crate::project::ensure_basecamp_qml_project;
use crate::repo::{sync_repo_to_pin, RepoSyncOptions};
use crate::DynResult;

const RAW_STAGE_REL_PATH: &str = ".scaffold/build/raw/plugins";
const IGNORED_ENTRY_NAMES: &[&str] = &[
    ".git",
    ".scaffold",
    "build",
    "node_modules",
    "result",
    "target",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BasecampBuildArtifact {
    Raw,
    All,
}

#[derive(Debug)]
pub(crate) struct StagedPlugin {
    pub(crate) plugin_name: String,
    pub(crate) stage_dir: PathBuf,
}

#[derive(Debug, Deserialize)]
struct BasecampPluginMetadata {
    name: String,
    #[serde(rename = "type")]
    plugin_type: String,
    view: String,
    #[serde(default)]
    icon: String,
    #[serde(default)]
    main: String,
}

pub(crate) fn parse_basecamp_build_artifact(value: &str) -> DynResult<BasecampBuildArtifact> {
    match value {
        "raw" => Ok(BasecampBuildArtifact::Raw),
        "all" => Ok(BasecampBuildArtifact::All),
        other => bail!("unsupported build artifact `{other}`. Expected `raw` or `all`."),
    }
}

pub(crate) fn cmd_setup_basecamp(project: &mut crate::model::Project) -> DynResult<()> {
    ensure_basecamp_qml_project(project, "logos-scaffold setup")?;
    let repo = project
        .config
        .logos_module_builder
        .as_mut()
        .ok_or_else(|| anyhow!("invalid scaffold.toml: missing [repos.logos_module_builder]"))?;

    fs::create_dir_all(project.root.join(".scaffold/state"))?;
    fs::create_dir_all(project.root.join(".scaffold/logs"))?;
    fs::create_dir_all(project.root.join(".scaffold/build"))?;
    fs::create_dir_all(project.root.join(".scaffold/runtime"))?;
    let cache_root = PathBuf::from(&project.config.cache_root);
    let repo_path = PathBuf::from(&repo.path);
    let sync_opts = if is_cache_managed_repo_path(&cache_root, &repo_path) {
        RepoSyncOptions::auto_reclone_cache_repo()
    } else {
        RepoSyncOptions::fail_on_source_mismatch()
    };
    sync_repo_to_pin(
        repo,
        "logos-module-builder",
        sync_opts,
    )?;

    println!("setup complete");
    println!("  logos-module-builder: {}", repo.path);
    println!("  pin: {}", repo.pin);
    Ok(())
}

pub(crate) fn build_basecamp_qml_project(
    project: &crate::model::Project,
    artifact: BasecampBuildArtifact,
) -> DynResult<()> {
    ensure_basecamp_qml_project(project, "logos-scaffold build")?;
    let staged = stage_raw_plugin_bundle(project)?;

    println!(
        "Raw plugin staged for {} at {}",
        staged.plugin_name,
        staged.stage_dir.display()
    );

    if artifact == BasecampBuildArtifact::All {
        if which("nix").is_none() {
            bail!(
                "Nix is required for `logos-scaffold build --artifact all`.\nNext step: install `nix`, or rerun `logos-scaffold build --artifact raw`."
            );
        }

        run_checked(
            Command::new("nix")
                .current_dir(&project.root)
                .arg("build")
                .arg(".#lgx"),
            "nix build .#lgx",
        )?;
        run_checked(
            Command::new("nix")
                .current_dir(&project.root)
                .arg("build")
                .arg(".#lgx-portable"),
            "nix build .#lgx-portable",
        )?;
    }

    Ok(())
}

pub(crate) fn install_basecamp_qml_project(project: &crate::model::Project) -> DynResult<()> {
    ensure_basecamp_qml_project(project, "logos-scaffold install")?;
    let metadata = load_plugin_metadata(project)?;
    let staged_dir = raw_stage_root(project).join(&metadata.name);
    if !staged_dir.exists() {
        bail!(
            "missing staged raw plugin at {}\nNext step: run `logos-scaffold build` first.",
            staged_dir.display()
        );
    }

    let target_dir = resolve_runtime_plugin_dir(project, &metadata.name);
    if target_dir.exists() {
        fs::remove_dir_all(&target_dir)?;
    }
    copy_dir_recursive(&staged_dir, &target_dir)?;

    println!("Installed Basecamp plugin");
    println!("  Plugin: {}", metadata.name);
    println!("  Target: {}", target_dir.display());
    println!("  LOGOS_DATA_DIR={}", resolve_data_root(project).display());
    println!("  Launch Basecamp with that `LOGOS_DATA_DIR` to load the plugin.");
    Ok(())
}

pub(crate) fn resolve_runtime_plugin_dir(
    project: &crate::model::Project,
    plugin_name: &str,
) -> PathBuf {
    runtime_base_dir(project).join("plugins").join(plugin_name)
}

pub(crate) fn resolve_data_root(project: &crate::model::Project) -> PathBuf {
    let configured = PathBuf::from(&project.config.basecamp.data_root);
    if configured.is_absolute() {
        configured
    } else {
        project.root.join(configured)
    }
}

pub(crate) fn runtime_base_dir(project: &crate::model::Project) -> PathBuf {
    let data_root = resolve_data_root(project);
    if project.config.basecamp.runtime_variant == BASECAMP_RUNTIME_PORTABLE {
        return data_root;
    }

    PathBuf::from(format!("{}Dev", data_root.to_string_lossy()))
}

pub(crate) fn raw_stage_root(project: &crate::model::Project) -> PathBuf {
    project.root.join(RAW_STAGE_REL_PATH)
}

fn stage_raw_plugin_bundle(project: &crate::model::Project) -> DynResult<StagedPlugin> {
    let metadata = load_plugin_metadata(project)?;
    validate_metadata_files(project, &metadata)?;
    maybe_run_qmllint(project, &metadata.view)?;

    let stage_root = raw_stage_root(project);
    fs::create_dir_all(&stage_root)?;

    let stage_dir = stage_root.join(&metadata.name);
    if stage_dir.exists() {
        fs::remove_dir_all(&stage_dir)?;
    }
    copy_project_tree(&project.root, &stage_dir, Path::new(""))?;

    Ok(StagedPlugin {
        plugin_name: metadata.name,
        stage_dir,
    })
}

fn load_plugin_metadata(project: &crate::model::Project) -> DynResult<BasecampPluginMetadata> {
    let path = project.root.join("metadata.json");
    let text = fs::read_to_string(&path)?;
    let metadata: BasecampPluginMetadata = serde_json::from_str(&text)
        .map_err(|err| anyhow!("invalid metadata.json at {}: {err}", path.display()))?;

    if metadata.name.trim().is_empty() {
        bail!("metadata.json must contain a non-empty `name`");
    }
    if metadata.plugin_type != "ui_qml" {
        bail!(
            "basecamp-qml v1 only supports `type: \"ui_qml\"`; found `{}`",
            metadata.plugin_type
        );
    }
    if metadata.view.trim().is_empty() {
        bail!("metadata.json must contain a non-empty `view`");
    }
    if !metadata.main.trim().is_empty() {
        bail!(
            "basecamp-qml v1 only supports QML-only plugins. Remove `main` from metadata.json before building."
        );
    }

    Ok(metadata)
}

fn validate_metadata_files(
    project: &crate::model::Project,
    metadata: &BasecampPluginMetadata,
) -> DynResult<()> {
    let view_path = project.root.join(&metadata.view);
    if !view_path.exists() {
        bail!("QML entry file not found at {}", view_path.display());
    }

    if !metadata.icon.trim().is_empty() {
        let icon_path = project.root.join(&metadata.icon);
        if !icon_path.exists() {
            bail!("plugin icon not found at {}", icon_path.display());
        }
    }

    Ok(())
}

fn maybe_run_qmllint(project: &crate::model::Project, view: &str) -> DynResult<()> {
    let Some(qmllint) = which("qmllint") else {
        println!("warning: `qmllint` not found in PATH; skipping QML lint");
        return Ok(());
    };

    let output = Command::new(qmllint)
        .current_dir(&project.root)
        .arg(view)
        .output()?;

    if output.status.success() {
        return Ok(());
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    println!("warning: `qmllint {view}` reported issues");
    if !stdout.is_empty() {
        println!("{stdout}");
    }
    if !stderr.is_empty() {
        println!("{stderr}");
    }
    Ok(())
}

fn copy_project_tree(src_root: &Path, dst_root: &Path, relative: &Path) -> DynResult<()> {
    let src_dir = src_root.join(relative);
    fs::create_dir_all(dst_root)?;

    for entry in fs::read_dir(&src_dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let rel_path = relative.join(&name);
        let src_path = src_root.join(&rel_path);
        let dst_path = dst_root.join(&name);

        if should_skip_entry(&name) {
            continue;
        }

        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            copy_project_tree(src_root, &dst_path, &rel_path)?;
        } else if file_type.is_file() {
            if let Some(parent) = dst_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(src_path, dst_path)?;
        }
    }

    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> DynResult<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else if file_type.is_file() {
            if let Some(parent) = dst_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(src_path, dst_path)?;
        }
    }

    Ok(())
}

fn should_skip_entry(name: &OsStr) -> bool {
    let value = name.to_string_lossy();
    if IGNORED_ENTRY_NAMES.contains(&value.as_ref()) {
        return true;
    }
    value.starts_with("result-")
}

fn is_cache_managed_repo_path(cache_root: &Path, repo_path: &Path) -> bool {
    let cache_repos = normalize_path(cache_root).join("repos");
    let repo = normalize_path(repo_path);
    repo.starts_with(cache_repos)
}

fn normalize_path(path: &Path) -> PathBuf {
    if let Ok(canonical) = path.canonicalize() {
        return canonical;
    }

    if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    }
}
