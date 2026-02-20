use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::serialize_config;
use crate::constants::{
    DEFAULT_FRAMEWORK_IDL_PATH, DEFAULT_FRAMEWORK_IDL_SPEC, DEFAULT_FRAMEWORK_VERSION,
    DEFAULT_LSSA_PIN, DEFAULT_WALLET_BINARY, FRAMEWORK_KIND_DEFAULT, FRAMEWORK_KIND_LSSA_LANG,
    LSSA_URL, VERSION,
};
use crate::model::{Config, FrameworkConfig, FrameworkIdlConfig, RepoRef};
use crate::project::{default_cache_root, infer_repo_path};
use crate::repo::sync_repo_to_pin_at_path;
use crate::state::write_text;
use crate::template::copy::{copy_dir_contents, patch_simple_tail_call_program_id};
use crate::template::project::{apply_overlay, OverlayRenderContext};
use crate::DynResult;

pub(crate) fn cmd_new(args: &[String]) -> DynResult<()> {
    if args.is_empty() {
        return Err(
            "usage: logos-scaffold new <name> [--template default|lssa-lang] [--vendor-deps] [--lssa-path PATH] [--cache-root PATH]"
                .into(),
        );
    }

    let name = args[0].clone();
    let mut vendor_deps = false;
    let mut lssa_path: Option<PathBuf> = None;
    let mut cache_root_override: Option<PathBuf> = None;
    let mut template_variant = FRAMEWORK_KIND_DEFAULT.to_string();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--template" => {
                let value = args.get(i + 1).ok_or("--template requires value")?;
                if value != FRAMEWORK_KIND_DEFAULT && value != FRAMEWORK_KIND_LSSA_LANG {
                    return Err(format!(
                        "unsupported template `{value}`. Expected `default` or `lssa-lang`."
                    )
                    .into());
                }
                template_variant = value.clone();
                i += 2;
            }
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
        framework: FrameworkConfig {
            kind: template_variant.clone(),
            version: DEFAULT_FRAMEWORK_VERSION.to_string(),
            idl: FrameworkIdlConfig {
                spec: DEFAULT_FRAMEWORK_IDL_SPEC.to_string(),
                path: DEFAULT_FRAMEWORK_IDL_PATH.to_string(),
            },
        },
    };

    let template_root = lssa_repo_path.join("examples/program_deployment");
    if !template_root.exists() {
        return Err(format!("template not found at {}", template_root.display()).into());
    }

    copy_dir_contents(&template_root, &target)?;
    if template_variant == FRAMEWORK_KIND_DEFAULT {
        patch_simple_tail_call_program_id(&target)?;
    }
    let overlay_ctx = OverlayRenderContext {
        crate_name: &crate_name,
        lssa_pin: &cfg.lssa.pin,
    };
    apply_overlay(&target, &template_variant, &overlay_ctx)?;
    if template_variant == FRAMEWORK_KIND_LSSA_LANG {
        cleanup_lssa_lang_hello_artifacts(&target)?;
    }
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
    println!("Template variant: {}", cfg.framework.kind);

    Ok(())
}

fn cleanup_lssa_lang_hello_artifacts(project_root: &Path) -> DynResult<()> {
    const RUNNER_FILES: &[&str] = &[
        "src/bin/run_hello_world.rs",
        "src/bin/run_hello_world_private.rs",
        "src/bin/run_hello_world_with_authorization.rs",
        "src/bin/run_hello_world_with_move_function.rs",
        "src/bin/run_hello_world_through_tail_call.rs",
        "src/bin/run_hello_world_through_tail_call_private.rs",
        "src/bin/run_hello_world_with_authorization_through_tail_call_with_pda.rs",
    ];
    const GUEST_METHOD_FILES: &[&str] = &[
        "methods/guest/src/bin/hello_world.rs",
        "methods/guest/src/bin/hello_world_with_authorization.rs",
        "methods/guest/src/bin/hello_world_with_move_function.rs",
        "methods/guest/src/bin/simple_tail_call.rs",
        "methods/guest/src/bin/tail_call_with_pda.rs",
    ];

    for rel_path in RUNNER_FILES.iter().chain(GUEST_METHOD_FILES) {
        let path = project_root.join(rel_path);
        if path.exists() {
            fs::remove_file(path)?;
        }
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
