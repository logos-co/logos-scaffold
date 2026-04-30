use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail};

use crate::config::{parse_config, serialize_config};
use crate::model::Project;
use crate::state::write_text_atomic;
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
    write_text_atomic(
        &project.root.join("scaffold.toml"),
        &serialize_config(&project.config)?,
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

/// Layer of the cache_root resolution chain that supplied the active value.
/// Surfaced by `lgs doctor` so CI users can confirm which layer won.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CacheRootSource {
    Env,
    Config,
    XdgCacheHome,
    HomeCache,
    MacOsCaches,
    WindowsLocalAppData,
}

impl CacheRootSource {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Env => "LOGOS_SCAFFOLD_CACHE_ROOT",
            Self::Config => "scaffold.toml [scaffold].cache_root",
            Self::XdgCacheHome => "$XDG_CACHE_HOME",
            Self::HomeCache => "$HOME/.cache",
            Self::MacOsCaches => "$HOME/Library/Caches",
            Self::WindowsLocalAppData => "%LOCALAPPDATA%",
        }
    }
}

/// Resolves `cache_root` by trying, in order:
/// 1. `LOGOS_SCAFFOLD_CACHE_ROOT` env var (non-empty),
/// 2. `[scaffold].cache_root` from `scaffold.toml` if set (relative values are
///    joined against `project.root`, so they resolve the same regardless of CWD),
/// 3. `default_cache_root()` — XDG / HOME / platform fallback.
///
/// The companion `source` is returned so `lgs doctor` can print which layer won.
pub(crate) fn resolve_cache_root(project: &Project) -> DynResult<(PathBuf, CacheRootSource)> {
    if let Ok(val) = env::var("LOGOS_SCAFFOLD_CACHE_ROOT") {
        if !val.is_empty() {
            return Ok((PathBuf::from(val), CacheRootSource::Env));
        }
    }

    if !project.config.cache_root.is_empty() {
        return Ok((
            project.root.join(&project.config.cache_root),
            CacheRootSource::Config,
        ));
    }

    default_cache_root()
}

/// Platform-default cache root when neither env nor `scaffold.toml` set one.
/// Returns the source layer alongside the path.
pub(crate) fn default_cache_root() -> DynResult<(PathBuf, CacheRootSource)> {
    let home = home_dir()?;
    if cfg!(target_os = "macos") {
        return Ok((
            home.join("Library/Caches/logos-scaffold"),
            CacheRootSource::MacOsCaches,
        ));
    }

    if cfg!(target_os = "windows") {
        if let Ok(local_app_data) = env::var("LOCALAPPDATA") {
            return Ok((
                PathBuf::from(local_app_data).join("logos-scaffold/Cache"),
                CacheRootSource::WindowsLocalAppData,
            ));
        }
    }

    if let Ok(xdg) = env::var("XDG_CACHE_HOME") {
        return Ok((
            PathBuf::from(xdg).join("logos-scaffold"),
            CacheRootSource::XdgCacheHome,
        ));
    }

    Ok((
        home.join(".cache/logos-scaffold"),
        CacheRootSource::HomeCache,
    ))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Config, FrameworkConfig, FrameworkIdlConfig, LocalnetConfig, RepoRef};
    use std::sync::Mutex;

    // Tests in this module mutate process-wide env vars; run them under one lock.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn fixture_project(root: PathBuf, cache_root: &str) -> Project {
        Project {
            root,
            config: Config {
                version: "0.1.0".into(),
                cache_root: cache_root.to_string(),
                lez: RepoRef {
                    url: String::new(),
                    source: String::new(),
                    path: String::new(),
                    pin: String::new(),
                },
                wallet_home_dir: ".scaffold/wallet".into(),
                framework: FrameworkConfig {
                    kind: String::new(),
                    version: String::new(),
                    idl: FrameworkIdlConfig {
                        spec: String::new(),
                        path: String::new(),
                    },
                },
                localnet: LocalnetConfig::default(),
                run: crate::model::RunConfig::default(),
                basecamp: None,
            },
        }
    }

    #[test]
    fn env_layer_wins_over_config() {
        let _guard = ENV_LOCK.lock().unwrap();
        env::set_var("LOGOS_SCAFFOLD_CACHE_ROOT", "/tmp/from-env");
        let project = fixture_project(PathBuf::from("/proj"), "should-be-ignored");
        let (path, source) = resolve_cache_root(&project).expect("resolve");
        env::remove_var("LOGOS_SCAFFOLD_CACHE_ROOT");

        assert_eq!(path, PathBuf::from("/tmp/from-env"));
        assert_eq!(source, CacheRootSource::Env);
    }

    #[test]
    fn config_layer_joins_relative_value_against_project_root() {
        let _guard = ENV_LOCK.lock().unwrap();
        env::remove_var("LOGOS_SCAFFOLD_CACHE_ROOT");
        let project = fixture_project(PathBuf::from("/proj"), ".scaffold/cache");
        let (path, source) = resolve_cache_root(&project).expect("resolve");

        assert_eq!(path, PathBuf::from("/proj/.scaffold/cache"));
        assert_eq!(source, CacheRootSource::Config);
    }

    #[test]
    fn config_layer_honors_absolute_value() {
        let _guard = ENV_LOCK.lock().unwrap();
        env::remove_var("LOGOS_SCAFFOLD_CACHE_ROOT");
        let project = fixture_project(PathBuf::from("/proj"), "/abs/cache");
        let (path, source) = resolve_cache_root(&project).expect("resolve");

        assert_eq!(path, PathBuf::from("/abs/cache"));
        assert_eq!(source, CacheRootSource::Config);
    }

    #[test]
    fn falls_through_to_default_when_env_and_config_empty() {
        let _guard = ENV_LOCK.lock().unwrap();
        env::remove_var("LOGOS_SCAFFOLD_CACHE_ROOT");
        let project = fixture_project(PathBuf::from("/proj"), "");
        let (_, source) = resolve_cache_root(&project).expect("resolve");

        assert!(
            matches!(
                source,
                CacheRootSource::XdgCacheHome
                    | CacheRootSource::HomeCache
                    | CacheRootSource::MacOsCaches
                    | CacheRootSource::WindowsLocalAppData
            ),
            "expected a default layer, got {source:?}"
        );
    }
}
