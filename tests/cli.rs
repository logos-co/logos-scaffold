use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::Duration;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

const TEST_PIN: &str = "dee3f7fa6f2bf63abef00828f246ddacade9cdaf";
const VALID_ACCOUNT_ID: &str = "6iArKUXxhUJqS7kCaPNhwMWt3ro71PDyBj7jwAyE2VQV";
const VALID_PUBLIC_ADDRESS: &str = "Public/6iArKUXxhUJqS7kCaPNhwMWt3ro71PDyBj7jwAyE2VQV";
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
        .stdout(predicate::str::contains("deploy [PROGRAM_NAME]"));
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
    let wallet_stub = write_wallet_stub(temp.path());
    setup_wallet_project(temp.path(), &wallet_stub, Some("http://127.0.0.1:3040"));

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
    let wallet_stub = write_wallet_stub(temp.path());
    setup_wallet_project(temp.path(), &wallet_stub, Some("http://127.0.0.1:3040"));

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
    let wallet_stub = write_wallet_stub(temp.path());
    setup_wallet_project(temp.path(), &wallet_stub, Some("http://127.0.0.1:3040"));

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
fn wallet_topup_dry_run_renders_pinata_claim_command() {
    let temp = tempdir().expect("tempdir");
    let wallet_stub = write_wallet_stub(temp.path());
    setup_wallet_project(temp.path(), &wallet_stub, Some("http://127.0.0.1:3040"));

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
                .and(predicate::str::contains("pinata claim --to"))
                .and(predicate::str::contains(
                    "planned method: pinata faucet claim",
                )),
        );
}

#[test]
fn wallet_topup_runs_pinata_claim_with_explicit_address() {
    let temp = tempdir().expect("tempdir");
    let wallet_stub = write_wallet_stub(temp.path());
    setup_wallet_project(temp.path(), &wallet_stub, Some("http://127.0.0.1:3040"));

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
fn wallet_topup_uses_default_wallet_when_address_is_omitted() {
    let temp = tempdir().expect("tempdir");
    let wallet_stub = write_wallet_stub(temp.path());
    setup_wallet_project(temp.path(), &wallet_stub, Some("http://127.0.0.1:3040"));

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
    let wallet_stub = write_wallet_stub(temp.path());
    setup_wallet_project(temp.path(), &wallet_stub, Some("http://127.0.0.1:3040"));

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
    let wallet_stub = write_wallet_stub(temp.path());
    setup_wallet_project(temp.path(), &wallet_stub, Some("http://127.0.0.1:3040"));

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
    let wallet_stub = write_wallet_stub(temp.path());
    setup_wallet_project(temp.path(), &wallet_stub, Some("http://127.0.0.1:3040"));

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
    let wallet_stub = write_wallet_stub(temp.path());
    setup_wallet_project(temp.path(), &wallet_stub, Some("http://127.0.0.1:3040"));

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
    let wallet_stub = write_wallet_stub(temp.path());
    setup_wallet_project(temp.path(), &wallet_stub, Some("http://127.0.0.1:3040"));

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
    let wallet_stub = write_wallet_stub(temp.path());
    setup_wallet_project(temp.path(), &wallet_stub, None);
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
    let wallet_stub = write_wallet_stub(temp.path());
    setup_wallet_project(temp.path(), &wallet_stub, Some(&rpc.url));
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
                .and(predicate::str::contains("Failed: 0")),
        );
}

#[test]
fn deploy_missing_binary_shows_build_hint() {
    let temp = tempdir().expect("tempdir");
    let rpc = RpcStub::start();
    let wallet_stub = write_wallet_stub(temp.path());
    setup_wallet_project(temp.path(), &wallet_stub, Some(&rpc.url));
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
    let wallet_stub = write_wallet_stub(temp.path());
    setup_wallet_project(temp.path(), &wallet_stub, Some(&rpc.url));
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
    let wallet_stub = write_wallet_stub(temp.path());
    setup_wallet_project(temp.path(), &wallet_stub, Some("http://127.0.0.1:65535"));
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
    let temp = tempdir().expect("tempdir");
    let wallet_stub = write_wallet_stub(temp.path());
    setup_wallet_project(temp.path(), &wallet_stub, None);
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

fn setup_wallet_project(project_root: &Path, wallet_binary: &str, sequencer_addr: Option<&str>) {
    let lssa_path = project_root.join("lssa");
    fs::create_dir_all(&lssa_path).expect("create lssa path");
    write_scaffold_toml(project_root, &lssa_path, wallet_binary);
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

fn write_wallet_stub(project_root: &Path) -> String {
    let path = project_root.join("wallet-stub.sh");
    let script = r#"#!/bin/sh
set -eu

if [ "$#" -ge 2 ] && [ "$1" = "account" ] && [ "$2" = "list" ]; then
  echo "Preconfigured Public/6iArKUXxhUJqS7kCaPNhwMWt3ro71PDyBj7jwAyE2VQV"
  echo "/ Public/8zxWNm1qh6FLsJpVBuDxdxcTm55qHPgFEdqJpPVu1fuy"
  exit 0
fi

if [ "$#" -ge 2 ] && [ "$1" = "pinata" ] && [ "$2" = "claim" ]; then
  if [ "${TOPUP_FAIL_CONNECT:-0}" = "1" ]; then
    echo "connection refused" >&2
    exit 1
  fi
  echo "tx_hash=pinata-topup-hash"
  exit 0
fi

if [ "$#" -ge 2 ] && [ "$1" = "deploy-program" ]; then
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

    path.to_string_lossy().to_string()
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

    let body = r#"{"jsonrpc":"2.0","result":{"last_block":123},"id":1}"#;
    let response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}
