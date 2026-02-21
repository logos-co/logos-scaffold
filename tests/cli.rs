use std::fs;
use std::path::Path;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

const TEST_PIN: &str = "dee3f7fa6f2bf63abef00828f246ddacade9cdaf";

#[test]
fn create_help_does_not_mutate_filesystem() {
    let temp = tempdir().expect("tempdir");

    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .arg("create")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage:"));

    assert!(
        !temp.path().join("--help").exists(),
        "--help must not be treated as project name"
    );
}

#[test]
fn localnet_status_json_is_parseable() {
    let temp = tempdir().expect("tempdir");
    let lssa_path = temp.path().join("lssa");
    fs::create_dir_all(&lssa_path).expect("create lssa path");
    write_scaffold_toml(temp.path(), &lssa_path, "wallet-not-installed-for-tests");

    let assert = Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .arg("localnet")
        .arg("status")
        .arg("--json")
        .assert()
        .success();

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8 stdout");
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("valid json");

    assert!(value.get("tracked_pid").is_some());
    assert!(value.get("listener_present").is_some());
    assert!(value.get("ownership").is_some());
    assert!(value.get("ready").is_some());
}

#[test]
fn doctor_json_outputs_machine_readable_report() {
    let temp = tempdir().expect("tempdir");
    let lssa_path = temp.path().join("lssa");
    fs::create_dir_all(&lssa_path).expect("create lssa path");
    write_scaffold_toml(temp.path(), &lssa_path, "wallet-not-installed-for-tests");

    let assert = Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .arg("doctor")
        .arg("--json")
        .assert();

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8 stdout");
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("valid json");

    assert!(value.get("status").is_some());
    assert!(value.get("summary").is_some());
    assert!(value.get("checks").is_some());
    assert!(value.get("next_steps").is_some());
}

#[test]
fn localnet_start_fails_when_process_exits_before_ready() {
    let temp = tempdir().expect("tempdir");
    let lssa_path = temp.path().join("lssa");
    let sequencer_bin = lssa_path.join("target/release/sequencer_runner");
    fs::create_dir_all(sequencer_bin.parent().expect("parent")).expect("create dirs");
    fs::write(&sequencer_bin, "#!/bin/sh\nexit 1\n").expect("write fake sequencer");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&sequencer_bin)
            .expect("metadata")
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&sequencer_bin, perms).expect("chmod");
    }

    write_scaffold_toml(temp.path(), &lssa_path, "wallet-not-installed-for-tests");

    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .arg("localnet")
        .arg("start")
        .arg("--timeout-sec")
        .arg("1")
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("sequencer process exited before becoming ready").or(
                predicate::str::contains("cannot start localnet: port 3040 is already in use"),
            ),
        );

    assert!(
        !temp.path().join(".scaffold/state/localnet.state").exists(),
        "state file should be cleaned after failed startup"
    );
}

fn write_scaffold_toml(project_root: &Path, lssa_path: &Path, wallet_binary: &str) {
    let content = format!(
        "[scaffold]\nversion = \"0.1.0\"\ncache_root = \"{}\"\n\n[repos.lssa]\nurl = \"https://github.com/logos-blockchain/lssa.git\"\nsource = \"https://github.com/logos-blockchain/lssa.git\"\npath = \"{}\"\npin = \"{}\"\n\n[wallet]\nbinary = \"{}\"\nhome_dir = \".scaffold/wallet\"\n",
        project_root.join("cache").display(),
        lssa_path.display(),
        TEST_PIN,
        wallet_binary
    );

    fs::write(project_root.join("scaffold.toml"), content).expect("write scaffold.toml");
}
