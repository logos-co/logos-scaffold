use std::fs;
use std::path::PathBuf;
use std::process::Command;

use crate::constants::FRAMEWORK_KIND_LSSA_LANG;
use crate::process::run_checked;
use crate::project::{load_project, run_in_project_dir};
use crate::DynResult;

pub(crate) fn cmd_client(args: &[String]) -> DynResult<()> {
    if args.is_empty() {
        return Err("usage: logos-scaffold client build [project-path]".into());
    }

    match args[0].as_str() {
        "build" => {
            let project_dir =
                parse_optional_project_path(&args[1..], "logos-scaffold client build")?;
            run_in_project_dir(project_dir.as_deref(), build_clients_for_current_project)
        }
        other => Err(format!("unknown client command: {other}").into()),
    }
}

pub(crate) fn build_clients_for_current_project() -> DynResult<()> {
    let project = load_project()?;
    if project.config.framework.kind != FRAMEWORK_KIND_LSSA_LANG {
        println!(
            "Skipping client build for framework kind `{}`",
            project.config.framework.kind
        );
        return Ok(());
    }

    let idl_dir = project.root.join(&project.config.framework.idl.path);
    if !idl_dir.exists() {
        return Err(format!(
            "IDL directory does not exist at {}. Run `logos-scaffold idl build` first.",
            idl_dir.display()
        )
        .into());
    }

    let json_count = fs::read_dir(&idl_dir)?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
        .count();
    if json_count == 0 {
        return Err(format!(
            "No IDL json files found under {}. Run `logos-scaffold idl build` first.",
            idl_dir.display()
        )
        .into());
    }

    let out_dir = project.root.join("src/generated");
    fs::create_dir_all(&out_dir)?;

    let generator_manifest = project.root.join("crates/lssa-client-gen/Cargo.toml");
    if !generator_manifest.exists() {
        return Err(format!(
            "missing client generator crate at {}",
            generator_manifest.display()
        )
        .into());
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
            return Err(format!("unknown flag for `{usage_label}`: {arg}").into());
        }
        if project_dir.is_none() {
            project_dir = Some(PathBuf::from(arg));
        } else {
            return Err(format!("unexpected argument `{arg}` for `{usage_label}`").into());
        }
    }

    Ok(project_dir)
}
