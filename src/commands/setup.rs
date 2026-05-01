use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::process::run_checked;
use crate::project::{ensure_dir_exists, load_project, resolve_cache_root, save_project_config};
use crate::repo::{sync_repo_to_pin, RepoSyncOptions};
use crate::state::prepare_wallet_home;
use crate::DynResult;

use super::wallet_support::{
    first_public_wallet_address, read_default_wallet_address, wallet_state_path,
    write_default_wallet_address,
};

pub(crate) fn cmd_setup(prebuilt: bool) -> DynResult<()> {
    let mut project = load_project()?;
    let lez = PathBuf::from(&project.config.lez.path);
    let (cache_root, _) = resolve_cache_root(&project)?;
    let sync_opts = if is_cache_managed_repo_path(&cache_root, &lez) {
        RepoSyncOptions::auto_reclone_cache_repo()
    } else {
        RepoSyncOptions::fail_on_source_mismatch()
    };

    sync_repo_to_pin(&mut project.config.lez, "lez", sync_opts)?;

    ensure_dir_exists(&lez, "lez")?;

    let built_from_prebuilt = if prebuilt {
        try_download_prebuilt(&lez, &project.config.lez.pin)?
    } else {
        false
    };

    if !built_from_prebuilt {
        run_checked(
            Command::new("cargo")
                .current_dir(&lez)
                .arg("build")
                .arg("--release")
                .arg("--features")
                .arg("standalone")
                .arg("-p")
                .arg("sequencer_service"),
            "build sequencer_service (standalone)",
        )?;
    }

    run_checked(
        Command::new("cargo")
            .current_dir(&lez)
            .arg("build")
            .arg("--release")
            .arg("-p")
            .arg("wallet"),
        "build wallet",
    )?;

    let wallet_home = project.root.join(&project.config.wallet_home_dir);
    prepare_wallet_home(&lez, &wallet_home)?;
    ensure_default_wallet_seeded(&project.root, &wallet_home)?;

    save_project_config(&project)?;
    println!("setup complete");

    Ok(())
}

fn ensure_default_wallet_seeded(project_root: &Path, wallet_home: &Path) -> DynResult<()> {
    let should_seed = match read_default_wallet_address(project_root) {
        Ok(Some(existing)) => {
            println!("default wallet already configured: {existing}");
            false
        }
        Ok(None) => true,
        Err(err) => {
            println!(
                "warning: wallet default state is malformed; attempting deterministic reseed: {err}"
            );
            true
        }
    };

    if !should_seed {
        return Ok(());
    }

    match first_public_wallet_address(wallet_home) {
        Ok(Some(address)) => {
            let normalized = write_default_wallet_address(project_root, &address)?;
            let state_path = wallet_state_path(project_root);
            println!("default wallet seeded from preconfigured account");
            println!("  Address: {normalized}");
            println!("  State file: {}", state_path.display());
        }
        Ok(None) => {
            println!(
                "warning: could not seed default wallet automatically (no preconfigured public account found)"
            );
        }
        Err(err) => {
            println!("warning: could not seed default wallet automatically: {err}");
        }
    }

    Ok(())
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


fn try_download_prebuilt(lez: &Path, pin: &str) -> DynResult<bool> {
    let commit = &pin[..8.min(pin.len())];
    let arch = if cfg!(target_arch = "x86_64") { "x86_64" } else { "aarch64" };
    let os = if cfg!(target_os = "linux") { "linux" } else { "macos" };
    let tag = format!("lez-prebuilt-{commit}-{arch}-{os}");

    println!("Checking for prebuilt binaries (tag: {tag})...");

    let url = format!(
        "https://github.com/logos-co/logos-scaffold/releases/download/{tag}/sequencer_service"
    );

    let bin_dir = lez.join("target/release");
    std::fs::create_dir_all(&bin_dir)?;
    let bin_path = bin_dir.join("sequencer_service");

    let status = Command::new("curl")
        .args([
            "--fail", "--silent", "--location",
            "--output", bin_path.to_str().unwrap_or("sequencer_service"),
            &url,
        ])
        .status();

    match status {
        Ok(s) if s.success() && bin_path.exists() => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = std::fs::metadata(&bin_path)?.permissions();
                perms.set_mode(0o755);
                std::fs::set_permissions(&bin_path, perms)?;
            }
            println!("prebuilt sequencer_service downloaded successfully");
            Ok(true)
        }
        _ => {
            println!("no prebuilt found for tag {tag}, falling back to source build...");
            Ok(false)
        }
    }
}


#[cfg(test)]
mod tests {
    use crate::commands::wallet_support::WALLET_CONFIG_PRIMARY;
    use std::fs;

    use tempfile::tempdir;

    use super::ensure_default_wallet_seeded;
    use crate::commands::wallet_support::wallet_state_path;

    const PUBLIC_ACCOUNT_ID: &str = "6iArKUXxhUJqS7kCaPNhwMWt3ro71PDyBj7jwAyE2VQV";
    const PRIVATE_ACCOUNT_ID: &str = "2ECgkFTaXzwjJBXR7ZKmXYQtpHbvTTHK9Auma4NL9AUo";

    #[test]
    fn ensure_default_wallet_seeded_writes_first_public_account() {
        let temp = tempdir().expect("tempdir");
        let wallet_home = temp.path().join(".scaffold/wallet");
        fs::create_dir_all(&wallet_home).expect("mkdir wallet home");
        fs::write(
            wallet_home.join(WALLET_CONFIG_PRIMARY),
            format!(
                r#"{{
  "initial_accounts": [
    {{ "Private": {{ "account_id": "{PRIVATE_ACCOUNT_ID}" }} }},
    {{ "Public": {{ "account_id": "{PUBLIC_ACCOUNT_ID}" }} }}
  ]
}}"#
            ),
        )
        .expect("write wallet config");

        ensure_default_wallet_seeded(temp.path(), &wallet_home).expect("seed default wallet");

        let state = fs::read_to_string(wallet_state_path(temp.path())).expect("read wallet.state");
        assert_eq!(
            state,
            format!("default_address=Public/{PUBLIC_ACCOUNT_ID}\n")
        );
    }

    #[test]
    fn ensure_default_wallet_seeded_does_not_overwrite_existing_default() {
        let temp = tempdir().expect("tempdir");
        let state_path = wallet_state_path(temp.path());
        fs::create_dir_all(state_path.parent().expect("parent")).expect("mkdir state parent");
        fs::write(
            &state_path,
            "default_address=Public/8zxWNm1qh6FLsJpVBuDxdxcTm55qHPgFEdqJpPVu1fuy\n",
        )
        .expect("write wallet.state");

        let wallet_home = temp.path().join(".scaffold/wallet");
        fs::create_dir_all(&wallet_home).expect("mkdir wallet home");
        fs::write(
            wallet_home.join(WALLET_CONFIG_PRIMARY),
            format!(
                r#"{{
  "initial_accounts": [
    {{ "Public": {{ "account_id": "{PUBLIC_ACCOUNT_ID}" }} }}
  ]
}}"#
            ),
        )
        .expect("write wallet config");

        ensure_default_wallet_seeded(temp.path(), &wallet_home).expect("seed default wallet");

        let state = fs::read_to_string(state_path).expect("read wallet.state");
        assert_eq!(
            state,
            "default_address=Public/8zxWNm1qh6FLsJpVBuDxdxcTm55qHPgFEdqJpPVu1fuy\n"
        );
    }
}
