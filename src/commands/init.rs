use std::env;
use std::fs;
use std::path::Path;

use anyhow::{bail, Context};

use crate::config::{escape_toml_string, serialize_config};
use crate::constants::{
    DEFAULT_FRAMEWORK_IDL_PATH, DEFAULT_FRAMEWORK_IDL_SPEC, DEFAULT_FRAMEWORK_VERSION, DEFAULT_LEZ,
    DEFAULT_SPEL, FRAMEWORK_KIND_DEFAULT, LEZ_URL, SPEL_URL, VERSION,
};
use crate::model::{Config, FrameworkConfig, FrameworkIdlConfig, LocalnetConfig, RepoRef};
use crate::project::default_cache_root;
use crate::state::write_text;
use crate::template::project::ensure_scaffold_in_gitignore;
use crate::DynResult;

pub(crate) fn cmd_init(bin_name: &str) -> DynResult<()> {
    let cwd = env::current_dir()?;
    cmd_init_at(&cwd, bin_name)
}

pub(crate) fn cmd_init_at(target: &Path, bin_name: &str) -> DynResult<()> {
    let scaffold_path = target.join("scaffold.toml");
    if scaffold_path.exists() {
        // Migration: a pre-spel scaffold.toml lacks the `[repos.spel]`
        // section. Append it in place and stop — never overwrite the
        // user's customized fields. We do NOT round-trip through
        // serialize_config here because that would reformat the file and
        // discard comments / cache_root help text.
        let existing = fs::read_to_string(&scaffold_path).with_context(|| {
            format!(
                "reading existing scaffold.toml at {}",
                scaffold_path.display()
            )
        })?;
        if existing.contains("[repos.spel]") {
            bail!(
                "scaffold.toml already exists in {} and is up to date; refusing to overwrite",
                target.display()
            );
        }
        // Place the vendored spel co-located with the existing lez clone so
        // the migration honors the project's actual layout (--cache-root
        // override, --vendor-deps `.scaffold/repos/`, or default cache root)
        // rather than silently writing a wrong default. Falls back to the
        // bootstrap cache only when the existing file has no parseable lez
        // path — that case shouldn't happen in real projects but keeps the
        // migration robust against hand-edited configs.
        let spel_path = derive_spel_path_from_lez_path(&existing).unwrap_or_else(|| {
            default_cache_root()
                .map(|(cache, _)| {
                    cache
                        .join("repos/spel")
                        .join(DEFAULT_SPEL.sha)
                        .display()
                        .to_string()
                })
                .unwrap_or_else(|_| "spel".to_string())
        });
        let separator = if existing.ends_with('\n') {
            "\n"
        } else {
            "\n\n"
        };
        let appended = format!(
            "{}{}[repos.spel]\nurl = \"{}\"\nsource = \"{}\"\npath = \"{}\"\npin = \"{}\"\n",
            existing,
            separator,
            escape_toml_string(SPEL_URL),
            escape_toml_string(SPEL_URL),
            escape_toml_string(&spel_path),
            escape_toml_string(DEFAULT_SPEL.sha),
        );
        write_text(&scaffold_path, &appended)?;
        println!(
            "scaffold.toml in {} backfilled with [repos.spel] (path={}). Run `{bin_name} setup` to clone and build the vendored spel.",
            target.display(),
            spel_path,
        );
        return Ok(());
    }

    let (bootstrap_cache, _) = default_cache_root()?;
    let lez_path = bootstrap_cache
        .join("repos/lez")
        .join(DEFAULT_LEZ.sha)
        .display()
        .to_string();
    let spel_path = bootstrap_cache
        .join("repos/spel")
        .join(DEFAULT_SPEL.sha)
        .display()
        .to_string();

    let cfg = Config {
        version: VERSION.to_string(),
        cache_root: String::new(),
        lez: RepoRef {
            url: LEZ_URL.to_string(),
            source: LEZ_URL.to_string(),
            path: lez_path,
            pin: DEFAULT_LEZ.sha.to_string(),
        },
        spel: RepoRef {
            url: SPEL_URL.to_string(),
            source: SPEL_URL.to_string(),
            path: spel_path,
            pin: DEFAULT_SPEL.sha.to_string(),
        },
        wallet_home_dir: ".scaffold/wallet".to_string(),
        framework: FrameworkConfig {
            kind: FRAMEWORK_KIND_DEFAULT.to_string(),
            version: DEFAULT_FRAMEWORK_VERSION.to_string(),
            idl: FrameworkIdlConfig {
                spec: DEFAULT_FRAMEWORK_IDL_SPEC.to_string(),
                path: DEFAULT_FRAMEWORK_IDL_PATH.to_string(),
            },
        },
        localnet: LocalnetConfig::default(),
        basecamp: None,
    };

    write_text(&scaffold_path, &serialize_config(&cfg)?)?;
    fs::create_dir_all(target.join(".scaffold/state"))
        .with_context(|| format!("creating {}/.scaffold/state", target.display()))?;
    fs::create_dir_all(target.join(".scaffold/logs"))
        .with_context(|| format!("creating {}/.scaffold/logs", target.display()))?;
    ensure_scaffold_in_gitignore(target)?;

    println!(
        "scaffold.toml created at {}. Run '{bin_name} setup' to clone LEZ and build dependencies.",
        scaffold_path.display()
    );
    println!(
        "If this project is building modules for basecamp, run '{bin_name} basecamp setup' to pin + build basecamp + lgpm and seed alice/bob profiles."
    );

    Ok(())
}

/// Read the existing scaffold.toml text and derive a sibling `spel` path next
/// to the configured `[repos.lez].path` so the migration honors the same
/// layout (cache-root override or `.scaffold/repos/` vendor mode) instead of
/// pinning the spel clone under a different root than lez. Returns `None` if
/// no usable `[repos.lez].path` line is found — caller falls back to the
/// bootstrap cache. This is deliberately a hand-rolled scan rather than a
/// `parse_config` call because the parser hard-fails on the missing
/// `[repos.spel]` we're about to add.
fn derive_spel_path_from_lez_path(scaffold_toml: &str) -> Option<String> {
    let mut in_lez_section = false;
    for raw in scaffold_toml.lines() {
        let line = raw.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_lez_section = matches!(&line[1..line.len() - 1], "repos.lez" | "repos.lssa");
            continue;
        }
        if !in_lez_section {
            continue;
        }
        if let Some(rest) = line.strip_prefix("path") {
            let rest = rest.trim_start();
            let rest = rest.strip_prefix('=')?.trim();
            let unquoted = if rest.starts_with('"') && rest.ends_with('"') && rest.len() >= 2 {
                &rest[1..rest.len() - 1]
            } else {
                rest
            };
            if unquoted.is_empty() {
                return None;
            }
            // Co-locate spel beside lez. Two layouts in the wild:
            //   1. cache-managed:    .../repos/lez/<lez_pin>     → .../repos/spel/<spel_pin>
            //   2. vendored (--vendor-deps): .scaffold/repos/lez → .scaffold/repos/spel
            // Operate on the raw string (split by `/`) rather than PathBuf
            // components so absolute Linux paths don't double the leading
            // slash when re-joined.
            let sep = '/';
            let mut segments: Vec<String> = unquoted.split(sep).map(|s| s.to_string()).collect();
            for i in (0..segments.len()).rev() {
                if segments[i] == "lez" {
                    segments[i] = "spel".to_string();
                    // If the segment immediately after is a pin (bootstrap-
                    // cache layout), swap the pin too.
                    if i + 1 < segments.len() && !segments[i + 1].is_empty() {
                        segments[i + 1] = DEFAULT_SPEL.sha.to_string();
                    }
                    return Some(segments.join(&sep.to_string()));
                }
            }
            return None;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::parse_config;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn init_writes_parseable_scaffold_toml() {
        let temp = tempdir().expect("tempdir");
        let target = temp.path();
        cmd_init_at(target, "lgs").expect("init");

        let text = fs::read_to_string(target.join("scaffold.toml")).expect("read scaffold.toml");
        let cfg = parse_config(&text).expect("parse scaffold.toml");

        assert_eq!(cfg.version, VERSION);
        assert_eq!(cfg.lez.pin, DEFAULT_LEZ.sha);
        assert_eq!(cfg.framework.kind, FRAMEWORK_KIND_DEFAULT);
        assert_eq!(cfg.wallet_home_dir, ".scaffold/wallet");
        assert_eq!(cfg.localnet.port, 3040);
        assert!(cfg.localnet.risc0_dev_mode);
    }

    #[test]
    fn init_does_not_persist_cache_root() {
        let temp = tempdir().expect("tempdir");
        let target = temp.path();
        cmd_init_at(target, "lgs").expect("init");

        let text = fs::read_to_string(target.join("scaffold.toml")).expect("read scaffold.toml");
        let has_active_cache_root = text
            .lines()
            .any(|l| !l.trim_start().starts_with('#') && l.contains("cache_root"));
        assert!(
            !has_active_cache_root,
            "scaffold.toml should not pin cache_root by default; got:\n{text}"
        );

        let cfg = parse_config(&text).expect("parse scaffold.toml");
        assert!(
            cfg.cache_root.is_empty(),
            "parsed cache_root should be empty; got {:?}",
            cfg.cache_root
        );
    }

    #[test]
    fn init_refuses_when_scaffold_toml_already_has_repos_spel() {
        let temp = tempdir().expect("tempdir");
        let target = temp.path();
        // A "complete" pre-existing config — already migrated, has [repos.spel].
        let seed =
            "# existing\n[repos.spel]\nurl = \"x\"\nsource = \"x\"\npath = \"y\"\npin = \"z\"\n";
        fs::write(target.join("scaffold.toml"), seed).expect("seed");

        let err = cmd_init_at(target, "lgs").expect_err("should refuse");
        assert!(
            err.to_string().contains("up to date"),
            "unexpected error: {err}"
        );

        let after = fs::read_to_string(target.join("scaffold.toml")).unwrap();
        assert_eq!(after, seed);
    }

    #[test]
    fn init_backfills_repos_spel_in_existing_scaffold_toml() {
        let temp = tempdir().expect("tempdir");
        let target = temp.path();
        // A pre-spel scaffold.toml: has lez at a custom cache_root path.
        let seed = r#"[scaffold]
version = "0.1.0"
cache_root = "/custom/cache"

[repos.lez]
url = "u"
source = "u"
path = "/custom/cache/repos/lez/abc123"
pin = "abc123"

[wallet]
home_dir = ".scaffold/wallet"
"#;
        fs::write(target.join("scaffold.toml"), seed).expect("seed");

        cmd_init_at(target, "lgs").expect("init backfills");

        let after = fs::read_to_string(target.join("scaffold.toml")).unwrap();
        assert!(
            after.starts_with(seed),
            "existing content must be preserved verbatim; got:\n{after}"
        );
        assert!(
            after.contains("[repos.spel]"),
            "[repos.spel] section must be appended; got:\n{after}"
        );
        assert!(
            after.contains(DEFAULT_SPEL.sha),
            "default spel pin must be present; got:\n{after}"
        );
        // Co-location: backfill must derive spel.path from the existing
        // lez.path, not from default_cache_root() (the C1 bug).
        let expected_spel_path = format!("/custom/cache/repos/spel/{}", DEFAULT_SPEL.sha);
        assert!(
            after.contains(&expected_spel_path),
            "spel path must mirror the lez layout; expected {expected_spel_path} in:\n{after}"
        );
    }

    #[test]
    fn derive_spel_path_handles_bootstrap_cache_layout() {
        let toml = r#"[repos.lez]
path = "/home/u/.cache/logos-scaffold/repos/lez/deadbeef"
pin = "deadbeef"
"#;
        let derived = derive_spel_path_from_lez_path(toml).expect("derived");
        let expected = format!(
            "/home/u/.cache/logos-scaffold/repos/spel/{}",
            DEFAULT_SPEL.sha
        );
        assert_eq!(derived, expected);
    }

    #[test]
    fn derive_spel_path_handles_vendored_layout() {
        let toml = r#"[repos.lez]
path = ".scaffold/repos/lez"
pin = "deadbeef"
"#;
        let derived = derive_spel_path_from_lez_path(toml).expect("derived");
        // Vendored (--vendor-deps) projects have no pin under the lez dir;
        // we mirror the layout literally.
        assert_eq!(derived, ".scaffold/repos/spel");
    }

    #[test]
    fn derive_spel_path_returns_none_when_no_lez_path() {
        let toml = r#"[wallet]
home_dir = ".scaffold/wallet"
"#;
        assert!(derive_spel_path_from_lez_path(toml).is_none());
    }

    #[test]
    fn derive_spel_path_accepts_legacy_repos_lssa() {
        let toml = r#"[repos.lssa]
path = "/old/cache/repos/lez/abc"
pin = "abc"
"#;
        let derived = derive_spel_path_from_lez_path(toml).expect("derived");
        assert!(derived.contains("/old/cache/repos/spel/"));
    }

    #[test]
    fn init_creates_scaffold_state_and_logs_dirs() {
        let temp = tempdir().expect("tempdir");
        let target = temp.path();
        cmd_init_at(target, "lgs").expect("init");

        assert!(target.join(".scaffold/state").is_dir());
        assert!(target.join(".scaffold/logs").is_dir());
    }

    #[test]
    fn init_gitignore_is_idempotent_with_existing_scaffold_line() {
        let temp = tempdir().expect("tempdir");
        let target = temp.path();
        fs::write(target.join(".gitignore"), "target\n.scaffold\n").expect("seed");

        cmd_init_at(target, "lgs").expect("init");

        let text = fs::read_to_string(target.join(".gitignore")).unwrap();
        let count = text.lines().filter(|l| l.trim() == ".scaffold").count();
        assert_eq!(count, 1);
    }
}
