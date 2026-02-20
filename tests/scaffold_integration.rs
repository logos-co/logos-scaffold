use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn integration_enabled() -> bool {
    matches!(
        env::var("LOGOS_SCAFFOLD_RUN_INTEGRATION")
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "1" | "true" | "yes"
    )
}

fn mk_temp_dir(suffix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    let path = env::temp_dir().join(format!(
        "logos-scaffold-integration-{suffix}-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&path).expect("failed to create temp directory");
    path
}

fn run_scaffold(args: &[&str]) {
    let mut argv = Vec::with_capacity(args.len() + 1);
    argv.push("logos-scaffold".to_string());
    argv.extend(args.iter().map(|v| (*v).to_string()));
    logos_scaffold::run(argv).expect("logos-scaffold command should succeed");
}

struct ScopedCwd {
    original: PathBuf,
}

impl ScopedCwd {
    fn new(target: &Path) -> Self {
        let original = env::current_dir().expect("failed to read cwd");
        env::set_current_dir(target).expect("failed to change cwd");
        Self { original }
    }
}

impl Drop for ScopedCwd {
    fn drop(&mut self) {
        let _ = env::set_current_dir(&self.original);
    }
}

#[test]
fn slice_run_smoke_local() {
    if !integration_enabled() {
        eprintln!("skipping integration test: set LOGOS_SCAFFOLD_RUN_INTEGRATION=1");
        return;
    }

    let workspace = mk_temp_dir("smoke");
    let _cwd = ScopedCwd::new(&workspace);

    run_scaffold(&["new", "smoke-app"]);
    let project_root = workspace.join("smoke-app");
    let _project_cwd = ScopedCwd::new(&project_root);

    run_scaffold(&["setup"]);
    run_scaffold(&["localnet", "start"]);
    run_scaffold(&["wallet", "init"]);
    run_scaffold(&["slice", "run"]);

    assert!(project_root
        .join(".scaffold/state/slice-last.json")
        .exists());

    let _ = logos_scaffold::run(vec![
        "logos-scaffold".to_string(),
        "localnet".to_string(),
        "stop".to_string(),
    ]);
}

#[test]
fn slice_run_repeat_three() {
    if !integration_enabled() {
        eprintln!("skipping integration test: set LOGOS_SCAFFOLD_RUN_INTEGRATION=1");
        return;
    }

    let workspace = mk_temp_dir("repeat");
    let _cwd = ScopedCwd::new(&workspace);

    run_scaffold(&["new", "repeat-app"]);
    let project_root = workspace.join("repeat-app");
    let _project_cwd = ScopedCwd::new(&project_root);

    run_scaffold(&["setup"]);
    run_scaffold(&["localnet", "start"]);
    run_scaffold(&["wallet", "init"]);
    run_scaffold(&["slice", "run", "--repeat", "3"]);

    let state = fs::read_to_string(project_root.join(".scaffold/state/slice-last.json"))
        .expect("expected slice state artifact");
    assert!(state.contains("\"repeat\": 3"));

    let _ = logos_scaffold::run(vec![
        "logos-scaffold".to_string(),
        "localnet".to_string(),
        "stop".to_string(),
    ]);
}

#[test]
fn wallet_config_migration_compat() {
    if !integration_enabled() {
        eprintln!("skipping integration test: set LOGOS_SCAFFOLD_RUN_INTEGRATION=1");
        return;
    }

    let workspace = mk_temp_dir("wallet-migration");
    let _cwd = ScopedCwd::new(&workspace);

    run_scaffold(&["new", "wallet-app"]);
    let project_root = workspace.join("wallet-app");
    let _project_cwd = ScopedCwd::new(&project_root);

    run_scaffold(&["setup"]);
    run_scaffold(&["localnet", "start"]);

    let wallet_home = project_root.join(".scaffold/wallet");
    fs::create_dir_all(&wallet_home).expect("failed to create wallet home");

    let legacy_cfg = wallet_home.join("config.json");
    let primary_cfg = wallet_home.join("wallet_config.json");
    if primary_cfg.exists() {
        fs::remove_file(&primary_cfg).expect("failed to remove wallet_config.json");
    }

    fs::write(
        &legacy_cfg,
        "{\"sequencer_addr\":\"http://127.0.0.1:3040\"}\n",
    )
    .expect("failed to write legacy config");

    run_scaffold(&["wallet", "init"]);

    assert!(primary_cfg.exists());

    let _ = logos_scaffold::run(vec![
        "logos-scaffold".to_string(),
        "localnet".to_string(),
        "stop".to_string(),
    ]);
}

#[test]
fn new_bootstrap_happy_path() {
    if !integration_enabled() {
        eprintln!("skipping integration test: set LOGOS_SCAFFOLD_RUN_INTEGRATION=1");
        return;
    }

    let workspace = mk_temp_dir("bootstrap");
    let _cwd = ScopedCwd::new(&workspace);

    run_scaffold(&["new", "boot-app", "--bootstrap"]);

    let project_root = workspace.join("boot-app");
    assert!(project_root
        .join(".scaffold/state/slice-last.json")
        .exists());

    let _project_cwd = ScopedCwd::new(&project_root);
    let _ = logos_scaffold::run(vec![
        "logos-scaffold".to_string(),
        "localnet".to_string(),
        "stop".to_string(),
    ]);
}
