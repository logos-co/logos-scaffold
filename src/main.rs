use std::collections::BTreeMap;
use std::env;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::Duration;

type DynResult<T> = Result<T, Box<dyn std::error::Error>>;

const VERSION: &str = "0.1.0";
const DEFAULT_WALLET_BINARY: &str = "wallet";
const DEFAULT_WALLET_PASSWORD: &str = "logos-scaffold-v0";
const LSSA_URL: &str = "https://github.com/logos-blockchain/lssa.git";
const LOGOS_BLOCKCHAIN_URL: &str = "https://github.com/logos-blockchain/logos-blockchain.git";

#[derive(Clone, Debug, PartialEq, Eq)]
enum LocalnetMode {
    Docker,
    Manual,
}

#[derive(Clone, Debug)]
struct RepoRef {
    url: String,
    source: String,
    path: String,
    pin: String,
}

#[derive(Clone, Debug)]
struct Config {
    version: String,
    cache_root: String,
    lssa: RepoRef,
    logos_blockchain: RepoRef,
    preferred_mode: LocalnetMode,
    wallet_binary: String,
    wallet_home_dir: String,
}

#[derive(Clone, Debug)]
struct Project {
    root: PathBuf,
    config: Config,
}

#[derive(Clone, Debug, Default)]
struct LocalnetState {
    mode: Option<LocalnetMode>,
    pids: BTreeMap<String, u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CheckStatus {
    Pass,
    Warn,
    Fail,
}

#[derive(Clone, Debug)]
struct CheckRow {
    status: CheckStatus,
    name: String,
    detail: String,
    remediation: Option<String>,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn run() -> DynResult<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_help();
        return Ok(());
    }

    match args[1].as_str() {
        "new" => cmd_new(&args[2..]),
        "deps" => cmd_deps(&args[2..]),
        "localnet" => cmd_localnet(&args[2..]),
        "example" => cmd_example(&args[2..]),
        "doctor" => cmd_doctor(),
        "-h" | "--help" => {
            print_help();
            Ok(())
        }
        "-V" | "--version" => {
            println!("{VERSION}");
            Ok(())
        }
        other => Err(format!("unknown command: {other}").into()),
    }
}

fn print_help() {
    println!("scaffold {VERSION}");
    println!("commands:");
    println!("  scaffold new <name> [--vendor-deps] [--lssa-path PATH] [--logos-blockchain-path PATH] [--cache-root PATH]");
    println!("  scaffold deps sync [--update-pins]");
    println!("  scaffold deps build [--reset-circuits] [--yes]");
    println!("  scaffold deps circuits import <path> [--force]");
    println!("  scaffold deps circuits import --from-global [--force]");
    println!("  scaffold localnet start [--mode docker|manual]");
    println!("  scaffold localnet stop");
    println!("  scaffold localnet status");
    println!("  scaffold localnet logs [component]");
    println!("  scaffold example program-deployment run");
    println!("  scaffold doctor");
}

fn cmd_new(args: &[String]) -> DynResult<()> {
    if args.is_empty() {
        return Err("usage: scaffold new <name> ...".into());
    }

    let name = args[0].clone();
    let mut vendor_deps = false;
    let mut lssa_path: Option<PathBuf> = None;
    let mut logos_path: Option<PathBuf> = None;
    let mut cache_root_override: Option<PathBuf> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--vendor-deps" => {
                vendor_deps = true;
                i += 1;
            }
            "--lssa-path" => {
                let value = args.get(i + 1).ok_or("--lssa-path requires value")?;
                lssa_path = Some(PathBuf::from(value));
                i += 2;
            }
            "--logos-blockchain-path" => {
                let value = args
                    .get(i + 1)
                    .ok_or("--logos-blockchain-path requires value")?;
                logos_path = Some(PathBuf::from(value));
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
    let crate_name = to_cargo_crate_name(&name);
    let target = cwd.join(name);
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
    let logos_source = logos_path
        .or_else(|| infer_repo_path(&cwd, "logos-blockchain"))
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| LOGOS_BLOCKCHAIN_URL.to_string());

    let (lssa_repo_path, logos_repo_path) = if vendor_deps {
        let root = target.join(".scaffold/repos");
        fs::create_dir_all(&root)?;
        let lssa_vendor = root.join("lssa");
        let logos_vendor = root.join("logos-blockchain");

        ensure_repo_present(&lssa_vendor, &lssa_source, "lssa")?;
        ensure_repo_present(&logos_vendor, &logos_source, "logos-blockchain")?;

        (lssa_vendor, logos_vendor)
    } else {
        let lssa_cached = cache_root.join("repos/lssa");
        let logos_cached = cache_root.join("repos/logos-blockchain");

        ensure_repo_present(&lssa_cached, &lssa_source, "lssa")?;
        ensure_repo_present(&logos_cached, &logos_source, "logos-blockchain")?;

        (lssa_cached, logos_cached)
    };

    let lssa_pin = git_head_sha(&lssa_repo_path)?;
    let logos_pin = git_head_sha(&logos_repo_path)?;

    let cfg = Config {
        version: VERSION.to_string(),
        cache_root: cache_root.display().to_string(),
        lssa: RepoRef {
            url: LSSA_URL.to_string(),
            source: lssa_source,
            path: lssa_repo_path.display().to_string(),
            pin: lssa_pin,
        },
        logos_blockchain: RepoRef {
            url: LOGOS_BLOCKCHAIN_URL.to_string(),
            source: logos_source,
            path: logos_repo_path.display().to_string(),
            pin: logos_pin,
        },
        preferred_mode: LocalnetMode::Docker,
        wallet_binary: DEFAULT_WALLET_BINARY.to_string(),
        wallet_home_dir: ".scaffold/wallet".to_string(),
    };

    let template_root = lssa_repo_path.join("examples/program_deployment");
    if !template_root.exists() {
        return Err(format!(
            "template not found at {}",
            template_root.display()
        )
        .into());
    }

    copy_dir_contents(&template_root, &target)?;
    write_text(
        &target.join("Cargo.toml"),
        &render_project_template_cargo(&crate_name, &lssa_repo_path),
    )?;
    write_text(
        &target.join(".env.local"),
        "RISC0_DEV_MODE=1\nRUST_LOG=info\n",
    )?;
    write_text(&target.join(".env.devnet"), "RUST_LOG=info\n")?;
    write_text(&target.join("scaffold.toml"), &serialize_config(&cfg))?;
    write_text(
        &target.join(".scaffold/commands.md"),
        "# Command References\n\n- lssa run docs: https://github.com/logos-blockchain/lssa?tab=readme-ov-file#run-the-sequencer-and-node\n- wallet install: cargo install --path wallet --force\n",
    )?;

    println!(
        "Created scaffold project from template {} at {}",
        template_root.display(),
        target.display()
    );
    println!("Cache root: {}", cfg.cache_root);
    println!("Pinned lssa: {}", cfg.lssa.pin);
    println!("Pinned logos-blockchain: {}", cfg.logos_blockchain.pin);

    Ok(())
}

fn cmd_deps(args: &[String]) -> DynResult<()> {
    if args.is_empty() {
        return Err("usage: scaffold deps <sync|build> ...".into());
    }

    let mut project = load_project()?;
    let circuits_dir = project.root.join(".scaffold/circuits");
    match args[0].as_str() {
        "sync" => {
            let mut update_pins = false;
            for arg in &args[1..] {
                if arg == "--update-pins" {
                    update_pins = true;
                } else {
                    return Err(format!("unknown flag for deps sync: {arg}").into());
                }
            }

            sync_repo(&mut project.config.lssa, update_pins, "lssa")?;
            sync_repo(
                &mut project.config.logos_blockchain,
                update_pins,
                "logos-blockchain",
            )?;
            save_project_config(&project)?;

            println!("deps sync complete");
        }
        "build" => {
            let mut reset_circuits = false;
            let mut yes = false;
            for arg in &args[1..] {
                match arg.as_str() {
                    "--reset-circuits" => reset_circuits = true,
                    "--yes" => yes = true,
                    other => return Err(format!("unknown flag for deps build: {other}").into()),
                }
            }

            let lssa = PathBuf::from(&project.config.lssa.path);
            let logos = PathBuf::from(&project.config.logos_blockchain.path);
            ensure_dir_exists(&lssa, "lssa")?;
            ensure_dir_exists(&logos, "logos-blockchain")?;

            if reset_circuits {
                if circuits_dir.exists() {
                    if !yes {
                        return Err(format!(
                            "refusing to remove {} without --yes",
                            circuits_dir.display()
                        )
                        .into());
                    }
                    println!("$ rm -rf {}", circuits_dir.display());
                    fs::remove_dir_all(&circuits_dir)?;
                }
            }

            run_checked(
                Command::new("cargo").current_dir(&logos).arg("clean"),
                "cargo clean (logos-blockchain)",
            )?;

            if !circuits_dir.exists() {
                let global_circuits = home_dir()?.join(".logos-blockchain-circuits");
                if global_circuits.exists() {
                    copy_dir_recursive(&global_circuits, &circuits_dir)?;
                }
            }

            if !circuits_dir.exists() {
                if let Err(err) = run_checked(
                    Command::new("bash")
                        .current_dir(&logos)
                        .arg("-lc")
                        .arg(format!(
                            "./scripts/setup-logos-blockchain-circuits.sh v0.4.1 {}",
                            shell_quote(&circuits_dir.display().to_string())
                        )),
                    "setup logos-blockchain circuits",
                ) {
                    return Err(format!(
                        "failed to provision circuits automatically: {err}. Use `scaffold deps circuits import <path>` or `scaffold deps circuits import --from-global`"
                    ).into());
                }
            } else {
                println!(
                    "Using existing circuits dir {} (skip setup)",
                    circuits_dir.display()
                );
            }
            validate_circuits_dir(&circuits_dir)?;
            run_checked(
                Command::new("cargo")
                    .current_dir(&logos)
                    .env(
                        "LOGOS_BLOCKCHAIN_CIRCUITS",
                        circuits_dir.display().to_string(),
                    )
                    .arg("build")
                    .arg("--all-features"),
                "build logos-blockchain",
            )?;
            run_checked(
                Command::new("cargo")
                    .current_dir(&lssa)
                    .arg("build")
                    .arg("--release")
                    .arg("-p")
                    .arg("indexer_service"),
                "build indexer_service",
            )?;
            run_checked(
                Command::new("cargo")
                    .current_dir(&lssa)
                    .arg("build")
                    .arg("--release")
                    .arg("-p")
                    .arg("sequencer_runner"),
                "build sequencer_runner",
            )?;
            if let Err(err) = run_checked(
                Command::new("cargo")
                    .current_dir(&lssa)
                    .arg("install")
                    .arg("--path")
                    .arg("wallet")
                    .arg("--force"),
                "install wallet",
            ) {
                if which(DEFAULT_WALLET_BINARY).is_some() {
                    println!(
                        "WARN: wallet install failed but `wallet` already exists on PATH: {err}"
                    );
                } else {
                    return Err(format!(
                        "wallet install failed and wallet is not on PATH: {err}. Run `cargo install --path wallet --force` in {}",
                        lssa.display()
                    )
                    .into());
                }
            }

            println!("deps build complete");
        }
        "circuits" => {
            if args.len() < 2 {
                return Err("usage: scaffold deps circuits import <path>|--from-global [--force]".into());
            }
            match args[1].as_str() {
                "import" => {
                    let mut from_global = false;
                    let mut source_path: Option<PathBuf> = None;
                    let mut force = false;

                    let mut i = 2;
                    while i < args.len() {
                        match args[i].as_str() {
                            "--from-global" => {
                                from_global = true;
                                i += 1;
                            }
                            "--force" => {
                                force = true;
                                i += 1;
                            }
                            value => {
                                if source_path.is_none() {
                                    source_path = Some(PathBuf::from(value));
                                    i += 1;
                                } else {
                                    return Err(format!("unexpected argument: {value}").into());
                                }
                            }
                        }
                    }

                    let source = if from_global {
                        if source_path.is_some() {
                            return Err("cannot use both --from-global and explicit path".into());
                        }
                        home_dir()?.join(".logos-blockchain-circuits")
                    } else {
                        source_path.ok_or("missing source path for circuits import")?
                    };

                    if !source.exists() {
                        return Err(format!("circuits source does not exist: {}", source.display()).into());
                    }

                    if circuits_dir.exists() {
                        if !force {
                            return Err(format!(
                                "circuits target already exists at {} (use --force to replace)",
                                circuits_dir.display()
                            ).into());
                        }
                        fs::remove_dir_all(&circuits_dir)?;
                    }

                    copy_dir_recursive(&source, &circuits_dir)?;
                    validate_circuits_dir(&circuits_dir)?;
                    println!(
                        "Imported circuits from {} to {}",
                        source.display(),
                        circuits_dir.display()
                    );
                }
                other => {
                    return Err(format!("unknown deps circuits command: {other}").into());
                }
            }
        }
        other => return Err(format!("unknown deps command: {other}").into()),
    }

    Ok(())
}

fn cmd_localnet(args: &[String]) -> DynResult<()> {
    if args.is_empty() {
        return Err("usage: scaffold localnet <start|stop|status|logs> ...".into());
    }

    let project = load_project()?;
    let lssa = PathBuf::from(&project.config.lssa.path);
    let logos = PathBuf::from(&project.config.logos_blockchain.path);
    let circuits_dir = project.root.join(".scaffold/circuits");
    let state_path = project.root.join(".scaffold/state/localnet.state");
    let logs_dir = project.root.join(".scaffold/logs");
    fs::create_dir_all(&logs_dir)?;

    match args[0].as_str() {
        "start" => {
            let mut mode: Option<LocalnetMode> = None;
            let mut i = 1;
            while i < args.len() {
                if args[i] == "--mode" {
                    let value = args.get(i + 1).ok_or("--mode requires docker|manual")?;
                    mode = Some(parse_mode(value)?);
                    i += 2;
                } else {
                    return Err(format!("unknown flag for localnet start: {}", args[i]).into());
                }
            }

            let selected = match mode {
                Some(v) => v,
                None => {
                    if docker_available() {
                        LocalnetMode::Docker
                    } else {
                        LocalnetMode::Manual
                    }
                }
            };

            match selected {
                LocalnetMode::Docker => {
                    run_checked(
                        Command::new("docker")
                            .current_dir(&lssa)
                            .arg("compose")
                            .arg("up")
                            .arg("-d"),
                        "localnet docker up",
                    )?;

                    let state = LocalnetState {
                        mode: Some(LocalnetMode::Docker),
                        pids: BTreeMap::new(),
                    };
                    write_localnet_state(&state_path, &state)?;
                }
                LocalnetMode::Manual => {
                    ensure_dir_exists(&lssa, "lssa")?;
                    ensure_dir_exists(&logos, "logos-blockchain")?;

                    let node_bin = logos.join("target/debug/logos-blockchain-node");
                    if !node_bin.exists() {
                        return Err(format!(
                            "missing node binary {}; run `scaffold deps build`",
                            node_bin.display()
                        )
                        .into());
                    }

                    let node_pid = spawn_to_log(
                        Command::new(node_bin)
                            .current_dir(&logos)
                            .env(
                                "LOGOS_BLOCKCHAIN_CIRCUITS",
                                circuits_dir.display().to_string(),
                            )
                            .arg("--deployment")
                            .arg("nodes/node/standalone-deployment-config.yaml")
                            .arg("nodes/node/standalone-node-config.yaml"),
                        &logs_dir.join("node.log"),
                    )?;

                    let indexer_bin = lssa.join("target/release/indexer_service");
                    if !indexer_bin.exists() {
                        return Err(format!(
                            "missing indexer binary {}; run `scaffold deps build`",
                            indexer_bin.display()
                        )
                        .into());
                    }
                    let indexer_pid = spawn_to_log(
                        Command::new(indexer_bin)
                            .current_dir(&lssa)
                            .arg("indexer/service/configs/indexer_config.json")
                            .env("RUST_LOG", "info"),
                        &logs_dir.join("indexer.log"),
                    )?;

                    let sequencer_bin = lssa.join("target/release/sequencer_runner");
                    if !sequencer_bin.exists() {
                        return Err(format!(
                            "missing sequencer binary {}; run `scaffold deps build`",
                            sequencer_bin.display()
                        )
                        .into());
                    }
                    let sequencer_pid = spawn_to_log(
                        Command::new(sequencer_bin)
                            .current_dir(&lssa)
                            .arg("sequencer_runner/configs/debug")
                            .env("RUST_LOG", "info")
                            .env("RISC0_DEV_MODE", "1"),
                        &logs_dir.join("sequencer.log"),
                    )?;

                    let mut pids = BTreeMap::new();
                    pids.insert("node".to_string(), node_pid);
                    pids.insert("indexer".to_string(), indexer_pid);
                    pids.insert("sequencer".to_string(), sequencer_pid);

                    let state = LocalnetState {
                        mode: Some(LocalnetMode::Manual),
                        pids,
                    };
                    write_localnet_state(&state_path, &state)?;
                }
            }

            thread::sleep(Duration::from_secs(2));
            println!("localnet start requested in {} mode", mode_to_str(&selected));
        }
        "stop" => {
            let state = read_localnet_state(&state_path).unwrap_or_default();
            match state.mode {
                Some(LocalnetMode::Docker) => {
                    run_checked(
                        Command::new("docker")
                            .current_dir(&lssa)
                            .arg("compose")
                            .arg("down"),
                        "localnet docker down",
                    )?;
                }
                Some(LocalnetMode::Manual) => {
                    for (name, pid) in state.pids {
                        println!("$ kill {pid} # {name}");
                        let _ = Command::new("kill").arg(pid.to_string()).status();
                    }
                }
                None => println!("no localnet state found"),
            }

            if state_path.exists() {
                fs::remove_file(state_path)?;
            }
        }
        "status" => {
            let state = read_localnet_state(&state_path).unwrap_or_default();
            match state.mode {
                Some(LocalnetMode::Docker) => {
                    println!("mode: docker");
                    let out = run_capture(
                        Command::new("docker")
                            .current_dir(&lssa)
                            .arg("compose")
                            .arg("ps"),
                        "docker compose ps",
                    )?;
                    print!("{}", out.stdout);
                }
                Some(LocalnetMode::Manual) => {
                    println!("mode: manual");
                    for (name, pid) in state.pids {
                        println!("{name}: pid={pid} running={}", pid_alive(pid));
                    }
                }
                None => println!("mode: unknown (state missing)"),
            }

            println!("port 3040 sequencer: {}", port_open("127.0.0.1:3040"));
            println!("port 8779 indexer: {}", port_open("127.0.0.1:8779"));
            println!("port 18080 node(docker): {}", port_open("127.0.0.1:18080"));
            println!("port 8080 node(manual): {}", port_open("127.0.0.1:8080"));
        }
        "logs" => {
            let component = args.get(1).cloned();
            let state = read_localnet_state(&state_path).unwrap_or_default();
            match state.mode {
                Some(LocalnetMode::Docker) => {
                    let mut cmd = Command::new("docker");
                    cmd.current_dir(&lssa)
                        .arg("compose")
                        .arg("logs")
                        .arg("--tail")
                        .arg("200");
                    if let Some(c) = component {
                        cmd.arg(c);
                    }
                    let out = run_capture(&mut cmd, "docker compose logs")?;
                    print!("{}", out.stdout);
                }
                Some(LocalnetMode::Manual) => {
                    let selected = component.unwrap_or_else(|| "sequencer".to_string());
                    let path = logs_dir.join(format!("{selected}.log"));
                    if !path.exists() {
                        return Err(format!("missing log file: {}", path.display()).into());
                    }
                    let content = fs::read_to_string(path)?;
                    let lines: Vec<&str> = content.lines().collect();
                    let start = lines.len().saturating_sub(200);
                    for line in &lines[start..] {
                        println!("{line}");
                    }
                }
                None => println!("no localnet state found"),
            }
        }
        other => return Err(format!("unknown localnet command: {other}").into()),
    }

    Ok(())
}

fn cmd_example(args: &[String]) -> DynResult<()> {
    if args.len() != 2 || args[0] != "program-deployment" || args[1] != "run" {
        return Err("usage: scaffold example program-deployment run".into());
    }

    let project = load_project()?;
    let lssa = PathBuf::from(&project.config.lssa.path);
    let wallet_bin = project.config.wallet_binary.clone();
    let wallet_home = project.root.join(&project.config.wallet_home_dir);
    fs::create_dir_all(&wallet_home)?;

    prepare_wallet_home(&lssa, &wallet_home)?;

    run_wallet(&wallet_bin, &wallet_home, &["check-health"])?;

    run_checked(
        Command::new("cargo")
            .current_dir(&project.root)
            .arg("risczero")
            .arg("build")
            .arg("--manifest-path")
            .arg("methods/guest/Cargo.toml"),
        "build example program methods",
    )?;

    let hello_bin = project
        .root
        .join("target/riscv32im-risc0-zkvm-elf/docker/hello_world.bin");
    if !hello_bin.exists() {
        return Err(format!("missing compiled binary: {}", hello_bin.display()).into());
    }

    run_wallet(
        &wallet_bin,
        &wallet_home,
        &["deploy-program", hello_bin.to_str().ok_or("invalid hello_world path")?],
    )?;

    let output_new = run_wallet(&wallet_bin, &wallet_home, &["account", "new", "public"])?;
    let app_account = parse_public_id(&output_new.stdout)?;

    run_checked(
        Command::new("cargo")
            .current_dir(&project.root)
            .env("NSSA_WALLET_HOME_DIR", wallet_home.display().to_string())
            .arg("run")
            .arg("--bin")
            .arg("run_hello_world")
            .arg(hello_bin.to_str().ok_or("invalid hello_world path")?)
            .arg(&app_account),
        "run hello_world (first)",
    )?;

    let first_state = run_wallet(
        &wallet_bin,
        &wallet_home,
        &[
            "account",
            "get",
            "--raw",
            "--account-id",
            &format!("Public/{app_account}"),
        ],
    )?;
    if first_state.stdout.contains("Uninitialized") {
        return Err("public program run did not modify account".into());
    }

    let transfer_from = parse_public_id(
        &run_wallet(&wallet_bin, &wallet_home, &["account", "new", "public"])?
            .stdout,
    )?;
    let transfer_to = parse_public_id(
        &run_wallet(&wallet_bin, &wallet_home, &["account", "new", "public"])?
            .stdout,
    )?;

    run_wallet(
        &wallet_bin,
        &wallet_home,
        &[
            "auth-transfer",
            "init",
            "--account-id",
            &format!("Public/{transfer_from}"),
        ],
    )?;
    run_wallet(
        &wallet_bin,
        &wallet_home,
        &[
            "auth-transfer",
            "init",
            "--account-id",
            &format!("Public/{transfer_to}"),
        ],
    )?;
    run_wallet(
        &wallet_bin,
        &wallet_home,
        &[
            "auth-transfer",
            "send",
            "--from",
            &format!("Public/{transfer_from}"),
            "--to",
            &format!("Public/{transfer_to}"),
            "--amount",
            "0",
        ],
    )?;

    run_checked(
        Command::new("cargo")
            .current_dir(&project.root)
            .env("NSSA_WALLET_HOME_DIR", wallet_home.display().to_string())
            .arg("run")
            .arg("--bin")
            .arg("run_hello_world")
            .arg(hello_bin.to_str().ok_or("invalid hello_world path")?)
            .arg(&app_account),
        "run hello_world (second modify)",
    )?;

    let second_state = run_wallet(
        &wallet_bin,
        &wallet_home,
        &[
            "account",
            "get",
            "--raw",
            "--account-id",
            &format!("Public/{app_account}"),
        ],
    )?;

    if first_state.stdout.trim() == second_state.stdout.trim() {
        return Err("expected account state change after second modification run".into());
    }

    let artifact_path = project
        .root
        .join(".scaffold/state/program_deployment_success.json");
    let artifact = format!(
        "{{\n  \"example\": \"program-deployment\",\n  \"version\": \"v0.1\",\n  \"program\": \"hello_world\",\n  \"app_account\": \"{}\",\n  \"public_transfer\": {{\n    \"from\": \"{}\",\n    \"to\": \"{}\",\n    \"amount\": 0\n  }},\n  \"first_state\": {},\n  \"second_state\": {}\n}}\n",
        app_account,
        transfer_from,
        transfer_to,
        json_string(first_state.stdout.trim()),
        json_string(second_state.stdout.trim())
    );
    write_text(&artifact_path, &artifact)?;
    println!("success artifact: {}", artifact_path.display());

    Ok(())
}

fn cmd_doctor() -> DynResult<()> {
    let project = load_project()?;
    let lssa = PathBuf::from(&project.config.lssa.path);
    let logos = PathBuf::from(&project.config.logos_blockchain.path);
    let wallet_home = project.root.join(&project.config.wallet_home_dir);
    let localnet_state_path = project.root.join(".scaffold/state/localnet.state");

    let mut rows = Vec::new();

    rows.push(check_binary("git", true));
    rows.push(check_binary("rustc", true));
    rows.push(check_binary("cargo", true));
    rows.push(check_binary("docker", false));

    rows.push(check_repo("lssa", &lssa, &project.config.lssa.pin));
    rows.push(check_repo(
        "logos-blockchain",
        &logos,
        &project.config.logos_blockchain.pin,
    ));

    rows.push(check_path(
        "node binary",
        &logos.join("target/debug/logos-blockchain-node"),
        "Run `scaffold deps build`",
    ));
    rows.push(check_path(
        "indexer binary",
        &lssa.join("target/release/indexer_service"),
        "Run `scaffold deps build`",
    ));
    rows.push(check_path(
        "sequencer binary",
        &lssa.join("target/release/sequencer_runner"),
        "Run `scaffold deps build`",
    ));

    let local_circuits = project.root.join(".scaffold/circuits");
    if local_circuits.exists() {
        rows.push(CheckRow {
            status: CheckStatus::Pass,
            name: "local circuits cache".to_string(),
            detail: format!("exists at {}", local_circuits.display()),
            remediation: None,
        });
    } else {
        rows.push(CheckRow {
            status: CheckStatus::Warn,
            name: "local circuits cache".to_string(),
            detail: "missing .scaffold/circuits".to_string(),
            remediation: Some(
                "Run `scaffold deps circuits import --from-global` or `scaffold deps build`"
                    .to_string(),
            ),
        });
    }

    let global_circuits = home_dir()?.join(".logos-blockchain-circuits");
    if global_circuits.exists() {
        rows.push(CheckRow {
            status: CheckStatus::Warn,
            name: "global circuits cache".to_string(),
            detail: format!("exists at {}", global_circuits.display()),
            remediation: Some(
                "scaffold uses .scaffold/circuits; global cache is ignored unless your environment overrides it"
                    .to_string(),
            ),
        });
    }

    rows.push(check_port("sequencer port 3040", "127.0.0.1:3040"));
    rows.push(check_port("indexer port 8779", "127.0.0.1:8779"));
    rows.push(CheckRow {
        status: if port_open("127.0.0.1:18080") || port_open("127.0.0.1:8080") {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail
        },
        name: "node port 18080/8080".to_string(),
        detail: "docker node or manual node port".to_string(),
        remediation: Some("Run `scaffold localnet start`".to_string()),
    });

    if localnet_state_path.exists() {
        match read_localnet_state(&localnet_state_path) {
            Ok(state) => {
                let detail = match state.mode {
                    Some(LocalnetMode::Docker) => "state file mode=docker".to_string(),
                    Some(LocalnetMode::Manual) => {
                        let mut chunks = Vec::new();
                        for (name, pid) in state.pids {
                            chunks.push(format!("{name}:{}", if pid_alive(pid) { "up" } else { "down" }));
                        }
                        format!("state file mode=manual ({})", chunks.join(", "))
                    }
                    None => "state file present with no mode".to_string(),
                };
                rows.push(CheckRow {
                    status: CheckStatus::Pass,
                    name: "runtime state file".to_string(),
                    detail,
                    remediation: None,
                });
            }
            Err(err) => rows.push(CheckRow {
                status: CheckStatus::Warn,
                name: "runtime state file".to_string(),
                detail: err.to_string(),
                remediation: Some("Recreate state via `scaffold localnet start`".to_string()),
            }),
        }
    } else {
        rows.push(CheckRow {
            status: CheckStatus::Warn,
            name: "runtime state file".to_string(),
            detail: "missing .scaffold/state/localnet.state".to_string(),
            remediation: Some("Run `scaffold localnet start`".to_string()),
        });
    }

    rows.push(check_binary(&project.config.wallet_binary, true));

    if which(&project.config.wallet_binary).is_none() {
        rows.push(CheckRow {
            status: CheckStatus::Fail,
            name: "wallet install command".to_string(),
            detail: "wallet binary is missing".to_string(),
            remediation: Some("cargo install --path wallet --force".to_string()),
        });
    }

    let wallet_cfg = wallet_home.join("config.json");
    if wallet_cfg.exists() {
        let cfg_text = fs::read_to_string(&wallet_cfg)?;
        if cfg_text.contains("127.0.0.1:3040") || cfg_text.contains("localhost:3040") {
            rows.push(CheckRow {
                status: CheckStatus::Pass,
                name: "wallet network config".to_string(),
                detail: "wallet points to local sequencer".to_string(),
                remediation: None,
            });
        } else {
            rows.push(CheckRow {
                status: CheckStatus::Warn,
                name: "wallet network config".to_string(),
                detail: "wallet may point to non-local sequencer".to_string(),
                remediation: Some(
                    "Set .scaffold/wallet/config.json sequencer_addr=http://127.0.0.1:3040"
                        .to_string(),
                ),
            });
        }
    } else {
        rows.push(CheckRow {
            status: CheckStatus::Warn,
            name: "wallet network config".to_string(),
            detail: "missing .scaffold/wallet/config.json".to_string(),
            remediation: Some("Run `scaffold example program-deployment run` once".to_string()),
        });
    }

    if which(&project.config.wallet_binary).is_some() {
        let mut version_cmd = Command::new(&project.config.wallet_binary);
        version_cmd.arg("--version");
        match run_capture(&mut version_cmd, "wallet --version") {
            Ok(out) => rows.push(CheckRow {
                status: CheckStatus::Pass,
                name: "wallet version".to_string(),
                detail: one_line(&out.stdout),
                remediation: None,
            }),
            Err(err) => rows.push(CheckRow {
                status: CheckStatus::Warn,
                name: "wallet version".to_string(),
                detail: err.to_string(),
                remediation: Some("Ensure wallet binary is healthy".to_string()),
            }),
        }

        let mut health_cmd = Command::new(&project.config.wallet_binary);
        health_cmd
            .env("NSSA_WALLET_HOME_DIR", wallet_home.display().to_string())
            .arg("check-health")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        match run_with_stdin(health_cmd, format!("{DEFAULT_WALLET_PASSWORD}\n")) {
            Ok(out) => {
                if out.status.success() {
                    rows.push(CheckRow {
                        status: CheckStatus::Pass,
                        name: "wallet usability".to_string(),
                        detail: "wallet check-health succeeded".to_string(),
                        remediation: None,
                    });
                } else {
                    rows.push(CheckRow {
                        status: CheckStatus::Fail,
                        name: "wallet usability".to_string(),
                        detail: one_line(&out.stderr),
                        remediation: Some("Verify localnet, wallet config, and NSSA_WALLET_HOME_DIR".to_string()),
                    });
                }
            }
            Err(err) => rows.push(CheckRow {
                status: CheckStatus::Fail,
                name: "wallet usability".to_string(),
                detail: err.to_string(),
                remediation: Some("Verify wallet binary and home dir".to_string()),
            }),
        }
    }

    print_rows(&rows);

    if rows.iter().any(|r| r.status == CheckStatus::Fail) {
        return Err("doctor reported FAIL checks".into());
    }

    Ok(())
}

fn sync_repo(repo: &mut RepoRef, update_pins: bool, label: &str) -> DynResult<()> {
    let path = PathBuf::from(&repo.path);
    if !path.exists() {
        ensure_repo_present(&path, &repo.source, label)?;
    }

    if !update_pins {
        let _ = run_checked(
            Command::new("git")
                .current_dir(&path)
                .arg("fetch")
                .arg("--all")
                .arg("--tags"),
            &format!("git fetch ({label})"),
        );

        run_checked(
            Command::new("git")
                .current_dir(&path)
                .arg("checkout")
                .arg(&repo.pin),
            &format!("git checkout pin ({label})"),
        )?;

        let head = git_head_sha(&path)?;
        if head != repo.pin {
            return Err(format!(
                "{label} pin mismatch after checkout (expected {}, got {})",
                repo.pin, head
            )
            .into());
        }
    } else {
        let head = git_head_sha(&path)?;
        repo.pin = head;
    }

    Ok(())
}

fn ensure_repo_present(path: &Path, source: &str, label: &str) -> DynResult<()> {
    if path.exists() {
        if path.join(".git").exists() {
            return Ok(());
        }
        return Err(format!(
            "{} exists but is not a git repo: {}",
            label,
            path.display()
        )
        .into());
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

fn load_project() -> DynResult<Project> {
    let cwd = env::current_dir()?;
    let root = find_project_root(cwd).ok_or("no scaffold.toml in current dir or parents")?;
    let config_path = root.join("scaffold.toml");
    let cfg_text = fs::read_to_string(&config_path)?;
    let cfg = parse_config(&cfg_text)?;
    Ok(Project { root, config: cfg })
}

fn save_project_config(project: &Project) -> DynResult<()> {
    write_text(
        &project.root.join("scaffold.toml"),
        &serialize_config(&project.config),
    )
}

fn find_project_root(mut dir: PathBuf) -> Option<PathBuf> {
    loop {
        if dir.join("scaffold.toml").exists() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

fn infer_repo_path(cwd: &Path, name: &str) -> Option<PathBuf> {
    let candidates = [
        cwd.join(name),
        cwd.join("..").join(name),
        cwd.join("..").join("..").join(name),
    ];
    for candidate in candidates {
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

fn default_cache_root() -> DynResult<PathBuf> {
    let home = home_dir()?;
    if cfg!(target_os = "macos") {
        return Ok(home.join("Library/Caches/logos-scaffold"));
    }

    if cfg!(target_os = "windows") {
        if let Ok(local_app_data) = env::var("LOCALAPPDATA") {
            return Ok(PathBuf::from(local_app_data).join("logos-scaffold/Cache"));
        }
    }

    if let Ok(xdg) = env::var("XDG_CACHE_HOME") {
        return Ok(PathBuf::from(xdg).join("logos-scaffold"));
    }

    Ok(home.join(".cache/logos-scaffold"))
}

fn home_dir() -> DynResult<PathBuf> {
    if let Ok(home) = env::var("HOME") {
        return Ok(PathBuf::from(home));
    }
    Err("HOME is not set".into())
}

fn parse_config(text: &str) -> DynResult<Config> {
    let mut section = String::new();

    let mut version = String::new();
    let mut cache_root = String::new();

    let mut lssa_url = String::new();
    let mut lssa_source = String::new();
    let mut lssa_path = String::new();
    let mut lssa_pin = String::new();

    let mut logos_url = String::new();
    let mut logos_source = String::new();
    let mut logos_path = String::new();
    let mut logos_pin = String::new();

    let mut preferred = String::new();
    let mut wallet_binary = String::new();
    let mut wallet_home_dir = String::new();

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].to_string();
            continue;
        }

        let mut parts = line.splitn(2, '=');
        let key = parts.next().unwrap_or("").trim();
        let value = unquote(parts.next().unwrap_or("").trim());

        match section.as_str() {
            "scaffold" => {
                if key == "version" {
                    version = value;
                } else if key == "cache_root" {
                    cache_root = value;
                }
            }
            "repos.lssa" => {
                if key == "url" {
                    lssa_url = value;
                } else if key == "source" {
                    lssa_source = value;
                } else if key == "path" {
                    lssa_path = value;
                } else if key == "pin" {
                    lssa_pin = value;
                }
            }
            "repos.logos_blockchain" => {
                if key == "url" {
                    logos_url = value;
                } else if key == "source" {
                    logos_source = value;
                } else if key == "path" {
                    logos_path = value;
                } else if key == "pin" {
                    logos_pin = value;
                }
            }
            "runtime" => {
                if key == "preferred_mode" {
                    preferred = value;
                }
            }
            "wallet" => {
                if key == "binary" {
                    wallet_binary = value;
                } else if key == "home_dir" {
                    wallet_home_dir = value;
                }
            }
            _ => {}
        }
    }

    if version.is_empty()
        || cache_root.is_empty()
        || lssa_url.is_empty()
        || lssa_source.is_empty()
        || lssa_path.is_empty()
        || lssa_pin.is_empty()
        || logos_url.is_empty()
        || logos_source.is_empty()
        || logos_path.is_empty()
        || logos_pin.is_empty()
        || wallet_binary.is_empty()
        || wallet_home_dir.is_empty()
    {
        return Err("invalid scaffold.toml: missing required keys".into());
    }

    Ok(Config {
        version,
        cache_root,
        lssa: RepoRef {
            url: lssa_url,
            source: lssa_source,
            path: lssa_path,
            pin: lssa_pin,
        },
        logos_blockchain: RepoRef {
            url: logos_url,
            source: logos_source,
            path: logos_path,
            pin: logos_pin,
        },
        preferred_mode: match preferred.as_str() {
            "manual" => LocalnetMode::Manual,
            _ => LocalnetMode::Docker,
        },
        wallet_binary,
        wallet_home_dir,
    })
}

fn serialize_config(cfg: &Config) -> String {
    format!(
        "[scaffold]\nversion = \"{}\"\ncache_root = \"{}\"\n\n[repos.lssa]\nurl = \"{}\"\nsource = \"{}\"\npath = \"{}\"\npin = \"{}\"\n\n[repos.logos_blockchain]\nurl = \"{}\"\nsource = \"{}\"\npath = \"{}\"\npin = \"{}\"\n\n[runtime]\npreferred_mode = \"{}\"\n\n[wallet]\nbinary = \"{}\"\nhome_dir = \"{}\"\n",
        escape_toml_string(&cfg.version),
        escape_toml_string(&cfg.cache_root),
        escape_toml_string(&cfg.lssa.url),
        escape_toml_string(&cfg.lssa.source),
        escape_toml_string(&cfg.lssa.path),
        escape_toml_string(&cfg.lssa.pin),
        escape_toml_string(&cfg.logos_blockchain.url),
        escape_toml_string(&cfg.logos_blockchain.source),
        escape_toml_string(&cfg.logos_blockchain.path),
        escape_toml_string(&cfg.logos_blockchain.pin),
        mode_to_str(&cfg.preferred_mode),
        escape_toml_string(&cfg.wallet_binary),
        escape_toml_string(&cfg.wallet_home_dir),
    )
}

fn parse_mode(value: &str) -> DynResult<LocalnetMode> {
    match value {
        "docker" => Ok(LocalnetMode::Docker),
        "manual" => Ok(LocalnetMode::Manual),
        _ => Err("mode must be docker or manual".into()),
    }
}

fn mode_to_str(mode: &LocalnetMode) -> &'static str {
    match mode {
        LocalnetMode::Docker => "docker",
        LocalnetMode::Manual => "manual",
    }
}

fn unquote(value: &str) -> String {
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        value[1..value.len() - 1].to_string()
    } else {
        value.to_string()
    }
}

fn escape_toml_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn git_head_sha(repo: &Path) -> DynResult<String> {
    let out = run_capture(
        Command::new("git")
            .current_dir(repo)
            .arg("rev-parse")
            .arg("HEAD"),
        "git rev-parse HEAD",
    )?;
    Ok(out.stdout.trim().to_string())
}

fn git_clean(repo: &Path) -> DynResult<bool> {
    let out = run_capture(
        Command::new("git")
            .current_dir(repo)
            .arg("status")
            .arg("--porcelain"),
        "git status --porcelain",
    )?;
    Ok(out.stdout.trim().is_empty())
}

fn ensure_dir_exists(path: &Path, label: &str) -> DynResult<()> {
    if !path.exists() {
        return Err(format!("missing {label} at {}", path.display()).into());
    }
    Ok(())
}

fn render_command(cmd: &Command) -> String {
    let mut out = cmd.get_program().to_string_lossy().to_string();
    for arg in cmd.get_args() {
        out.push(' ');
        out.push_str(&arg.to_string_lossy());
    }
    out
}

fn run_checked(cmd: &mut Command, label: &str) -> DynResult<()> {
    println!("$ {}", render_command(cmd));
    let status = cmd.status()?;
    if !status.success() {
        return Err(format!("{label} failed with {status}").into());
    }
    Ok(())
}

struct Captured {
    status: std::process::ExitStatus,
    stdout: String,
    stderr: String,
}

fn run_capture(cmd: &mut Command, label: &str) -> DynResult<Captured> {
    println!("$ {}", render_command(cmd));
    let Output {
        status,
        stdout,
        stderr,
    } = cmd.output()?;

    let captured = Captured {
        status,
        stdout: String::from_utf8_lossy(&stdout).to_string(),
        stderr: String::from_utf8_lossy(&stderr).to_string(),
    };

    if !captured.status.success() {
        return Err(format!("{label} failed: {}", captured.stderr).into());
    }

    Ok(captured)
}

fn run_with_stdin(mut cmd: Command, input: String) -> DynResult<Captured> {
    println!("$ {}", render_command(&cmd));
    let mut child = cmd.spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input.as_bytes())?;
    }
    let out = child.wait_with_output()?;
    Ok(Captured {
        status: out.status,
        stdout: String::from_utf8_lossy(&out.stdout).to_string(),
        stderr: String::from_utf8_lossy(&out.stderr).to_string(),
    })
}

fn spawn_to_log(cmd: &mut Command, log_path: &Path) -> DynResult<u32> {
    println!("$ {}", render_command(cmd));
    let file = File::create(log_path)?;
    let err_file = file.try_clone()?;
    cmd.stdout(Stdio::from(file)).stderr(Stdio::from(err_file));
    let child = cmd.spawn()?;
    Ok(child.id())
}

fn docker_available() -> bool {
    Command::new("docker")
        .arg("compose")
        .arg("version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn pid_alive(pid: u32) -> bool {
    Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn port_open(addr: &str) -> bool {
    let parsed: SocketAddr = match addr.parse() {
        Ok(v) => v,
        Err(_) => return false,
    };
    TcpStream::connect_timeout(&parsed, Duration::from_millis(500)).is_ok()
}

fn write_text(path: &Path, text: &str) -> DynResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, text)?;
    Ok(())
}

fn write_localnet_state(path: &Path, state: &LocalnetState) -> DynResult<()> {
    let mut content = String::new();
    if let Some(mode) = &state.mode {
        content.push_str(&format!("mode={}\n", mode_to_str(mode)));
    }
    for (name, pid) in &state.pids {
        content.push_str(&format!("pid.{name}={pid}\n"));
    }
    write_text(path, &content)
}

fn read_localnet_state(path: &Path) -> DynResult<LocalnetState> {
    let mut text = String::new();
    File::open(path)?.read_to_string(&mut text)?;

    let mut state = LocalnetState::default();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(mode) = line.strip_prefix("mode=") {
            state.mode = Some(parse_mode(mode)?);
            continue;
        }
        if let Some(rest) = line.strip_prefix("pid.") {
            let mut parts = rest.splitn(2, '=');
            let name = parts.next().unwrap_or("").to_string();
            let pid: u32 = parts
                .next()
                .unwrap_or("")
                .parse()
                .map_err(|_| "invalid pid in localnet state")?;
            state.pids.insert(name, pid);
        }
    }

    Ok(state)
}

fn prepare_wallet_home(lssa_repo: &Path, wallet_home: &Path) -> DynResult<()> {
    fs::create_dir_all(wallet_home)?;
    let cfg_dst = wallet_home.join("config.json");
    if !cfg_dst.exists() {
        let cfg_src = lssa_repo.join("wallet/configs/debug/wallet_config.json");
        if !cfg_src.exists() {
            return Err("missing wallet debug config in lssa repo".into());
        }
        fs::copy(cfg_src, cfg_dst)?;
    }
    Ok(())
}

fn run_wallet(wallet_bin: &str, wallet_home: &Path, args: &[&str]) -> DynResult<Captured> {
    let mut cmd = Command::new(wallet_bin);
    cmd.env("NSSA_WALLET_HOME_DIR", wallet_home.display().to_string())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let out = run_with_stdin(cmd, format!("{DEFAULT_WALLET_PASSWORD}\n"))?;
    print!("{}", out.stdout);
    eprint!("{}", out.stderr);

    if !out.status.success() {
        return Err("wallet command failed".into());
    }

    Ok(out)
}

fn parse_public_id(output: &str) -> DynResult<String> {
    let marker = "Public/";
    let start = output
        .find(marker)
        .ok_or("could not parse Public/<account_id> from wallet output")?;
    let tail = &output[start + marker.len()..];

    let mut id = String::new();
    for ch in tail.chars() {
        if ch.is_ascii_alphanumeric() {
            id.push(ch);
        } else {
            break;
        }
    }

    if id.is_empty() {
        return Err("could not parse public account id from wallet output".into());
    }

    Ok(id)
}

fn which(binary: &str) -> Option<PathBuf> {
    let paths = env::var_os("PATH")?;
    for p in env::split_paths(&paths) {
        let candidate = p.join(binary);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

fn check_binary(binary: &str, required: bool) -> CheckRow {
    if let Some(path) = which(binary) {
        CheckRow {
            status: CheckStatus::Pass,
            name: format!("tool {binary}"),
            detail: format!("found {}", path.display()),
            remediation: None,
        }
    } else {
        CheckRow {
            status: if required {
                CheckStatus::Fail
            } else {
                CheckStatus::Warn
            },
            name: format!("tool {binary}"),
            detail: "not found on PATH".to_string(),
            remediation: Some(match binary {
                "wallet" => "Run `cargo install --path wallet --force`".to_string(),
                "docker" => "Install Docker and verify `docker compose version`".to_string(),
                _ => format!("Install `{binary}`"),
            }),
        }
    }
}

fn check_repo(name: &str, path: &Path, pin: &str) -> CheckRow {
    if !path.exists() {
        return CheckRow {
            status: CheckStatus::Fail,
            name: format!("repo {name}"),
            detail: format!("missing {}", path.display()),
            remediation: Some("Run `scaffold deps sync`".to_string()),
        };
    }

    match git_head_sha(path) {
        Ok(head) => {
            let mut status = if head == pin {
                CheckStatus::Pass
            } else {
                CheckStatus::Fail
            };

            let mut detail = format!("pin={pin}, head={head}");
            if let Ok(clean) = git_clean(path) {
                if !clean {
                    if status == CheckStatus::Pass {
                        status = CheckStatus::Warn;
                    }
                    detail.push_str("; working tree dirty");
                }
            }

            CheckRow {
                status,
                name: format!("repo {name}"),
                detail,
                remediation: if status == CheckStatus::Fail {
                    Some("Run `scaffold deps sync` or `scaffold deps sync --update-pins`".to_string())
                } else {
                    None
                },
            }
        }
        Err(err) => CheckRow {
            status: CheckStatus::Fail,
            name: format!("repo {name}"),
            detail: err.to_string(),
            remediation: Some("Ensure repo path is valid git repository".to_string()),
        },
    }
}

fn check_path(name: &str, path: &Path, remediation: &str) -> CheckRow {
    if path.exists() {
        CheckRow {
            status: CheckStatus::Pass,
            name: name.to_string(),
            detail: format!("found {}", path.display()),
            remediation: None,
        }
    } else {
        CheckRow {
            status: CheckStatus::Fail,
            name: name.to_string(),
            detail: format!("missing {}", path.display()),
            remediation: Some(remediation.to_string()),
        }
    }
}

fn check_port(name: &str, addr: &str) -> CheckRow {
    if port_open(addr) {
        CheckRow {
            status: CheckStatus::Pass,
            name: name.to_string(),
            detail: format!("{addr} reachable"),
            remediation: None,
        }
    } else {
        CheckRow {
            status: CheckStatus::Fail,
            name: name.to_string(),
            detail: format!("{addr} not reachable"),
            remediation: Some("Run `scaffold localnet start`".to_string()),
        }
    }
}

fn print_rows(rows: &[CheckRow]) {
    println!("STATUS | CHECK | DETAILS");
    println!("-------|-------|--------");

    for row in rows {
        let status = match row.status {
            CheckStatus::Pass => "PASS",
            CheckStatus::Warn => "WARN",
            CheckStatus::Fail => "FAIL",
        };
        println!("{status} | {} | {}", row.name, one_line(&row.detail));
        if row.status == CheckStatus::Fail {
            if let Some(remediation) = &row.remediation {
                println!("  remediation: {remediation}");
            }
        }
    }
}

fn one_line(text: &str) -> String {
    text.replace('\n', " ").replace('\r', " ")
}

fn json_string(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r");
    format!("\"{escaped}\"")
}

fn to_cargo_crate_name(input: &str) -> String {
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

fn render_project_template_cargo(crate_name: &str, lssa_repo: &Path) -> String {
    let lssa = lssa_repo.display().to_string();
    format!(
        "[package]\nname = \"{crate_name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\nlicense = {{ workspace = true }}\n\n[workspace.package]\nlicense = \"MIT or Apache-2.0\"\n\n[workspace]\nresolver = \"3\"\nmembers = [\n  \".\",\n  \"methods\",\n  \"methods/guest\",\n]\n\n[workspace.dependencies]\nnssa = {{ path = \"{lssa}/nssa\" }}\nnssa_core = {{ path = \"{lssa}/nssa/core\" }}\nwallet = {{ path = \"{lssa}/wallet\" }}\n\nrisc0-zkvm = {{ version = \"3.0.5\", features = [\"std\"] }}\nrisc0-build = \"3.0.5\"\n\nhex = \"0.4.3\"\nbytemuck = \"1.24.0\"\ntokio = {{ version = \"1.28.2\", features = [\"macros\", \"net\", \"rt-multi-thread\", \"sync\", \"fs\"] }}\nclap = {{ version = \"4.5.42\", features = [\"derive\", \"env\"] }}\n\n[dependencies]\nnssa.workspace = true\nnssa_core.workspace = true\nwallet.workspace = true\n\nclap.workspace = true\ntokio = {{ workspace = true, features = [\"macros\"] }}\n"
    )
}

fn copy_dir_contents(src: &Path, dst: &Path) -> DynResult<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

fn validate_circuits_dir(path: &Path) -> DynResult<()> {
    let required = [
        "poc/witness_generator",
        "poq/witness_generator",
        "pol/witness_generator",
        "zksign/witness_generator",
    ];
    for rel in required {
        let entry = path.join(rel);
        if !entry.exists() {
            return Err(format!(
                "invalid circuits directory {} (missing {})",
                path.display(),
                rel
            )
            .into());
        }
    }
    Ok(())
}

fn shell_quote(input: &str) -> String {
    let escaped = input.replace('\'', "'\"'\"'");
    format!("'{escaped}'")
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> DynResult<()> {
    if !src.exists() {
        return Err(format!("copy source does not exist: {}", src.display()).into());
    }
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}
