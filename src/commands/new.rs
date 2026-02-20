use std::env;
use std::fs;
use std::path::PathBuf;

use crate::commands::localnet::cmd_localnet;
use crate::commands::setup::cmd_setup;
use crate::commands::slice::cmd_slice;
use crate::commands::wallet::cmd_wallet;
use crate::config::serialize_config;
use crate::constants::{DEFAULT_LSSA_PIN, DEFAULT_WALLET_BINARY, LSSA_URL, VERSION};
use crate::model::{Config, RepoRef};
use crate::project::{default_cache_root, infer_repo_path, run_in_project_dir};
use crate::repo::sync_repo_to_pin_at_path;
use crate::state::write_text;
use crate::template::copy::{copy_dir_contents, patch_simple_tail_call_program_id};
use crate::template::project::{apply_default_overlay, OverlayRenderContext};
use crate::DynResult;

pub(crate) fn cmd_new(args: &[String]) -> DynResult<()> {
    if args.is_empty() {
        return Err(
            "usage: logos-scaffold new <name> [--vendor-deps] [--lssa-path PATH] [--cache-root PATH] [--bootstrap]"
                .into(),
        );
    }

    let name = args[0].clone();
    let mut vendor_deps = false;
    let mut lssa_path: Option<PathBuf> = None;
    let mut cache_root_override: Option<PathBuf> = None;
    let mut bootstrap = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--vendor-deps" => {
                vendor_deps = true;
                i += 1;
            }
            "--lssa-path" => {
                let value = args.get(i + 1).ok_or("--lssa-path requires value")?;
                lssa_path = Some(PathBuf::from(value));
                i += 2;
            }
            "--cache-root" => {
                let value = args.get(i + 1).ok_or("--cache-root requires value")?;
                cache_root_override = Some(PathBuf::from(value));
                i += 2;
            }
            "--bootstrap" => {
                bootstrap = true;
                i += 1;
            }
            other => return Err(format!("unknown flag for new: {other}").into()),
        }
    }

    let cwd = env::current_dir()?;
    let target = cwd.join(name);
    let crate_name = {
        let fallback = "app";
        let file_name = target
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(fallback);
        to_cargo_crate_name(file_name)
    };

    if target.exists() {
        return Err(format!("target exists: {}", target.display()).into());
    }

    fs::create_dir_all(target.join(".scaffold/state"))?;
    fs::create_dir_all(target.join(".scaffold/logs"))?;

    let cache_root = cache_root_override.unwrap_or(default_cache_root()?);
    fs::create_dir_all(cache_root.join("repos"))?;
    fs::create_dir_all(cache_root.join("state"))?;
    fs::create_dir_all(cache_root.join("logs"))?;
    fs::create_dir_all(cache_root.join("builds"))?;

    let lssa_source = lssa_path
        .or_else(|| infer_repo_path(&cwd, "lssa"))
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| LSSA_URL.to_string());

    let lssa_repo_path = if vendor_deps {
        let root = target.join(".scaffold/repos");
        fs::create_dir_all(&root)?;
        let lssa_vendor = root.join("lssa");
        sync_repo_to_pin_at_path(&lssa_vendor, &lssa_source, DEFAULT_LSSA_PIN, "lssa")?;
        lssa_vendor
    } else {
        let lssa_cached = cache_root.join("repos/lssa");
        sync_repo_to_pin_at_path(&lssa_cached, &lssa_source, DEFAULT_LSSA_PIN, "lssa")?;
        lssa_cached
    };

    let cfg = Config {
        version: VERSION.to_string(),
        cache_root: cache_root.display().to_string(),
        lssa: RepoRef {
            url: LSSA_URL.to_string(),
            source: lssa_source,
            path: lssa_repo_path.display().to_string(),
            pin: DEFAULT_LSSA_PIN.to_string(),
        },
        wallet_binary: DEFAULT_WALLET_BINARY.to_string(),
        wallet_home_dir: ".scaffold/wallet".to_string(),
    };

    let template_root = lssa_repo_path.join("examples/program_deployment");
    if !template_root.exists() {
        return Err(format!("template not found at {}", template_root.display()).into());
    }

    copy_dir_contents(&template_root, &target)?;
    patch_simple_tail_call_program_id(&target)?;
    let overlay_ctx = OverlayRenderContext {
        crate_name: &crate_name,
        lssa_pin: &cfg.lssa.pin,
    };
    apply_default_overlay(&target, &overlay_ctx)?;
    write_text(&target.join("scaffold.toml"), &serialize_config(&cfg))?;

    let old_getting_started = target.join("GETTING_STARTED.md");
    if old_getting_started.exists() {
        fs::remove_file(old_getting_started)?;
    }

    println!(
        "Created logos-scaffold project from template {} at {}",
        template_root.display(),
        target.display()
    );
    println!("Cache root: {}", cfg.cache_root);
    println!("Pinned lssa: {}", cfg.lssa.pin);

    if bootstrap {
        println!("Running bootstrap workflow...");
        run_in_project_dir(Some(&target), || {
            cmd_setup(&[])?;
            let localnet_start = vec!["start".to_string()];
            cmd_localnet(&localnet_start)?;
            let wallet_init = vec!["init".to_string()];
            cmd_wallet(&wallet_init)?;
            let slice_run = vec!["run".to_string()];
            cmd_slice(&slice_run)?;
            Ok(())
        })?;
    }

    Ok(())
}

pub(crate) fn to_cargo_crate_name(input: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in input.chars() {
        let mapped = if ch.is_ascii_alphanumeric() {
            ch.to_ascii_lowercase()
        } else {
            '-'
        };

        if mapped == '-' {
            if !prev_dash {
                out.push('-');
                prev_dash = true;
            }
        } else {
            out.push(mapped);
            prev_dash = false;
        }
    }

    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        "program_deployment".to_string()
    } else {
        out
    }
}
