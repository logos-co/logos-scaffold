use std::fs;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{anyhow, bail};

use crate::commands::idl::build_idl_for_current_project;
use crate::constants::FRAMEWORK_KIND_LEZ_FRAMEWORK;
use crate::process::run_checked;
use crate::project::{load_project, run_in_project_dir};
use crate::DynResult;

pub(crate) fn cmd_client(args: &[String]) -> DynResult<()> {
    if args.is_empty() {
        bail!("usage: logos-scaffold build client [project-path]");
    }

    match args[0].as_str() {
        "build" => {
            let project_dir =
                parse_optional_project_path(&args[1..], "logos-scaffold build client")?;
            run_in_project_dir(project_dir.as_deref(), build_clients_for_current_project)
        }
        other => Err(anyhow!("unknown client command: {other}")),
    }
}

pub(crate) fn build_clients_for_current_project() -> DynResult<()> {
    let project = load_project()?;
    if project.config.framework.kind != FRAMEWORK_KIND_LEZ_FRAMEWORK {
        println!(
            "Skipping client build for framework kind `{}`",
            project.config.framework.kind
        );
        return Ok(());
    }

    // Always regenerate IDL first — prevents stale IDL drift (fixes issues like missing rest:true)
    println!("[client] Regenerating IDL to ensure it is fresh...");
    build_idl_for_current_project()?;

    let idl_dir = project.root.join(&project.config.framework.idl.path);
    let out_dir = project.root.join("src/generated");
    fs::create_dir_all(&out_dir)?;

    let generator_manifest = project.root.join("crates/lssa-client-gen/Cargo.toml");
    if !generator_manifest.exists() {
        bail!(
            "missing client generator crate at {}",
            generator_manifest.display()
        );
    }

    run_checked(
        Command::new("cargo")
            .current_dir(&project.root)
            .arg("run")
            .arg("--manifest-path")
            .arg(&generator_manifest)
            .arg("--")
            .arg("--idl-dir")
            .arg(&idl_dir)
            .arg("--out-dir")
            .arg(&out_dir),
        "run lssa client generator",
    )?;

    Ok(())
}

fn parse_optional_project_path(args: &[String], usage_label: &str) -> DynResult<Option<PathBuf>> {
    let mut project_dir: Option<PathBuf> = None;

    for arg in args {
        if arg.starts_with("--") {
            bail!("unknown flag for `{usage_label}`: {arg}");
        }
        if project_dir.is_none() {
            project_dir = Some(PathBuf::from(arg));
        } else {
            bail!("unexpected argument `{arg}` for `{usage_label}`");
        }
    }

    Ok(project_dir)
}
