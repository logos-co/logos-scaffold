use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::bail;

use crate::model::RepoRef;
use crate::process::{run_capture, run_checked};
use crate::DynResult;

pub(crate) fn sync_repo_to_pin(repo: &mut RepoRef, label: &str) -> DynResult<()> {
    let path = std::path::PathBuf::from(&repo.path);
    sync_repo_to_pin_at_path(&path, &repo.source, &repo.pin, label)?;
    repo.pin = git_head_sha(&path)?;
    Ok(())
}

pub(crate) fn sync_repo_to_pin_at_path(
    path: &Path,
    source: &str,
    pin: &str,
    label: &str,
) -> DynResult<()> {
    ensure_repo_present(path, source, label)?;

    let _ = run_checked(
        Command::new("git")
            .current_dir(path)
            .arg("fetch")
            .arg("--all")
            .arg("--tags"),
        &format!("git fetch ({label})"),
    );

    ensure_pin_exists(path, pin, label)?;

    run_checked(
        Command::new("git")
            .current_dir(path)
            .arg("checkout")
            .arg(pin),
        &format!("git checkout pin ({label})"),
    )?;

    let head = git_head_sha(path)?;
    if head != pin {
        bail!(
            "{label} pin mismatch after checkout (expected {}, got {})",
            pin,
            head
        );
    }

    Ok(())
}

pub(crate) fn ensure_pin_exists(path: &Path, pin: &str, label: &str) -> DynResult<()> {
    let rev = format!("{pin}^{{commit}}");
    if run_capture(
        Command::new("git")
            .current_dir(path)
            .arg("rev-parse")
            .arg("--verify")
            .arg(&rev),
        &format!("verify pin ({label})"),
    )
    .is_err()
    {
        bail!(
            "configured {label} pin {pin} is not available in {}. Ensure the repo source contains this commit (try `--lssa-path` pointing to a repo that has it).",
            path.display()
        );
    }

    Ok(())
}

pub(crate) fn ensure_repo_present(path: &Path, source: &str, label: &str) -> DynResult<()> {
    if path.exists() {
        if path.join(".git").exists() {
            return Ok(());
        }
        bail!("{} exists but is not a git repo: {}", label, path.display());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    run_checked(
        Command::new("git")
            .arg("clone")
            .arg("--no-hardlinks")
            .arg(source)
            .arg(path),
        &format!("clone {label}"),
    )
}

pub(crate) fn git_head_sha(repo: &Path) -> DynResult<String> {
    let out = run_capture(
        Command::new("git")
            .current_dir(repo)
            .arg("rev-parse")
            .arg("HEAD"),
        "git rev-parse HEAD",
    )?;
    Ok(out.stdout.trim().to_string())
}

pub(crate) fn git_clean(repo: &Path) -> DynResult<bool> {
    let out = run_capture(
        Command::new("git")
            .current_dir(repo)
            .arg("status")
            .arg("--porcelain"),
        "git status --porcelain",
    )?;
    Ok(out.stdout.trim().is_empty())
}
