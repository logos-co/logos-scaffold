use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::Duration;

use assert_cmd::Command;
use flate2::read::GzDecoder;
use predicates::prelude::*;
use tar::Archive;
use tempfile::tempdir;

const TEST_PIN: &str = "767b5afd388c7981bcdf6f5b5c80159607e07e5b";
const VALID_ACCOUNT_ID: &str = "6iArKUXxhUJqS7kCaPNhwMWt3ro71PDyBj7jwAyE2VQV";
const VALID_PUBLIC_ADDRESS: &str = "Public/6iArKUXxhUJqS7kCaPNhwMWt3ro71PDyBj7jwAyE2VQV";
const DEFAULT_WALLET_PASSWORD: &str = "logos-scaffold-v0";
const GUEST_BIN_REL_PATH: &str =
    "target/riscv-guest/example_program_deployment_methods/example_program_deployment_programs/riscv32im-risc0-zkvm-elf/release";

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
fn wallet_help_lists_list_topup_and_default_commands() {
    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .arg("wallet")
        .arg("--help")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("list")
                .and(predicate::str::contains("topup"))
                .and(predicate::str::contains("default")),
        );
}

#[test]
fn deploy_help_lists_optional_program_name() {
    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .arg("deploy")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("[PROGRAM_NAME]"));
    // Note: output includes [OPTIONS] when extra flags are present
}

#[test]
fn report_help_lists_out_and_tail_flags() {
    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .arg("report")
        .arg("--help")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("--out")
                .and(predicate::str::contains("--tail"))
                .and(predicate::str::contains(
                    "Collect a sanitized diagnostics archive",
                )),
        );
}

#[test]
fn report_generates_default_archive_with_warning_and_manifest() {
    let temp = tempdir().expect("tempdir");
    setup_wallet_project(temp.path(), Some("http://127.0.0.1:3040"));
    fs::create_dir_all(temp.path().join(".scaffold/logs")).expect("create logs dir");
    fs::write(
        temp.path().join(".scaffold/logs/sequencer.log"),
        "sequencer started\n",
    )
    .expect("write sequencer log");

    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .arg("report")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("report complete")
                .and(predicate::str::contains("archive:"))
                .and(predicate::str::contains(
                    "Inspect every file before sharing",
                )),
        );

    let archive_path = find_single_report_archive(&temp.path().join(".scaffold/reports"));
    assert!(archive_path.exists(), "expected default report archive");

    let entries = read_report_archive_entries(&archive_path);
    assert!(archive_entry_exists(&entries, "README.txt"));
    assert!(archive_entry_exists(&entries, "manifest.json"));
    assert!(archive_entry_exists(&entries, "diagnostics/doctor.json"));
    assert!(archive_entry_exists(
        &entries,
        "diagnostics/localnet-status.json"
    ));
    assert!(archive_entry_exists(
        &entries,
        "summaries/build-evidence.json"
    ));

    let readme = archive_entry_content(&entries, "README.txt");
    assert!(
        readme.contains("best-effort basis"),
        "README should include warning, got: {readme}"
    );

    let build_evidence = archive_entry_content(&entries, "summaries/build-evidence.json");
    assert!(
        build_evidence.contains("No build commands were executed"),
        "build evidence should confirm metadata-only mode, got: {build_evidence}"
    );
}

#[test]
fn report_supports_custom_output_path() {
    let temp = tempdir().expect("tempdir");
    setup_wallet_project(temp.path(), Some("http://127.0.0.1:3040"));

    let custom_out = temp.path().join("artifacts/support-report.tar.gz");
    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .arg("report")
        .arg("--out")
        .arg(&custom_out)
        .assert()
        .success();

    assert!(
        custom_out.exists(),
        "custom report output should exist at {}",
        custom_out.display()
    );
}

#[test]
fn report_excludes_wallet_files_from_archive() {
    let temp = tempdir().expect("tempdir");
    setup_wallet_project(temp.path(), Some("http://127.0.0.1:3040"));

    let wallet_dir = temp.path().join(".scaffold/wallet");
    fs::create_dir_all(&wallet_dir).expect("create wallet dir");
    fs::write(wallet_dir.join("config.json"), "{ \"test\": true }\n").expect("write config");
    fs::write(
        wallet_dir.join("storage.json"),
        "{ \"secret_spending_key\": [1,2,3] }\n",
    )
    .expect("write storage");
    fs::write(
        wallet_dir.join("wallet_config.json"),
        "{ \"initial_accounts\": [] }\n",
    )
    .expect("write wallet config");

    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .arg("report")
        .assert()
        .success();

    let archive_path = find_single_report_archive(&temp.path().join(".scaffold/reports"));
    let entries = read_report_archive_entries(&archive_path);
    for (path, _) in &entries {
        assert!(
            !path.contains(".scaffold/wallet/"),
            "wallet files must be excluded, found archive path: {path}"
        );
    }
}

#[test]
fn report_redacts_sensitive_values_in_logs() {
    let temp = tempdir().expect("tempdir");
    setup_wallet_project(temp.path(), Some("http://127.0.0.1:3040"));

    fs::create_dir_all(temp.path().join(".scaffold/logs")).expect("create logs dir");
    fs::write(
        temp.path().join(".scaffold/logs/sequencer.log"),
        "password=super-secret\napi_token=abc123\nrpc=http://user:pass@127.0.0.1:3040\n",
    )
    .expect("write sequencer log");

    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .arg("report")
        .assert()
        .success();

    let archive_path = find_single_report_archive(&temp.path().join(".scaffold/reports"));
    let entries = read_report_archive_entries(&archive_path);
    let log_body = archive_entry_content(&entries, "logs/sequencer.log");

    assert!(!log_body.contains("super-secret"));
    assert!(!log_body.contains("abc123"));
    assert!(!log_body.contains("user:pass@"));
    assert!(
        log_body.contains("[REDACTED]"),
        "expected redaction marker in sanitized log, got: {log_body}"
    );
}

#[test]
fn report_keeps_non_utf8_logs_via_lossy_decoding() {
    let temp = tempdir().expect("tempdir");
    setup_wallet_project(temp.path(), Some("http://127.0.0.1:3040"));

    fs::create_dir_all(temp.path().join(".scaffold/logs")).expect("create logs dir");
    fs::write(
        temp.path().join(".scaffold/logs/sequencer.log"),
        [b'o', b'k', b'\n', 0xff, 0xfe, b'\n'],
    )
    .expect("write non-utf8 log");

    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .arg("report")
        .assert()
        .success();

    let archive_path = find_single_report_archive(&temp.path().join(".scaffold/reports"));
    let entries = read_report_archive_entries(&archive_path);
    let log_body = archive_entry_content(&entries, "logs/sequencer.log");

    assert!(log_body.contains("ok"), "expected preserved utf8 content");
    assert!(
        log_body.contains('\u{fffd}'),
        "expected lossy replacement chars for invalid utf8, got: {log_body:?}"
    );
}

#[test]
fn report_manifest_scrubs_absolute_paths_in_warnings() {
    let temp = tempdir().expect("tempdir");
    let lez_path = temp.path().join("lez");
    fs::create_dir_all(&lez_path).expect("create lez path");
    // No wallet stub — wallet binary is missing at lez/target/release/wallet
    write_scaffold_toml(temp.path(), &lez_path);
    write_wallet_config(temp.path(), Some("http://127.0.0.1:3040"));

    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .arg("report")
        .assert()
        .success();

    let archive_path = find_single_report_archive(&temp.path().join(".scaffold/reports"));
    let entries = read_report_archive_entries(&archive_path);
    let manifest = archive_entry_content(&entries, "manifest.json");
    let temp_root = temp.path().to_string_lossy();

    assert!(
        !manifest.contains(temp_root.as_ref()),
        "manifest should not leak absolute project path, got: {manifest}"
    );
    assert!(
        manifest.contains("tool probe `wallet` did not succeed"),
        "manifest should contain wallet probe warning, got: {manifest}"
    );
}

#[test]
fn report_sanitizes_localnet_status_log_path() {
    let temp = tempdir().expect("tempdir");
    setup_wallet_project(temp.path(), Some("http://127.0.0.1:3040"));

    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .arg("report")
        .assert()
        .success();

    let archive_path = find_single_report_archive(&temp.path().join(".scaffold/reports"));
    let entries = read_report_archive_entries(&archive_path);
    let localnet_status = archive_entry_content(&entries, "diagnostics/localnet-status.json");
    let temp_root = temp.path().to_string_lossy();

    assert!(
        !localnet_status.contains(temp_root.as_ref()),
        "localnet status should not leak project abs path, got: {localnet_status}"
    );

    let value: serde_json::Value =
        serde_json::from_str(localnet_status).expect("valid localnet status json");
    let log_path = value
        .get("log_path")
        .and_then(serde_json::Value::as_str)
        .expect("log_path string");
    assert!(
        log_path.contains("<PROJECT_ROOT>"),
        "expected scrubbed project placeholder in localnet log path, got: {log_path}"
    );
}

#[test]
fn report_sanitizes_doctor_json_paths() {
    let temp = tempdir().expect("tempdir");
    setup_wallet_project(temp.path(), Some("http://127.0.0.1:3040"));

    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .arg("report")
        .assert()
        .success();

    let archive_path = find_single_report_archive(&temp.path().join(".scaffold/reports"));
    let entries = read_report_archive_entries(&archive_path);
    let doctor = archive_entry_content(&entries, "diagnostics/doctor.json");
    let temp_root = temp.path().to_string_lossy();

    assert!(
        !doctor.contains(temp_root.as_ref()),
        "doctor report should not leak absolute paths, got: {doctor}"
    );
    assert!(
        doctor.contains("<PROJECT_ROOT>"),
        "doctor report should include scrubbed placeholder for project path, got: {doctor}"
    );
}

#[test]
fn report_scrubs_tool_command_paths_in_summary() {
    let temp = tempdir().expect("tempdir");
    setup_wallet_project(temp.path(), Some("http://127.0.0.1:3040"));

    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .arg("report")
        .assert()
        .success();

    let archive_path = find_single_report_archive(&temp.path().join(".scaffold/reports"));
    let entries = read_report_archive_entries(&archive_path);
    let tool_versions = archive_entry_content(&entries, "summaries/tool-versions.json");
    let temp_root = temp.path().to_string_lossy();

    assert!(
        !tool_versions.contains(temp_root.as_ref()),
        "tool summary should not leak absolute paths, got: {tool_versions}"
    );

    let value: serde_json::Value =
        serde_json::from_str(tool_versions).expect("valid tool versions json");
    let wallet = value
        .as_array()
        .and_then(|rows| {
            rows.iter()
                .find(|row| row.get("name").and_then(serde_json::Value::as_str) == Some("wallet"))
        })
        .expect("wallet tool row");
    let wallet_command = wallet
        .get("command")
        .and_then(serde_json::Value::as_str)
        .expect("wallet command string");
    assert!(
        wallet_command.contains("<PROJECT_ROOT>/lez/target/release/wallet"),
        "expected scrubbed wallet command path, got: {wallet_command}"
    );
}

#[test]
fn report_redacts_multiline_private_key_blocks() {
    let temp = tempdir().expect("tempdir");
    setup_wallet_project(temp.path(), Some("http://127.0.0.1:3040"));

    fs::create_dir_all(temp.path().join(".scaffold/logs")).expect("create logs dir");
    fs::write(
        temp.path().join(".scaffold/logs/sequencer.log"),
        "before\n-----BEGIN PRIVATE KEY-----\nMIIEvQIBADANBgkqhkiG9w0BAQEFAASC\n-----END PRIVATE KEY-----\nafter\n",
    )
    .expect("write sequencer log");

    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .arg("report")
        .assert()
        .success();

    let archive_path = find_single_report_archive(&temp.path().join(".scaffold/reports"));
    let entries = read_report_archive_entries(&archive_path);
    let log_body = archive_entry_content(&entries, "logs/sequencer.log");

    assert!(!log_body.contains("MIIEvQIBADANBgkqhkiG9w0BAQEFAASC"));
    assert!(!log_body.contains("-----BEGIN PRIVATE KEY-----"));
    assert!(!log_body.contains("-----END PRIVATE KEY-----"));
    assert!(
        log_body.contains("[REDACTED SENSITIVE LINE]"),
        "expected redaction markers, got: {log_body}"
    );
}

#[test]
fn report_redacts_url_userinfo_without_colon() {
    let temp = tempdir().expect("tempdir");
    setup_wallet_project(temp.path(), Some("http://127.0.0.1:3040"));

    fs::create_dir_all(temp.path().join(".scaffold/logs")).expect("create logs dir");
    fs::write(
        temp.path().join(".scaffold/logs/sequencer.log"),
        "fetch https://ghp_very_secret_token@github.com/logos/repo\n",
    )
    .expect("write sequencer log");

    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .arg("report")
        .assert()
        .success();

    let archive_path = find_single_report_archive(&temp.path().join(".scaffold/reports"));
    let entries = read_report_archive_entries(&archive_path);
    let log_body = archive_entry_content(&entries, "logs/sequencer.log");

    assert!(!log_body.contains("ghp_very_secret_token"));
    assert!(
        log_body.contains("https://[REDACTED]@github.com/logos/repo"),
        "expected token-style userinfo redaction, got: {log_body}"
    );
}

#[test]
fn report_tail_keeps_only_last_requested_lines() {
    let temp = tempdir().expect("tempdir");
    setup_wallet_project(temp.path(), Some("http://127.0.0.1:3040"));

    fs::create_dir_all(temp.path().join(".scaffold/logs")).expect("create logs dir");
    fs::write(
        temp.path().join(".scaffold/logs/sequencer.log"),
        "line-1\nline-2\nline-3\nline-4\n",
    )
    .expect("write sequencer log");

    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .arg("report")
        .arg("--tail")
        .arg("2")
        .assert()
        .success();

    let archive_path = find_single_report_archive(&temp.path().join(".scaffold/reports"));
    let entries = read_report_archive_entries(&archive_path);
    let log_body = archive_entry_content(&entries, "logs/sequencer.log");

    assert!(!log_body.contains("line-1"));
    assert!(!log_body.contains("line-2"));
    assert!(log_body.contains("line-3"));
    assert!(log_body.contains("line-4"));
}

#[test]
fn report_default_archive_names_are_unique_for_fast_repeats() {
    let temp = tempdir().expect("tempdir");
    setup_wallet_project(temp.path(), Some("http://127.0.0.1:3040"));

    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .arg("report")
        .assert()
        .success();
    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .arg("report")
        .assert()
        .success();

    let archives = list_report_archives(&temp.path().join(".scaffold/reports"));
    assert_eq!(
        archives.len(),
        2,
        "expected two report archives from back-to-back runs, got: {:?}",
        archives
    );
    assert_ne!(archives[0], archives[1], "archive paths must be unique");
}

#[test]
fn report_fails_outside_project_with_project_scoped_message() {
    let temp = tempdir().expect("tempdir");

    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .arg("report")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "This command must be run inside a logos-scaffold project.",
        ));
}

#[test]
fn report_skips_unreadable_optional_file_and_keeps_succeeding() {
    let temp = tempdir().expect("tempdir");
    setup_wallet_project(temp.path(), Some("http://127.0.0.1:3040"));
    fs::create_dir(temp.path().join(".env.local")).expect("make .env.local unreadable as dir");

    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .arg("report")
        .assert()
        .success();

    let archive_path = find_single_report_archive(&temp.path().join(".scaffold/reports"));
    let entries = read_report_archive_entries(&archive_path);
    let manifest = archive_entry_content(&entries, "manifest.json");
    assert!(
        manifest.contains("project/env.local"),
        "manifest should record skipped env summary, got: {manifest}"
    );
    assert!(
        manifest.contains("failed to read .env.local"),
        "manifest should include skip reason, got: {manifest}"
    );
}

#[test]
fn localnet_status_json_is_parseable() {
    let temp = tempdir().expect("tempdir");
    let lez_path = temp.path().join("lez");
    fs::create_dir_all(&lez_path).expect("create lez path");
    write_scaffold_toml(temp.path(), &lez_path);

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
    let lez_path = temp.path().join("lez");
    fs::create_dir_all(&lez_path).expect("create lez path");
    write_scaffold_toml(temp.path(), &lez_path);

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
fn doctor_uses_password_env_override_for_wallet_health() {
    let temp = tempdir().expect("tempdir");
    setup_wallet_project(temp.path(), Some("http://127.0.0.1:3040"));

    let assert = Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .env("EXPECT_PASSWORD", "override-pass")
        .env("LOGOS_SCAFFOLD_WALLET_PASSWORD", "override-pass")
        .arg("doctor")
        .arg("--json")
        .assert();

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8 stdout");
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("valid json");

    let checks = value
        .get("checks")
        .and_then(serde_json::Value::as_array)
        .expect("checks array");
    let wallet_usability = checks
        .iter()
        .find(|check| {
            check.get("name").and_then(serde_json::Value::as_str) == Some("wallet usability")
        })
        .expect("wallet usability check present");

    assert_eq!(
        wallet_usability
            .get("status")
            .and_then(serde_json::Value::as_str),
        Some("pass")
    );
}

#[test]
fn localnet_start_fails_when_process_exits_before_ready() {
    let temp = tempdir().expect("tempdir");
    let lez_path = temp.path().join("lez");
    let sequencer_bin = lez_path.join("target/release/sequencer_service");
    let config_path = lez_path.join("sequencer/service/configs/debug/sequencer_config.json");
    fs::create_dir_all(sequencer_bin.parent().expect("parent")).expect("create dirs");
    fs::create_dir_all(config_path.parent().expect("parent")).expect("create config dir");
    fs::write(&config_path, r#"{"port": 3040}"#).expect("write sequencer config");
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

    write_scaffold_toml(temp.path(), &lez_path);

    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .arg("localnet")
        .arg("start")
        .arg("--timeout-sec")
        .arg("1")
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("sequencer process exited before becoming ready")
                .or(predicate::str::contains("localnet start timed out after"))
                .or(predicate::str::contains(
                    "cannot start localnet: port 3040 is already in use",
                )),
        );

    assert!(
        !temp.path().join(".scaffold/state/localnet.state").exists(),
        "state file should be cleaned after failed startup"
    );
}

#[test]
fn localnet_start_patches_config_and_uses_configured_port() {
    let temp = tempdir().expect("tempdir");
    let lez_path = temp.path().join("lez");
    let sequencer_bin = lez_path.join("target/release/sequencer_service");
    let config_path = lez_path.join("sequencer/service/configs/debug/sequencer_config.json");
    let args_log = temp.path().join("sequencer-args.log");
    let env_log = temp.path().join("sequencer-env.log");
    let localnet_port = unused_local_port();

    fs::create_dir_all(sequencer_bin.parent().expect("parent")).expect("create dirs");
    fs::create_dir_all(config_path.parent().expect("parent")).expect("create config dir");
    fs::write(&config_path, r#"{"port": 3040}"#).expect("write sequencer config");

    // Fake sequencer: reads port from sequencer_config.json (like the real one),
    // logs args and env for assertions.
    fs::write(
        &sequencer_bin,
        format!(
            "#!/bin/sh\nset -eu\nprintf '%s\\n' \"$@\" > '{}'\nprintf '%s' \"${{RISC0_DEV_MODE:-}}\" > '{}'\nport=$(python3 -c \"import json,sys; print(json.load(open(sys.argv[1]))['port'])\" \"$1\")\nexec python3 -m http.server \"$port\" --bind 127.0.0.1\n",
            args_log.display(),
            env_log.display(),
        ),
    )
    .expect("write fake sequencer");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&sequencer_bin)
            .expect("metadata")
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&sequencer_bin, perms).expect("chmod");
    }

    write_scaffold_toml_with_localnet(temp.path(), &lez_path, Some(localnet_port), Some(false));

    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .arg("localnet")
        .arg("start")
        .arg("--timeout-sec")
        .arg("5")
        .assert()
        .success()
        .stdout(predicate::str::contains("localnet ready"));

    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .arg("localnet")
        .arg("stop")
        .assert()
        .success();

    // Verify sequencer_config.json was patched with the configured port
    let patched_config = fs::read_to_string(&config_path).expect("read patched config");
    let config_json: serde_json::Value =
        serde_json::from_str(&patched_config).expect("parse patched config");
    assert_eq!(
        config_json["port"],
        serde_json::Value::Number(localnet_port.into()),
        "expected port in sequencer_config.json to be patched to {localnet_port}, got: {patched_config}"
    );

    // Verify --port was NOT passed as a CLI arg
    let args = fs::read_to_string(&args_log).expect("read args log");
    assert!(
        !args.contains("--port"),
        "expected --port NOT to appear in sequencer args, got: {args}"
    );

    let env = fs::read_to_string(&env_log).expect("read env log");
    assert_eq!(env, "0", "expected risc0 dev mode override to be passed");
}

#[test]
fn localnet_stop_outside_project_succeeds() {
    let temp = tempdir().expect("tempdir");

    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .arg("localnet")
        .arg("stop")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("localnet not running").or(predicate::str::contains(
                "listener detected on 127.0.0.1:3040",
            )),
        );
}

#[test]
fn localnet_stop_outside_project_with_listener_prints_hint() {
    let temp = tempdir().expect("tempdir");

    match TcpListener::bind("127.0.0.1:3040") {
        Ok(listener) => {
            listener
                .set_nonblocking(true)
                .expect("set nonblocking listener");

            Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
                .current_dir(temp.path())
                .arg("localnet")
                .arg("stop")
                .assert()
                .success()
                .stdout(
                    predicate::str::contains("127.0.0.1:3040").and(
                        predicate::str::contains("Try: kill")
                            .or(predicate::str::contains("Try: lsof -nP -iTCP:3040")),
                    ),
                );
        }
        Err(_) => {
            Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
                .current_dir(temp.path())
                .arg("localnet")
                .arg("stop")
                .assert()
                .success()
                .stdout(predicate::str::contains("localnet not running").or(
                    predicate::str::contains("listener detected on 127.0.0.1:3040"),
                ));
        }
    }
}

#[test]
fn wallet_list_proxies_account_list() {
    let temp = tempdir().expect("tempdir");
    setup_wallet_project(temp.path(), Some("http://127.0.0.1:3040"));

    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .arg("wallet")
        .arg("list")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("account list")
                .and(predicate::str::contains("Preconfigured Public/")),
        );
}

#[test]
fn wallet_passthrough_account_list_works() {
    let temp = tempdir().expect("tempdir");
    setup_wallet_project(temp.path(), Some("http://127.0.0.1:3040"));

    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .arg("wallet")
        .arg("--")
        .arg("account")
        .arg("list")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("account list")
                .and(predicate::str::contains("Preconfigured Public/")),
        );
}

#[test]
fn wallet_passthrough_requires_args_after_double_dash() {
    let temp = tempdir().expect("tempdir");
    setup_wallet_project(temp.path(), Some("http://127.0.0.1:3040"));

    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .arg("wallet")
        .arg("--")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "wallet passthrough requires at least one argument after `--`",
        ));
}

#[test]
fn wallet_topup_dry_run_planned_network_uses_localnet_port_without_wallet_sequencer_addr() {
    let temp = tempdir().expect("tempdir");
    let lez_path = temp.path().join("lez");
    fs::create_dir_all(&lez_path).expect("create lez path");
    write_wallet_stub(&lez_path);
    let port = unused_local_port();
    write_scaffold_toml_with_localnet(temp.path(), &lez_path, Some(port), Some(true));
    write_wallet_config(temp.path(), None);

    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .arg("wallet")
        .arg("topup")
        .arg("--address")
        .arg(VALID_PUBLIC_ADDRESS)
        .arg("--dry-run")
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "planned network: local sequencer (http://127.0.0.1:{port})"
        )));
}

#[test]
fn wallet_topup_dry_run_renders_pinata_claim_command() {
    let temp = tempdir().expect("tempdir");
    setup_wallet_project(temp.path(), Some("http://127.0.0.1:3040"));

    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .arg("wallet")
        .arg("topup")
        .arg("--address")
        .arg(VALID_PUBLIC_ADDRESS)
        .arg("--dry-run")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("dry-run: wallet topup command will not be executed")
                .and(predicate::str::contains(
                    "planned preflight: check destination wallet initialization",
                ))
                .and(predicate::str::contains("auth-transfer init --account-id"))
                .and(predicate::str::contains("pinata claim --to"))
                .and(predicate::str::contains(
                    "planned method: pinata faucet claim",
                )),
        );
}

#[test]
fn wallet_topup_runs_pinata_claim_with_explicit_address() {
    let temp = tempdir().expect("tempdir");
    setup_wallet_project(temp.path(), Some("http://127.0.0.1:3040"));

    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .arg("wallet")
        .arg("topup")
        .arg("--address")
        .arg(VALID_PUBLIC_ADDRESS)
        .assert()
        .success()
        .stdout(
            predicate::str::contains("pinata claim --to Public/")
                .and(predicate::str::contains("wallet topup complete")),
        );
}

#[test]
fn wallet_topup_initializes_when_account_uninitialized_before_pinata() {
    let temp = tempdir().expect("tempdir");
    setup_wallet_project(temp.path(), Some("http://127.0.0.1:3040"));

    let assert = Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .env("TOPUP_ACCOUNT_STATE", "uninitialized")
        .env("TOPUP_GUARD_REQUIRE_INIT", "1")
        .arg("wallet")
        .arg("topup")
        .arg("--address")
        .arg(VALID_PUBLIC_ADDRESS)
        .assert()
        .success()
        .stdout(predicate::str::contains("wallet topup complete"));

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8 stdout");
    let init_pos = stdout
        .find("auth-transfer init --account-id Public/")
        .expect("init command should be present");
    let pinata_pos = stdout
        .find("pinata claim --to Public/")
        .expect("pinata command should be present");
    assert!(
        init_pos < pinata_pos,
        "auth-transfer init must run before pinata claim, got output:\n{stdout}"
    );
}

#[test]
fn wallet_topup_skips_init_when_account_already_initialized() {
    let temp = tempdir().expect("tempdir");
    setup_wallet_project(temp.path(), Some("http://127.0.0.1:3040"));

    let assert = Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .env("TOPUP_ACCOUNT_STATE", "initialized")
        .arg("wallet")
        .arg("topup")
        .arg("--address")
        .arg(VALID_PUBLIC_ADDRESS)
        .assert()
        .success();

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8 stdout");
    assert!(
        !stdout.contains("auth-transfer init --account-id"),
        "init command must not run for initialized accounts, got output:\n{stdout}"
    );
    assert!(
        stdout.contains("pinata claim --to Public/"),
        "pinata claim should still run, got output:\n{stdout}"
    );
}

#[test]
fn wallet_topup_preflight_failure_blocks_pinata() {
    let temp = tempdir().expect("tempdir");
    setup_wallet_project(temp.path(), Some("http://127.0.0.1:3040"));

    let assert = Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .env("TOPUP_PREFLIGHT_FAIL", "1")
        .arg("wallet")
        .arg("topup")
        .arg("--address")
        .arg(VALID_PUBLIC_ADDRESS)
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "wallet topup failed while checking account initialization",
        ));

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8 stdout");
    assert!(
        !stdout.contains("pinata claim --to"),
        "pinata must not run when preflight fails, got output:\n{stdout}"
    );
}

#[test]
fn wallet_topup_uses_password_env_override() {
    let temp = tempdir().expect("tempdir");
    setup_wallet_project(temp.path(), Some("http://127.0.0.1:3040"));

    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .env("EXPECT_PASSWORD", "override-pass")
        .env("LOGOS_SCAFFOLD_WALLET_PASSWORD", "override-pass")
        .arg("wallet")
        .arg("topup")
        .arg("--address")
        .arg(VALID_PUBLIC_ADDRESS)
        .assert()
        .success()
        .stdout(predicate::str::contains("wallet topup complete"));
}

#[test]
fn wallet_topup_falls_back_to_default_password_when_env_missing() {
    let temp = tempdir().expect("tempdir");
    setup_wallet_project(temp.path(), Some("http://127.0.0.1:3040"));

    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .env("EXPECT_PASSWORD", DEFAULT_WALLET_PASSWORD)
        .arg("wallet")
        .arg("topup")
        .arg("--address")
        .arg(VALID_PUBLIC_ADDRESS)
        .assert()
        .success()
        .stdout(predicate::str::contains("wallet topup complete"));
}

#[test]
fn wallet_topup_uses_default_wallet_when_address_is_omitted() {
    let temp = tempdir().expect("tempdir");
    setup_wallet_project(temp.path(), Some("http://127.0.0.1:3040"));

    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .arg("wallet")
        .arg("default")
        .arg("set")
        .arg(VALID_PUBLIC_ADDRESS)
        .assert()
        .success();

    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .arg("wallet")
        .arg("topup")
        .assert()
        .success()
        .stdout(predicate::str::contains("pinata claim --to Public/"));
}

#[test]
fn wallet_topup_errors_when_address_and_default_are_missing() {
    let temp = tempdir().expect("tempdir");
    setup_wallet_project(temp.path(), Some("http://127.0.0.1:3040"));

    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .arg("wallet")
        .arg("topup")
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("wallet topup requires a destination address")
                .and(predicate::str::contains("logos-scaffold wallet list")),
        );
}

#[test]
fn wallet_topup_rejects_invalid_address() {
    let temp = tempdir().expect("tempdir");
    setup_wallet_project(temp.path(), Some("http://127.0.0.1:3040"));

    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .arg("wallet")
        .arg("topup")
        .arg("--address")
        .arg("abc")
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("invalid address format `abc`")
                .and(predicate::str::contains("Accepted formats")),
        );
}

#[test]
fn wallet_topup_shows_sequencer_hint_on_connectivity_failure() {
    let temp = tempdir().expect("tempdir");
    setup_wallet_project(temp.path(), Some("http://127.0.0.1:3040"));

    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .env("TOPUP_FAIL_CONNECT", "1")
        .arg("wallet")
        .arg("topup")
        .arg("--address")
        .arg(VALID_PUBLIC_ADDRESS)
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("sequencer appears unavailable")
                .and(predicate::str::contains("logos-scaffold localnet start"))
                .and(predicate::str::contains("Another project's sequencer")),
        );
}

#[test]
fn wallet_topup_init_connectivity_failure_shows_sequencer_hint() {
    let temp = tempdir().expect("tempdir");
    setup_wallet_project(temp.path(), Some("http://127.0.0.1:3040"));

    let assert = Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .env("TOPUP_ACCOUNT_STATE", "uninitialized")
        .env("TOPUP_INIT_FAIL_CONNECT", "1")
        .arg("wallet")
        .arg("topup")
        .arg("--address")
        .arg(VALID_PUBLIC_ADDRESS)
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("sequencer appears unavailable")
                .and(predicate::str::contains("logos-scaffold localnet start"))
                .and(predicate::str::contains("Another project's sequencer")),
        );

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8 stdout");
    assert!(
        !stdout.contains("pinata claim --to"),
        "pinata must not run when init fails with connectivity error, got output:\n{stdout}"
    );
}

#[test]
fn wallet_topup_continues_when_init_reports_already_initialized() {
    let temp = tempdir().expect("tempdir");
    setup_wallet_project(temp.path(), Some("http://127.0.0.1:3040"));

    let assert = Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .env("TOPUP_ACCOUNT_STATE", "uninitialized")
        .env("TOPUP_INIT_FAIL_ALREADY", "1")
        .arg("wallet")
        .arg("topup")
        .arg("--address")
        .arg(VALID_PUBLIC_ADDRESS)
        .assert()
        .success()
        .stdout(
            predicate::str::contains("destination already initialized; continuing")
                .and(predicate::str::contains("wallet topup complete")),
        );

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8 stdout");
    assert!(
        stdout.contains("pinata claim --to Public/"),
        "pinata should run after tolerated init race, got output:\n{stdout}"
    );
}

#[test]
fn wallet_topup_timeout_is_reported_as_non_fatal() {
    let temp = tempdir().expect("tempdir");
    setup_wallet_project(temp.path(), Some("http://127.0.0.1:3040"));

    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .env("TOPUP_FAIL_TIMEOUT", "1")
        .arg("wallet")
        .arg("topup")
        .arg("--address")
        .arg(VALID_PUBLIC_ADDRESS)
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "wallet topup submitted, but confirmation timed out",
        ));
}

#[test]
fn wallet_topup_fails_outside_project_with_project_scoped_message() {
    let temp = tempdir().expect("tempdir");

    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .arg("wallet")
        .arg("topup")
        .arg("--address")
        .arg(VALID_PUBLIC_ADDRESS)
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "This command must be run inside a logos-scaffold project.",
        ));
}

#[test]
fn wallet_default_set_persists_normalized_address_positional() {
    let temp = tempdir().expect("tempdir");
    setup_wallet_project(temp.path(), Some("http://127.0.0.1:3040"));

    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .arg("wallet")
        .arg("default")
        .arg("set")
        .arg(VALID_ACCOUNT_ID)
        .assert()
        .success()
        .stdout(predicate::str::contains("default wallet updated"));

    let state_path = temp.path().join(".scaffold/state/wallet.state");
    let state = fs::read_to_string(state_path).expect("read wallet.state");
    assert_eq!(state, format!("default_address={VALID_PUBLIC_ADDRESS}\n"));
}

#[test]
fn wallet_default_set_accepts_flag_form() {
    let temp = tempdir().expect("tempdir");
    setup_wallet_project(temp.path(), Some("http://127.0.0.1:3040"));

    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .arg("wallet")
        .arg("default")
        .arg("set")
        .arg("--address")
        .arg(VALID_PUBLIC_ADDRESS)
        .assert()
        .success()
        .stdout(predicate::str::contains("default wallet updated"));
}

#[test]
fn deploy_unknown_program_lists_available_programs() {
    let temp = tempdir().expect("tempdir");
    setup_wallet_project(temp.path(), None);
    write_guest_program(temp.path(), "alpha");
    write_guest_program(temp.path(), "beta");

    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .arg("deploy")
        .arg("missing")
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("unknown program `missing`")
                .and(predicate::str::contains("alpha"))
                .and(predicate::str::contains("beta")),
        );
}

#[test]
fn deploy_single_program_submits_successfully() {
    let temp = tempdir().expect("tempdir");
    let rpc = RpcStub::start();
    setup_wallet_project(temp.path(), Some(&rpc.url));
    write_guest_program(temp.path(), "hello");
    write_guest_binary(temp.path(), "hello");

    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .arg("deploy")
        .arg("hello")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("OK  hello submitted")
                .and(predicate::str::contains(
                    "Submission confirmed by wallet exit status",
                ))
                .and(predicate::str::contains("Succeeded: 1"))
                .and(predicate::str::contains("Failed: 0"))
                .and(predicate::str::contains("reachability probe failed").not()),
        );
}

#[test]
fn deploy_uses_password_env_override() {
    let temp = tempdir().expect("tempdir");
    let rpc = RpcStub::start();
    setup_wallet_project(temp.path(), Some(&rpc.url));
    write_guest_program(temp.path(), "hello");
    write_guest_binary(temp.path(), "hello");

    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .env("EXPECT_PASSWORD", "override-pass")
        .env("LOGOS_SCAFFOLD_WALLET_PASSWORD", "override-pass")
        .arg("deploy")
        .arg("hello")
        .assert()
        .success()
        .stdout(predicate::str::contains("OK  hello submitted"));
}

#[test]
fn deploy_missing_binary_shows_build_hint() {
    let temp = tempdir().expect("tempdir");
    let rpc = RpcStub::start();
    setup_wallet_project(temp.path(), Some(&rpc.url));
    write_guest_program(temp.path(), "hello");

    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .arg("deploy")
        .arg("hello")
        .assert()
        .failure()
        .stdout(
            predicate::str::contains("missing binary")
                .and(predicate::str::contains("logos-scaffold build")),
        );
}

#[test]
fn deploy_continues_and_summarizes_mixed_results() {
    let temp = tempdir().expect("tempdir");
    let rpc = RpcStub::start();
    setup_wallet_project(temp.path(), Some(&rpc.url));
    write_guest_program(temp.path(), "alpha");
    write_guest_program(temp.path(), "beta");
    write_guest_binary(temp.path(), "alpha");
    write_guest_binary(temp.path(), "beta");

    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .env("FAIL_PROGRAM", "beta.bin")
        .arg("deploy")
        .assert()
        .failure()
        .stdout(
            predicate::str::contains("OK  alpha submitted")
                .and(predicate::str::contains("FAIL beta deployment failed"))
                .and(predicate::str::contains("Succeeded: 1"))
                .and(predicate::str::contains("Failed: 1")),
        );
}

#[test]
fn deploy_shows_hint_when_sequencer_is_unreachable_with_configured_addr() {
    let temp = tempdir().expect("tempdir");
    setup_wallet_project(temp.path(), Some("http://127.0.0.1:65535"));
    write_guest_program(temp.path(), "hello");
    write_guest_binary(temp.path(), "hello");

    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .arg("deploy")
        .arg("hello")
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("sequencer appears unavailable")
                .and(predicate::str::contains("logos-scaffold localnet start"))
                .and(predicate::str::contains("Another project's sequencer")),
        );
}

#[test]
fn deploy_shows_hint_when_sequencer_is_unreachable_with_fallback_addr() {
    // Wallet omits `sequencer_addr`; deploy falls back to `http://127.0.0.1:<localnet.port>`
    // (default 3040 when `[localnet]` is omitted). Skip if something is already listening.
    if TcpStream::connect("127.0.0.1:3040").is_ok() {
        return;
    }

    let temp = tempdir().expect("tempdir");
    setup_wallet_project(temp.path(), None);
    write_guest_program(temp.path(), "hello");
    write_guest_binary(temp.path(), "hello");

    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp.path())
        .arg("deploy")
        .arg("hello")
        .assert()
        .failure()
        .stderr(predicate::str::contains("sequencer appears unavailable"));
}

fn find_single_report_archive(reports_dir: &Path) -> PathBuf {
    let mut archives = list_report_archives(reports_dir);
    assert_eq!(
        archives.len(),
        1,
        "expected exactly one report archive in {}",
        reports_dir.display()
    );
    archives.remove(0)
}

fn list_report_archives(reports_dir: &Path) -> Vec<PathBuf> {
    let mut archives = fs::read_dir(reports_dir)
        .expect("read reports dir")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(|name| name.ends_with(".tar.gz"))
                    .unwrap_or(false)
        })
        .collect::<Vec<_>>();

    archives.sort();
    archives
}

fn read_report_archive_entries(archive_path: &Path) -> Vec<(String, String)> {
    let file = fs::File::open(archive_path).expect("open report archive");
    let decoder = GzDecoder::new(file);
    let mut archive = Archive::new(decoder);

    let mut entries = Vec::new();
    for entry in archive.entries().expect("archive entries") {
        let mut entry = entry.expect("archive entry");
        if !entry.header().entry_type().is_file() {
            continue;
        }

        let path = entry
            .path()
            .expect("archive entry path")
            .display()
            .to_string();
        let mut body = String::new();
        entry.read_to_string(&mut body).expect("archive entry body");
        entries.push((path, body));
    }

    entries
}

fn archive_entry_exists(entries: &[(String, String)], suffix: &str) -> bool {
    entries.iter().any(|(path, _)| path.ends_with(suffix))
}

fn archive_entry_content<'a>(entries: &'a [(String, String)], suffix: &str) -> &'a str {
    entries
        .iter()
        .find(|(path, _)| path.ends_with(suffix))
        .map(|(_, body)| body.as_str())
        .unwrap_or_else(|| panic!("archive missing expected entry suffix `{suffix}`"))
}

fn write_scaffold_toml(project_root: &Path, lez_path: &Path) {
    write_scaffold_toml_with_localnet(project_root, lez_path, None, None);
}

fn write_scaffold_toml_with_localnet(
    project_root: &Path,
    lez_path: &Path,
    localnet_port: Option<u16>,
    risc0_dev_mode: Option<bool>,
) {
    let mut content = format!(
        "[scaffold]\nversion = \"0.1.0\"\ncache_root = \"{}\"\n\n[repos.lez]\nurl = \"https://github.com/logos-blockchain/logos-execution-zone.git\"\nsource = \"https://github.com/logos-blockchain/logos-execution-zone.git\"\npath = \"{}\"\npin = \"{}\"\n\n[wallet]\nhome_dir = \".scaffold/wallet\"\n",
        project_root.join("cache").display(),
        lez_path.display(),
        TEST_PIN,
    );

    if let Some(port) = localnet_port {
        let risc0_dev_mode = risc0_dev_mode.unwrap_or(true);
        content.push_str(&format!(
            "\n[localnet]\nport = {port}\nrisc0_dev_mode = {risc0_dev_mode}\n"
        ));
    }

    fs::write(project_root.join("scaffold.toml"), content).expect("write scaffold.toml");
}

fn unused_local_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("bind unused local port")
        .local_addr()
        .expect("local addr")
        .port()
}

fn setup_wallet_project(project_root: &Path, sequencer_addr: Option<&str>) {
    let lez_path = project_root.join("lez");
    fs::create_dir_all(&lez_path).expect("create lez path");
    write_wallet_stub(&lez_path);
    write_scaffold_toml(project_root, &lez_path);
    write_wallet_config(project_root, sequencer_addr);
}

fn write_wallet_config(project_root: &Path, sequencer_addr: Option<&str>) {
    let wallet_home = project_root.join(".scaffold/wallet");
    fs::create_dir_all(&wallet_home).expect("create wallet home");
    let path = wallet_home.join("wallet_config.json");

    let mut value = serde_json::json!({
        "initial_accounts": [
            { "Public": { "account_id": VALID_ACCOUNT_ID } }
        ]
    });
    if let Some(addr) = sequencer_addr {
        value["sequencer_addr"] = serde_json::Value::String(addr.to_string());
    }

    let text = serde_json::to_string_pretty(&value).expect("wallet config json");
    fs::write(path, text).expect("write wallet config");
}

fn write_wallet_stub(lez_path: &Path) {
    let path = lez_path.join("target/release/wallet");
    fs::create_dir_all(path.parent().expect("parent")).expect("create wallet binary dir");
    let script = r#"#!/bin/sh
set -eu

require_password_if_configured() {
  if [ "${EXPECT_PASSWORD:-}" = "" ]; then
    return 0
  fi
  IFS= read -r provided || true
  if [ "$provided" != "$EXPECT_PASSWORD" ]; then
    echo "password mismatch" >&2
    exit 3
  fi
}

if [ "$#" -ge 2 ] && [ "$1" = "account" ] && [ "$2" = "list" ]; then
  echo "Preconfigured Public/6iArKUXxhUJqS7kCaPNhwMWt3ro71PDyBj7jwAyE2VQV"
  echo "/ Public/8zxWNm1qh6FLsJpVBuDxdxcTm55qHPgFEdqJpPVu1fuy"
  exit 0
fi

if [ "$#" -ge 2 ] && [ "$1" = "account" ] && [ "$2" = "get" ]; then
  if [ "${TOPUP_PREFLIGHT_FAIL:-0}" = "1" ]; then
    echo "simulated account get failure" >&2
    exit 4
  fi
  if [ "${TOPUP_ACCOUNT_STATE:-initialized}" = "uninitialized" ]; then
    echo "Account is Uninitialized"
  else
    echo "Account state: Initialized"
  fi
  exit 0
fi

if [ "$#" -ge 3 ] && [ "$1" = "auth-transfer" ] && [ "$2" = "init" ] && [ "$3" = "--account-id" ]; then
  require_password_if_configured
  if [ "${TOPUP_INIT_FAIL_CONNECT:-0}" = "1" ]; then
    echo "connection refused" >&2
    exit 1
  fi
  if [ "${TOPUP_INIT_FAIL_ALREADY:-0}" = "1" ]; then
    echo "Error: Account must be uninitialized" >&2
    exit 1
  fi
  marker_path="${NSSA_WALLET_HOME_DIR:-.}/.topup-init-ran"
  : > "$marker_path"
  echo "init ok"
  exit 0
fi

if [ "$#" -ge 2 ] && [ "$1" = "pinata" ] && [ "$2" = "claim" ]; then
  require_password_if_configured
  if [ "${TOPUP_GUARD_REQUIRE_INIT:-0}" = "1" ]; then
    marker_path="${NSSA_WALLET_HOME_DIR:-.}/.topup-init-ran"
    if [ ! -f "$marker_path" ]; then
      echo "pinata called before init" >&2
      exit 9
    fi
  fi
  if [ "${TOPUP_FAIL_CONNECT:-0}" = "1" ]; then
    echo "connection refused" >&2
    exit 1
  fi
  if [ "${TOPUP_FAIL_TIMEOUT:-0}" = "1" ]; then
    echo "Error: Transaction not found in preconfigured amount of blocks" >&2
    exit 1
  fi
  echo "tx_hash=pinata-topup-hash"
  exit 0
fi

if [ "$#" -ge 2 ] && [ "$1" = "deploy-program" ]; then
  require_password_if_configured
  bin_path="$2"
  bin_name="$(basename "$bin_path")"
  if [ "${FAIL_PROGRAM:-}" = "$bin_name" ]; then
    echo "simulated deploy failure for $bin_name" >&2
    exit 2
  fi
  echo "tx_hash=deploy-$bin_name"
  exit 0
fi

if [ "$#" -ge 1 ] && [ "$1" = "--version" ]; then
  echo "wallet stub 0.1.0"
  exit 0
fi

if [ "$#" -ge 1 ] && [ "$1" = "check-health" ]; then
  require_password_if_configured
  echo "ok"
  exit 0
fi

echo "unsupported wallet invocation: $*" >&2
exit 2
"#;
    fs::write(&path, script).expect("write wallet stub");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&path).expect("metadata").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms).expect("chmod");
    }
}

fn write_guest_program(project_root: &Path, name: &str) {
    let dir = project_root.join("methods/guest/src/bin");
    fs::create_dir_all(&dir).expect("create guest program dir");
    fs::write(dir.join(format!("{name}.rs")), "fn main() {}\n").expect("write guest source");
}

fn write_guest_binary(project_root: &Path, name: &str) {
    let dir = project_root.join(GUEST_BIN_REL_PATH);
    fs::create_dir_all(&dir).expect("create guest binary dir");
    fs::write(dir.join(format!("{name}.bin")), b"stub-program-bin").expect("write guest binary");
}

struct RpcStub {
    url: String,
    stop: Arc<AtomicBool>,
    addr: String,
    handle: Option<thread::JoinHandle<()>>,
}

impl RpcStub {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind rpc stub");
        let addr = listener.local_addr().expect("local addr");
        let addr_str = addr.to_string();
        listener
            .set_nonblocking(true)
            .expect("set nonblocking rpc stub");

        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = Arc::clone(&stop);

        let handle = thread::spawn(move || {
            while !stop_flag.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        respond_last_block(&mut stream);
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });

        Self {
            url: format!("http://{addr_str}"),
            stop,
            addr: addr_str,
            handle: Some(handle),
        }
    }
}

impl Drop for RpcStub {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(&self.addr);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn respond_last_block(stream: &mut TcpStream) {
    let mut buf = [0_u8; 4096];
    let _ = stream.read(&mut buf);

    let body = r#"{"jsonrpc":"2.0","result":123,"id":1}"#;
    let response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

#[test]
fn lgs_help_usage_line_shows_lgs() {
    Command::new(assert_cmd::cargo::cargo_bin!("lgs"))
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage: lgs"));
}

#[test]
fn logos_scaffold_help_usage_line_shows_logos_scaffold() {
    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage: logos-scaffold"));
}

#[test]
fn lgs_help_subcommand_uses_invoked_bin_name() {
    Command::new(assert_cmd::cargo::cargo_bin!("lgs"))
        .arg("help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage: lgs"));
}

#[test]
fn logos_scaffold_help_subcommand_uses_invoked_bin_name() {
    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .arg("help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage: logos-scaffold"));
}

#[test]
fn lgs_no_args_uses_invoked_bin_name() {
    Command::new(assert_cmd::cargo::cargo_bin!("lgs"))
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage: lgs"));
}

#[test]
fn logos_scaffold_no_args_uses_invoked_bin_name() {
    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage: logos-scaffold"));
}

#[test]
fn lgs_and_logos_scaffold_advertise_same_subcommands() {
    let subcommands = [
        "create", "new", "setup", "build", "deploy", "wallet", "localnet", "doctor", "report",
    ];

    let lgs_help = Command::new(assert_cmd::cargo::cargo_bin!("lgs"))
        .arg("--help")
        .output()
        .expect("run lgs --help");
    let ls_help = Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .arg("--help")
        .output()
        .expect("run logos-scaffold --help");

    assert!(lgs_help.status.success(), "lgs --help failed");
    assert!(ls_help.status.success(), "logos-scaffold --help failed");

    let lgs_out = String::from_utf8_lossy(&lgs_help.stdout);
    let ls_out = String::from_utf8_lossy(&ls_help.stdout);

    for sub in subcommands {
        assert!(lgs_out.contains(sub), "lgs help missing subcommand `{sub}`");
        assert!(
            ls_out.contains(sub),
            "logos-scaffold help missing subcommand `{sub}`"
        );
    }
}

#[test]
fn lgs_and_logos_scaffold_version_match() {
    let lgs_ver = Command::new(assert_cmd::cargo::cargo_bin!("lgs"))
        .arg("--version")
        .output()
        .expect("run lgs --version");
    let ls_ver = Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .arg("--version")
        .output()
        .expect("run logos-scaffold --version");

    assert!(lgs_ver.status.success());
    assert!(ls_ver.status.success());

    let lgs_version_number = String::from_utf8_lossy(&lgs_ver.stdout)
        .split_whitespace()
        .last()
        .unwrap_or_default()
        .to_string();
    let ls_version_number = String::from_utf8_lossy(&ls_ver.stdout)
        .split_whitespace()
        .last()
        .unwrap_or_default()
        .to_string();

    assert_eq!(
        lgs_version_number, ls_version_number,
        "version numbers differ"
    );
    assert!(!lgs_version_number.is_empty(), "version number is empty");
}

#[test]
fn completions_bash_prints_script_covering_both_bin_names() {
    let output = Command::new(assert_cmd::cargo::cargo_bin!("lgs"))
        .args(["completions", "bash"])
        .output()
        .expect("run lgs completions bash");
    assert!(output.status.success(), "expected success exit");
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(
        stdout.contains("complete -F _lgs"),
        "missing primary binding: {stdout}"
    );
    assert!(
        stdout.contains("logos-scaffold"),
        "missing alias binding: {stdout}"
    );
}

#[test]
fn completions_zsh_compdef_directive_covers_both_names() {
    let output = Command::new(assert_cmd::cargo::cargo_bin!("lgs"))
        .args(["completions", "zsh"])
        .output()
        .expect("run lgs completions zsh");
    assert!(output.status.success(), "expected success exit");
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let compdef_headers = stdout.matches("#compdef").count();
    assert_eq!(
        compdef_headers, 1,
        "expected exactly one #compdef header, got {compdef_headers}: {stdout}"
    );
    assert!(
        stdout.starts_with("#compdef lgs logos-scaffold\n"),
        "expected `#compdef lgs logos-scaffold` directive so autoload \
         registers both names at compinit time; got head: {:?}",
        stdout.lines().next()
    );
}

#[test]
fn completions_bash_output_is_syntax_clean() {
    let output = Command::new(assert_cmd::cargo::cargo_bin!("lgs"))
        .args(["completions", "bash"])
        .output()
        .expect("run lgs completions bash");
    assert!(output.status.success(), "expected success exit");

    let temp = tempdir().expect("tempdir");
    let path = temp.path().join("lgs.bash");
    fs::write(&path, &output.stdout).expect("write script");

    let syntax = std::process::Command::new("bash")
        .arg("-n")
        .arg(&path)
        .output()
        .expect("bash -n");
    assert!(
        syntax.status.success(),
        "bash -n failed: {}",
        String::from_utf8_lossy(&syntax.stderr)
    );
}

#[test]
fn completions_zsh_output_is_syntax_clean() {
    if std::process::Command::new("zsh")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("skipping: zsh not available");
        return;
    }

    let output = Command::new(assert_cmd::cargo::cargo_bin!("lgs"))
        .args(["completions", "zsh"])
        .output()
        .expect("run lgs completions zsh");
    assert!(output.status.success(), "expected success exit");

    let temp = tempdir().expect("tempdir");
    let path = temp.path().join("_lgs");
    fs::write(&path, &output.stdout).expect("write script");

    let syntax = std::process::Command::new("zsh")
        .arg("-n")
        .arg(&path)
        .output()
        .expect("zsh -n");
    assert!(
        syntax.status.success(),
        "zsh -n failed: {}",
        String::from_utf8_lossy(&syntax.stderr)
    );
}

#[test]
fn completions_unsupported_shell_errors() {
    Command::new(assert_cmd::cargo::cargo_bin!("lgs"))
        .args(["completions", "fish"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("fish"));
}

#[test]
fn completions_bash_help_shows_install_instructions() {
    Command::new(assert_cmd::cargo::cargo_bin!("lgs"))
        .args(["completions", "bash", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("bash-completion/completions/lgs")
                .and(predicate::str::contains("logos-scaffold")),
        );
}

#[test]
fn completions_zsh_help_shows_install_instructions() {
    Command::new(assert_cmd::cargo::cargo_bin!("lgs"))
        .args(["completions", "zsh", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("~/.zfunc/_lgs")
                .and(predicate::str::contains("oh-my-zsh"))
                .and(predicate::str::contains("compinit")),
        );
}

#[test]
fn completions_missing_shell_arg_errors() {
    Command::new(assert_cmd::cargo::cargo_bin!("lgs"))
        .arg("completions")
        .assert()
        .failure();
}

#[test]
fn completions_does_not_write_filesystem() {
    let temp = tempdir().expect("tempdir");
    Command::new(assert_cmd::cargo::cargo_bin!("lgs"))
        .current_dir(temp.path())
        .args(["completions", "bash"])
        .assert()
        .success();

    let entries: Vec<_> = fs::read_dir(temp.path())
        .expect("read tempdir")
        .filter_map(|e| e.ok())
        .collect();
    assert!(
        entries.is_empty(),
        "completions must not write to cwd, found: {:?}",
        entries.iter().map(|e| e.path()).collect::<Vec<_>>()
    );
}

#[test]
fn init_creates_scaffold_toml_and_dirs() {
    let temp = tempdir().expect("tempdir");

    Command::new(assert_cmd::cargo::cargo_bin!("lgs"))
        .current_dir(temp.path())
        .arg("init")
        .assert()
        .success();

    assert!(
        temp.path().join("scaffold.toml").exists(),
        "scaffold.toml missing"
    );
    assert!(
        temp.path().join(".scaffold/state").is_dir(),
        ".scaffold/state missing"
    );
    assert!(
        temp.path().join(".scaffold/logs").is_dir(),
        ".scaffold/logs missing"
    );

    let gitignore = fs::read_to_string(temp.path().join(".gitignore")).expect("read .gitignore");
    assert!(
        gitignore.lines().any(|l| l.trim() == ".scaffold"),
        ".gitignore must contain .scaffold, got: {gitignore:?}"
    );
}

#[test]
fn init_refuses_existing_scaffold_toml() {
    let temp = tempdir().expect("tempdir");
    let scaffold_path = temp.path().join("scaffold.toml");
    let original = "# pre-existing\n";
    fs::write(&scaffold_path, original).expect("seed scaffold.toml");

    Command::new(assert_cmd::cargo::cargo_bin!("lgs"))
        .current_dir(temp.path())
        .arg("init")
        .assert()
        .failure()
        .stderr(predicate::str::contains("scaffold.toml already exists"));

    let after = fs::read_to_string(&scaffold_path).expect("read scaffold.toml");
    assert_eq!(
        after, original,
        "init must not overwrite existing scaffold.toml"
    );
}

#[test]
fn init_appends_gitignore_once() {
    let temp = tempdir().expect("tempdir");
    fs::write(temp.path().join(".gitignore"), "target\n.scaffold\nother\n")
        .expect("seed .gitignore");

    Command::new(assert_cmd::cargo::cargo_bin!("lgs"))
        .current_dir(temp.path())
        .arg("init")
        .assert()
        .success();

    let gitignore = fs::read_to_string(temp.path().join(".gitignore")).expect("read .gitignore");
    let scaffold_count = gitignore
        .lines()
        .filter(|l| l.trim() == ".scaffold")
        .count();
    assert_eq!(
        scaffold_count, 1,
        ".gitignore must contain .scaffold exactly once, got: {gitignore:?}"
    );
}

#[test]
fn init_hint_uses_invoked_bin_name() {
    let temp_lgs = tempdir().expect("tempdir");
    Command::new(assert_cmd::cargo::cargo_bin!("lgs"))
        .current_dir(temp_lgs.path())
        .arg("init")
        .assert()
        .success()
        .stdout(predicate::str::contains("Run 'lgs setup'"));

    let temp_long = tempdir().expect("tempdir");
    Command::new(assert_cmd::cargo::cargo_bin!("logos-scaffold"))
        .current_dir(temp_long.path())
        .arg("init")
        .assert()
        .success()
        .stdout(predicate::str::contains("Run 'logos-scaffold setup'"));
}

#[test]
fn completions_zsh_registers_both_names_in_pristine_shell() {
    if std::process::Command::new("zsh")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("skipping: zsh not available");
        return;
    }

    let output = Command::new(assert_cmd::cargo::cargo_bin!("lgs"))
        .args(["completions", "zsh"])
        .output()
        .expect("run lgs completions zsh");
    assert!(output.status.success(), "expected success exit");

    let temp = tempdir().expect("tempdir");
    let fpath_dir = temp.path().join("fpath");
    fs::create_dir_all(&fpath_dir).expect("mkdir fpath");
    fs::write(fpath_dir.join("_lgs"), &output.stdout).expect("write _lgs");

    // Run a pristine zsh (-f skips rc files) with only our fpath plus
    // system completion functions, then verify both names are registered
    // at compinit time — not deferred to first tab.
    let script = format!(
        "fpath=({} /usr/share/zsh/*/functions); \
         autoload -Uz compinit && compinit -u -d {}/zcompdump; \
         print \"lgs=${{_comps[lgs]:-MISSING}}\"; \
         print \"logos-scaffold=${{_comps[logos-scaffold]:-MISSING}}\"",
        fpath_dir.display(),
        temp.path().display(),
    );

    let zsh_output = std::process::Command::new("zsh")
        .args(["-f", "-c", &script])
        .output()
        .expect("run pristine zsh");
    let stdout = String::from_utf8_lossy(&zsh_output.stdout);
    assert!(
        stdout.contains("lgs=_lgs"),
        "expected lgs to be registered, got: {stdout}"
    );
    assert!(
        stdout.contains("logos-scaffold=_lgs"),
        "expected logos-scaffold to be registered at compinit time, got: {stdout}"
    );
}
