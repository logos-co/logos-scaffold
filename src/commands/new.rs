use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context};

use crate::config::serialize_config;
use crate::constants::{
    BASECAMP_RUNTIME_DEV, BASECAMP_RUNTIME_PORTABLE, DEFAULT_BASECAMP_DATA_ROOT, DEFAULT_FRAMEWORK_IDL_PATH,
    DEFAULT_FRAMEWORK_IDL_SPEC, DEFAULT_FRAMEWORK_VERSION, DEFAULT_LEZ_PIN,
    DEFAULT_LOGOS_MODULE_BUILDER_PIN, FRAMEWORK_KIND_DEFAULT, FRAMEWORK_KIND_LEZ_FRAMEWORK,
    LEZ_URL, LOGOS_MODULE_BUILDER_URL, PROJECT_KIND_BASECAMP_QML, PROJECT_KIND_LEZ, VERSION,
};
use crate::model::{
    BasecampConfig, Config, FrameworkConfig, FrameworkIdlConfig, LocalnetConfig, ProjectConfig,
    RepoRef,
};
use crate::project::default_cache_root;
use crate::repo::{sync_repo_to_pin_at_path_with_opts, RepoSyncOptions};
use crate::state::write_text;
use crate::template::copy::{copy_dir_contents, patch_simple_tail_call_program_id};
use crate::template::project::{apply_overlay, OverlayRenderContext};
use crate::DynResult;

#[derive(Debug)]
pub(crate) struct NewCommand {
    pub(crate) name: String,
    pub(crate) template: String,
    pub(crate) vendor_deps: bool,
    pub(crate) lez_path: Option<PathBuf>,
    pub(crate) module_builder_path: Option<PathBuf>,
    pub(crate) cache_root: Option<PathBuf>,
    pub(crate) basecamp_data_root: Option<PathBuf>,
    pub(crate) basecamp_runtime: String,
}

pub(crate) fn cmd_new(cmd: NewCommand) -> DynResult<()> {
    match cmd.template.as_str() {
        FRAMEWORK_KIND_DEFAULT | FRAMEWORK_KIND_LEZ_FRAMEWORK => create_lez_project(cmd),
        PROJECT_KIND_BASECAMP_QML => create_basecamp_qml_project(cmd),
        other => bail!(
            "unsupported template `{other}`. Expected `default`, `lez-framework`, or `basecamp-qml`."
        ),
    }
}

fn create_lez_project(cmd: NewCommand) -> DynResult<()> {
    let template_variant = cmd.template;
    let cwd = env::current_dir()?;
    let target = cwd.join(&cmd.name);
    let crate_name = to_cargo_crate_name(target_file_name_or(&target, "app"));

    if target.exists() {
        bail!("target exists: {}", target.display());
    }

    create_common_scaffold_dirs(&target)?;

    let cache_root = ensure_cache_root(cmd.cache_root)?;
    let lez_source = cmd
        .lez_path
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| LEZ_URL.to_string());

    let lez_repo_path = prepare_repo_path(
        &target,
        &cache_root,
        cmd.vendor_deps,
        "lez",
        DEFAULT_LEZ_PIN,
        &lez_source,
        RepoSyncOptions::auto_reclone_cache_repo(),
    )?;

    let cfg = Config {
        version: VERSION.to_string(),
        cache_root: cache_root.display().to_string(),
        project: ProjectConfig {
            kind: PROJECT_KIND_LEZ.to_string(),
        },
        lez: Some(RepoRef {
            url: LEZ_URL.to_string(),
            source: lez_source,
            path: lez_repo_path.display().to_string(),
            pin: DEFAULT_LEZ_PIN.to_string(),
        }),
        logos_module_builder: None,
        wallet_home_dir: ".scaffold/wallet".to_string(),
        framework: FrameworkConfig {
            kind: template_variant.clone(),
            version: DEFAULT_FRAMEWORK_VERSION.to_string(),
            idl: FrameworkIdlConfig {
                spec: DEFAULT_FRAMEWORK_IDL_SPEC.to_string(),
                path: DEFAULT_FRAMEWORK_IDL_PATH.to_string(),
            },
        },
        localnet: LocalnetConfig::default(),
        basecamp: BasecampConfig {
            data_root: DEFAULT_BASECAMP_DATA_ROOT.to_string(),
            runtime_variant: BASECAMP_RUNTIME_DEV.to_string(),
        },
    };

    let template_root = lez_repo_path.join("examples/program_deployment");
    if !template_root.exists() {
        bail!("template not found at {}", template_root.display());
    }

    copy_dir_contents(&template_root, &target).context("failed to copy scaffold template")?;
    if template_variant == FRAMEWORK_KIND_DEFAULT {
        patch_simple_tail_call_program_id(&target)?;
    }

    let overlay_ctx = build_overlay_context(
        &cmd.name,
        &cfg,
        &crate_name,
        "",
    );
    apply_overlay(&target, &template_variant, &overlay_ctx)?;
    if template_variant == FRAMEWORK_KIND_LEZ_FRAMEWORK {
        cleanup_lez_hello_artifacts(&target)?;
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
    if let Some(lez) = &cfg.lez {
        println!("Pinned lez: {}", lez.pin);
    }
    println!("Template variant: {}", cfg.framework.kind);

    Ok(())
}

fn create_basecamp_qml_project(cmd: NewCommand) -> DynResult<()> {
    validate_basecamp_runtime(&cmd.basecamp_runtime)?;
    let cwd = env::current_dir()?;
    let target = cwd.join(&cmd.name);
    let crate_name = to_cargo_crate_name(target_file_name_or(&target, "app"));
    let plugin_name = to_plugin_name(&cmd.name);

    if target.exists() {
        bail!("target exists: {}", target.display());
    }

    create_common_scaffold_dirs(&target)?;
    fs::create_dir_all(target.join(".scaffold/build"))?;
    fs::create_dir_all(target.join(".scaffold/runtime"))?;

    let cache_root = ensure_cache_root(cmd.cache_root)?;
    let module_builder_source = cmd
        .module_builder_path
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| LOGOS_MODULE_BUILDER_URL.to_string());
    let module_builder_repo_path = prepare_repo_path(
        &target,
        &cache_root,
        cmd.vendor_deps,
        "logos-module-builder",
        DEFAULT_LOGOS_MODULE_BUILDER_PIN,
        &module_builder_source,
        RepoSyncOptions::auto_reclone_cache_repo(),
    )?;

    let basecamp_data_root = cmd
        .basecamp_data_root
        .unwrap_or_else(|| target.join(DEFAULT_BASECAMP_DATA_ROOT));

    let cfg = Config {
        version: VERSION.to_string(),
        cache_root: cache_root.display().to_string(),
        project: ProjectConfig {
            kind: PROJECT_KIND_BASECAMP_QML.to_string(),
        },
        lez: None,
        logos_module_builder: Some(RepoRef {
            url: LOGOS_MODULE_BUILDER_URL.to_string(),
            source: module_builder_source,
            path: module_builder_repo_path.display().to_string(),
            pin: DEFAULT_LOGOS_MODULE_BUILDER_PIN.to_string(),
        }),
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
        basecamp: BasecampConfig {
            data_root: basecamp_data_root.display().to_string(),
            runtime_variant: cmd.basecamp_runtime.clone(),
        },
    };

    let module_builder_flake_url = format!("path:{}", module_builder_repo_path.display());
    let overlay_ctx = build_overlay_context(
        &cmd.name,
        &cfg,
        &crate_name,
        &module_builder_flake_url,
    );
    apply_overlay(&target, PROJECT_KIND_BASECAMP_QML, &overlay_ctx)?;
    write_text(&target.join("scaffold.toml"), &serialize_config(&cfg))?;

    println!("Created Basecamp QML project at {}", target.display());
    println!("Cache root: {}", cfg.cache_root);
    if let Some(module_builder) = &cfg.logos_module_builder {
        println!("Pinned logos-module-builder: {}", module_builder.pin);
        println!("Module builder path: {}", module_builder.path);
    }
    println!("Plugin name: {plugin_name}");
    println!("Basecamp data root: {}", cfg.basecamp.data_root);
    println!("Basecamp runtime: {}", cfg.basecamp.runtime_variant);

    Ok(())
}

fn validate_basecamp_runtime(value: &str) -> DynResult<()> {
    match value {
        BASECAMP_RUNTIME_DEV | BASECAMP_RUNTIME_PORTABLE => Ok(()),
        other => bail!("unsupported Basecamp runtime `{other}`. Expected `dev` or `portable`."),
    }
}

fn build_overlay_context(
    raw_name: &str,
    cfg: &Config,
    crate_name: &str,
    module_builder_flake_url: &str,
) -> OverlayRenderContext {
    let lez_pin = cfg
        .lez
        .as_ref()
        .map(|repo| repo.pin.clone())
        .unwrap_or_default();

    OverlayRenderContext {
        crate_name: crate_name.to_string(),
        lez_pin,
        plugin_name: to_plugin_name(raw_name),
        project_title: to_project_title(raw_name),
        module_builder_flake_url: module_builder_flake_url.to_string(),
        basecamp_data_root: cfg.basecamp.data_root.clone(),
        basecamp_runtime_variant: cfg.basecamp.runtime_variant.clone(),
    }
}

fn prepare_repo_path(
    project_root: &Path,
    cache_root: &Path,
    vendor_deps: bool,
    repo_name: &str,
    pin: &str,
    source: &str,
    cache_opts: RepoSyncOptions,
) -> DynResult<PathBuf> {
    if vendor_deps {
        let root = project_root.join(".scaffold/repos");
        fs::create_dir_all(&root)?;
        let vendored = root.join(repo_name);
        sync_repo_to_pin_at_path_with_opts(
            &vendored,
            source,
            pin,
            repo_name,
            RepoSyncOptions::fail_on_source_mismatch(),
        )?;
        return Ok(vendored);
    }

    let cached = cache_root.join("repos").join(repo_name).join(pin);
    sync_repo_to_pin_at_path_with_opts(&cached, source, pin, repo_name, cache_opts)?;
    Ok(cached)
}

fn ensure_cache_root(cache_root: Option<PathBuf>) -> DynResult<PathBuf> {
    let cache_root = cache_root.unwrap_or(default_cache_root()?);
    fs::create_dir_all(cache_root.join("repos"))?;
    fs::create_dir_all(cache_root.join("state"))?;
    fs::create_dir_all(cache_root.join("logs"))?;
    fs::create_dir_all(cache_root.join("builds"))?;
    Ok(cache_root)
}

fn create_common_scaffold_dirs(target: &Path) -> DynResult<()> {
    fs::create_dir_all(target.join(".scaffold/state"))?;
    fs::create_dir_all(target.join(".scaffold/logs"))?;
    Ok(())
}

fn target_file_name_or<'a>(target: &'a Path, fallback: &'a str) -> &'a str {
    target
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(fallback)
}

fn cleanup_lez_hello_artifacts(project_root: &Path) -> DynResult<()> {
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

pub(crate) fn to_plugin_name(input: &str) -> String {
    let mut out = String::new();
    let mut prev_sep = false;
    for ch in input.chars() {
        let mapped = if ch.is_ascii_alphanumeric() {
            ch.to_ascii_lowercase()
        } else {
            '_'
        };

        if mapped == '_' {
            if !prev_sep {
                out.push('_');
                prev_sep = true;
            }
        } else {
            out.push(mapped);
            prev_sep = false;
        }
    }

    let out = out.trim_matches('_').to_string();
    if out.is_empty() {
        "basecamp_app".to_string()
    } else {
        out
    }
}

fn to_project_title(input: &str) -> String {
    let mut words = Vec::new();
    for chunk in input.split(|ch: char| !ch.is_ascii_alphanumeric()) {
        if chunk.is_empty() {
            continue;
        }
        let mut chars = chunk.chars();
        if let Some(first) = chars.next() {
            let mut word = first.to_ascii_uppercase().to_string();
            word.push_str(&chars.as_str().to_ascii_lowercase());
            words.push(word);
        }
    }

    if words.is_empty() {
        "Basecamp App".to_string()
    } else {
        words.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::{to_cargo_crate_name, to_plugin_name};

    #[test]
    fn simple_name_is_lowercased() {
        assert_eq!(to_cargo_crate_name("MyApp"), "myapp");
    }

    #[test]
    fn spaces_become_dashes() {
        assert_eq!(to_cargo_crate_name("my app"), "my-app");
    }

    #[test]
    fn special_chars_become_single_dash() {
        assert_eq!(to_cargo_crate_name("my--app"), "my-app");
        assert_eq!(to_cargo_crate_name("my___app"), "my-app");
    }

    #[test]
    fn leading_and_trailing_dashes_are_trimmed() {
        assert_eq!(to_cargo_crate_name("--myapp--"), "myapp");
        assert_eq!(to_cargo_crate_name("_myapp_"), "myapp");
    }

    #[test]
    fn empty_string_returns_default() {
        assert_eq!(to_cargo_crate_name(""), "program_deployment");
    }

    #[test]
    fn only_special_chars_returns_default() {
        assert_eq!(to_cargo_crate_name("---"), "program_deployment");
        assert_eq!(to_cargo_crate_name("!!!"), "program_deployment");
    }

    #[test]
    fn alphanumeric_preserved() {
        assert_eq!(to_cargo_crate_name("my-app-123"), "my-app-123");
    }

    #[test]
    fn unicode_becomes_dash() {
        assert_eq!(to_cargo_crate_name("héllo"), "h-llo");
    }

    #[test]
    fn plugin_name_uses_underscores() {
        assert_eq!(to_plugin_name("Hello World"), "hello_world");
    }
}
