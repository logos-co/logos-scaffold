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
const LSSA_URL: &str = "https://github.com/logos-blockchain/lssa.git";
const DEFAULT_LSSA_PIN: &str = "dee3f7fa6f2bf63abef00828f246ddacade9cdaf";
const DEFAULT_HELLO_WORLD_IMAGE_ID_HEX: &str =
    "4880b298f59699c1e4263c5c2245c80123632d608b9116f4b253c63e6c340771";
const DEFAULT_WALLET_BINARY: &str = "wallet";
const DEFAULT_WALLET_PASSWORD: &str = "logos-scaffold-v0";

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
    sequencer_pid: Option<u32>,
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
        "create" => cmd_new(&args[2..]),
        "new" => cmd_new(&args[2..]),
        "setup" => cmd_setup(&args[2..]),
        "build" => cmd_build_shortcut(&args[2..]),
        "localnet" => cmd_localnet(&args[2..]),
        "doctor" => cmd_doctor(),
        "-h" | "--help" => {
            print_help();
            Ok(())
        }
        "-V" | "--version" => {
            println!("{VERSION}");
            Ok(())
        }
        other => {
            if let Some(suggested) = suggest_command(other) {
                Err(format!("unknown command: {other}. Did you mean `{suggested}`?").into())
            } else {
                Err(format!("unknown command: {other}").into())
            }
        }
    }
}

fn print_help() {
    println!("logos-scaffold {VERSION}");
    println!("commands:");
    println!(
        "  logos-scaffold create <name> [--vendor-deps] [--lssa-path PATH] [--cache-root PATH]"
    );
    println!("  logos-scaffold new <name> [--vendor-deps] [--lssa-path PATH] [--cache-root PATH]");
    println!("  logos-scaffold build [project-path]");
    println!("  logos-scaffold setup");
    println!("  logos-scaffold localnet start");
    println!("  logos-scaffold localnet stop");
    println!("  logos-scaffold localnet status");
    println!("  logos-scaffold localnet logs [--tail N]");
    println!("  logos-scaffold doctor");
}

fn cmd_new(args: &[String]) -> DynResult<()> {
    if args.is_empty() {
        return Err(
            "usage: logos-scaffold new <name> [--vendor-deps] [--lssa-path PATH] [--cache-root PATH]"
                .into(),
        );
    }

    let name = args[0].clone();
    let mut vendor_deps = false;
    let mut lssa_path: Option<PathBuf> = None;
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
            "--cache-root" => {
                let value = args.get(i + 1).ok_or("--cache-root requires value")?;
                cache_root_override = Some(PathBuf::from(value));
                i += 2;
            }
            other => return Err(format!("unknown flag for new: {other}").into()),
        }
    }

    let cwd = env::current_dir()?;
    let target = cwd.join(name);
    let crate_name = {
        let fallback = "app";
        let file_name = target
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(fallback);
        to_cargo_crate_name(file_name)
    };

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

    let lssa_repo_path = if vendor_deps {
        let root = target.join(".scaffold/repos");
        fs::create_dir_all(&root)?;
        let lssa_vendor = root.join("lssa");
        sync_repo_to_pin_at_path(&lssa_vendor, &lssa_source, DEFAULT_LSSA_PIN, "lssa")?;
        lssa_vendor
    } else {
        let lssa_cached = cache_root.join("repos/lssa");
        sync_repo_to_pin_at_path(&lssa_cached, &lssa_source, DEFAULT_LSSA_PIN, "lssa")?;
        lssa_cached
    };

    let cfg = Config {
        version: VERSION.to_string(),
        cache_root: cache_root.display().to_string(),
        lssa: RepoRef {
            url: LSSA_URL.to_string(),
            source: lssa_source,
            path: lssa_repo_path.display().to_string(),
            pin: DEFAULT_LSSA_PIN.to_string(),
        },
        wallet_binary: DEFAULT_WALLET_BINARY.to_string(),
        wallet_home_dir: ".scaffold/wallet".to_string(),
    };

    let template_root = lssa_repo_path.join("examples/program_deployment");
    if !template_root.exists() {
        return Err(format!("template not found at {}", template_root.display()).into());
    }

    copy_dir_contents(&template_root, &target)?;
    patch_simple_tail_call_program_id(&target)?;
    write_text(
        &target.join("Cargo.toml"),
        &render_project_template_cargo(&crate_name, &cfg.lssa.pin),
    )?;
    apply_scaffold_dx_overrides(&target)?;
    write_text(
        &target.join("README.md"),
        &render_scaffolded_project_readme(),
    )?;
    write_text(
        &target.join(".scaffold/commands.md"),
        "# Command References\n\n- standalone sequencer: RUST_LOG=info target/release/sequencer_runner sequencer_runner/configs/debug\n- lssa standalone docs: https://github.com/logos-blockchain/lssa/tree/main?tab=readme-ov-file#standalone-mode\n- wallet install: cargo install --path wallet --force\n- wallet home env: export NSSA_WALLET_HOME_DIR=$(pwd)/.scaffold/wallet\n",
    )?;
    write_text(
        &target.join(".env.local"),
        "RUST_LOG=info\nRISC0_DEV_MODE=1\n",
    )?;
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
    println!("Pinned lssa: {}", cfg.lssa.pin);

    Ok(())
}

fn cmd_setup(args: &[String]) -> DynResult<()> {
    if !args.is_empty() {
        return Err("usage: logos-scaffold setup".into());
    }

    let mut project = load_project()?;
    sync_repo_to_pin(&mut project.config.lssa, "lssa")?;

    let lssa = PathBuf::from(&project.config.lssa.path);
    ensure_dir_exists(&lssa, "lssa")?;

    run_checked(
        Command::new("cargo")
            .current_dir(&lssa)
            .arg("build")
            .arg("--release")
            .arg("--features")
            .arg("standalone")
            .arg("-p")
            .arg("sequencer_runner"),
        "build sequencer_runner (standalone)",
    )?;

    run_checked(
        Command::new("cargo")
            .current_dir(&lssa)
            .arg("install")
            .arg("--path")
            .arg("wallet")
            .arg("--force"),
        "install wallet",
    )?;

    let wallet_home = project.root.join(&project.config.wallet_home_dir);
    prepare_wallet_home(&lssa, &wallet_home)?;

    save_project_config(&project)?;
    println!("setup complete");

    Ok(())
}

fn cmd_build_shortcut(args: &[String]) -> DynResult<()> {
    let mut project_dir: Option<PathBuf> = None;

    for arg in args {
        if arg.starts_with("--") {
            return Err(format!("unknown flag for build: {arg}").into());
        }

        if project_dir.is_none() {
            project_dir = Some(PathBuf::from(arg));
        } else {
            return Err(format!(
                "unexpected argument `{}`. Usage: logos-scaffold build [project-path]",
                arg
            )
            .into());
        }
    }

    run_in_project_dir(project_dir.as_deref(), || {
        cmd_setup(&[])?;
        let cwd = env::current_dir()?;
        run_checked(
            Command::new("cargo")
                .current_dir(&cwd)
                .arg("build")
                .arg("--workspace"),
            "cargo build --workspace (project)",
        )?;
        Ok(())
    })
}

fn cmd_localnet(args: &[String]) -> DynResult<()> {
    if args.is_empty() {
        return Err("usage: logos-scaffold localnet <start|stop|status|logs> ...".into());
    }

    let project = load_project()?;
    let lssa = PathBuf::from(&project.config.lssa.path);
    let state_path = project.root.join(".scaffold/state/localnet.state");
    let logs_dir = project.root.join(".scaffold/logs");
    fs::create_dir_all(&logs_dir)?;

    match args[0].as_str() {
        "start" => {
            if args.len() != 1 {
                return Err("usage: logos-scaffold localnet start".into());
            }

            ensure_dir_exists(&lssa, "lssa")?;
            let sequencer_bin = lssa.join("target/release/sequencer_runner");
            if !sequencer_bin.exists() {
                return Err(format!(
                    "missing sequencer binary {}; run `logos-scaffold setup`",
                    sequencer_bin.display()
                )
                .into());
            }

            let state = read_localnet_state(&state_path).unwrap_or_default();
            if let Some(pid) = state.sequencer_pid {
                if pid_alive(pid) {
                    println!("sequencer already running with pid={pid}");
                    return Ok(());
                }
            }

            let sequencer_pid = spawn_to_log(
                Command::new(sequencer_bin)
                    .current_dir(&lssa)
                    .arg("sequencer_runner/configs/debug")
                    .env("RUST_LOG", "info")
                    .env("RISC0_DEV_MODE", "1"),
                &logs_dir.join("sequencer.log"),
            )?;

            let state = LocalnetState {
                sequencer_pid: Some(sequencer_pid),
            };
            write_localnet_state(&state_path, &state)?;

            thread::sleep(Duration::from_secs(2));
            println!("localnet start requested (sequencer pid={sequencer_pid})");
        }
        "stop" => {
            if args.len() != 1 {
                return Err("usage: logos-scaffold localnet stop".into());
            }

            let state = read_localnet_state(&state_path).unwrap_or_default();
            if let Some(pid) = state.sequencer_pid {
                println!("$ kill {pid} # sequencer");
                let _ = Command::new("kill").arg(pid.to_string()).status();
            } else {
                println!("no localnet state found");
            }

            if state_path.exists() {
                fs::remove_file(state_path)?;
            }
        }
        "status" => {
            if args.len() != 1 {
                return Err("usage: logos-scaffold localnet status".into());
            }

            let state = read_localnet_state(&state_path).unwrap_or_default();
            if let Some(pid) = state.sequencer_pid {
                println!("sequencer: pid={pid} running={}", pid_alive(pid));
            } else {
                println!("sequencer: not tracked (state missing)");
            }
            println!("port 3040 sequencer: {}", port_open("127.0.0.1:3040"));
        }
        "logs" => {
            let mut tail: usize = 200;
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--tail" => {
                        let value = args.get(i + 1).ok_or("--tail requires value")?;
                        tail = value
                            .parse::<usize>()
                            .map_err(|_| "--tail expects positive integer")?;
                        i += 2;
                    }
                    other => return Err(format!("unknown flag for localnet logs: {other}").into()),
                }
            }

            let log_path = logs_dir.join("sequencer.log");
            if !log_path.exists() {
                return Err(format!("missing log file: {}", log_path.display()).into());
            }

            let content = fs::read_to_string(log_path)?;
            let lines: Vec<&str> = content.lines().collect();
            let start = lines.len().saturating_sub(tail);
            for line in &lines[start..] {
                println!("{line}");
            }
        }
        other => return Err(format!("unknown localnet command: {other}").into()),
    }

    Ok(())
}

fn cmd_doctor() -> DynResult<()> {
    let project = load_project()?;
    let lssa = PathBuf::from(&project.config.lssa.path);
    let wallet_home = project.root.join(&project.config.wallet_home_dir);
    let localnet_state_path = project.root.join(".scaffold/state/localnet.state");

    let mut rows = Vec::new();

    rows.push(check_binary("git", true));
    rows.push(check_binary("rustc", true));
    rows.push(check_binary("cargo", true));
    rows.push(check_binary(&project.config.wallet_binary, true));

    rows.push(check_repo("lssa", &lssa, &project.config.lssa.pin));

    rows.push(CheckRow {
        status: if project.config.lssa.pin == DEFAULT_LSSA_PIN {
            CheckStatus::Pass
        } else {
            CheckStatus::Warn
        },
        name: "lssa standalone pin".to_string(),
        detail: format!(
            "configured pin={} expected={}",
            project.config.lssa.pin, DEFAULT_LSSA_PIN
        ),
        remediation: if project.config.lssa.pin == DEFAULT_LSSA_PIN {
            None
        } else {
            Some(format!(
                "Set repos.lssa.pin in scaffold.toml to {}",
                DEFAULT_LSSA_PIN
            ))
        },
    });

    rows.push(check_standalone_support(&lssa));

    rows.push(check_path(
        "sequencer binary",
        &lssa.join("target/release/sequencer_runner"),
        "Run `logos-scaffold setup`",
    ));

    rows.push(check_port_warn(
        "sequencer port 3040",
        "127.0.0.1:3040",
        "Run `logos-scaffold localnet start`",
    ));

    if localnet_state_path.exists() {
        match read_localnet_state(&localnet_state_path) {
            Ok(state) => {
                let detail = match state.sequencer_pid {
                    Some(pid) => format!("sequencer pid={pid} running={}", pid_alive(pid)),
                    None => "state file present but sequencer pid missing".to_string(),
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
                remediation: Some("Recreate state via `logos-scaffold localnet start`".to_string()),
            }),
        }
    } else {
        rows.push(CheckRow {
            status: CheckStatus::Warn,
            name: "runtime state file".to_string(),
            detail: "missing .scaffold/state/localnet.state".to_string(),
            remediation: Some("Run `logos-scaffold localnet start`".to_string()),
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
            remediation: Some("Run `logos-scaffold setup`".to_string()),
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
                        remediation: Some(
                            "Verify localnet, wallet config, and NSSA_WALLET_HOME_DIR".to_string(),
                        ),
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

fn sync_repo_to_pin(repo: &mut RepoRef, label: &str) -> DynResult<()> {
    let path = PathBuf::from(&repo.path);
    sync_repo_to_pin_at_path(&path, &repo.source, &repo.pin, label)?;
    repo.pin = git_head_sha(&path)?;
    Ok(())
}

fn sync_repo_to_pin_at_path(path: &Path, source: &str, pin: &str, label: &str) -> DynResult<()> {
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
        return Err(format!(
            "{label} pin mismatch after checkout (expected {}, got {})",
            pin, head
        )
        .into());
    }

    Ok(())
}

fn ensure_pin_exists(path: &Path, pin: &str, label: &str) -> DynResult<()> {
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
        return Err(format!(
            "configured {label} pin {pin} is not available in {}. Ensure the repo source contains this commit (try `--lssa-path` pointing to a repo that has it).",
            path.display()
        )
        .into());
    }

    Ok(())
}

fn ensure_repo_present(path: &Path, source: &str, label: &str) -> DynResult<()> {
    if path.exists() {
        if path.join(".git").exists() {
            return Ok(());
        }
        return Err(format!("{} exists but is not a git repo: {}", label, path.display()).into());
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
    let root = find_project_root(cwd.clone()).ok_or_else(|| {
        format!(
            "Not a logos-scaffold project at {}. Run `logos-scaffold create <name>` (or `logos-scaffold new <name>`) first.",
            cwd.display()
        )
    })?;

    let config_path = root.join("scaffold.toml");
    let cfg_text = fs::read_to_string(&config_path)?;
    let cfg = parse_config(&cfg_text)?;
    Ok(Project { root, config: cfg })
}

fn run_in_project_dir(path: Option<&Path>, op: impl FnOnce() -> DynResult<()>) -> DynResult<()> {
    let original = env::current_dir()?;
    if let Some(path) = path {
        env::set_current_dir(path)?;
    }
    let result = op();
    let _ = env::set_current_dir(original);
    result
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

    if version.is_empty() || cache_root.is_empty() {
        return Err("invalid scaffold.toml: missing [scaffold] keys".into());
    }

    if lssa_url.is_empty() {
        lssa_url = LSSA_URL.to_string();
    }

    if lssa_source.is_empty() || lssa_path.is_empty() || lssa_pin.is_empty() {
        return Err("invalid scaffold.toml: missing required repos.lssa keys".into());
    }

    if wallet_binary.is_empty() {
        wallet_binary = DEFAULT_WALLET_BINARY.to_string();
    }
    if wallet_home_dir.is_empty() {
        wallet_home_dir = ".scaffold/wallet".to_string();
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
        wallet_binary,
        wallet_home_dir,
    })
}

fn serialize_config(cfg: &Config) -> String {
    format!(
        "[scaffold]\nversion = \"{}\"\ncache_root = \"{}\"\n\n[repos.lssa]\nurl = \"{}\"\nsource = \"{}\"\npath = \"{}\"\npin = \"{}\"\n\n[wallet]\nbinary = \"{}\"\nhome_dir = \"{}\"\n",
        escape_toml_string(&cfg.version),
        escape_toml_string(&cfg.cache_root),
        escape_toml_string(&cfg.lssa.url),
        escape_toml_string(&cfg.lssa.source),
        escape_toml_string(&cfg.lssa.path),
        escape_toml_string(&cfg.lssa.pin),
        escape_toml_string(&cfg.wallet_binary),
        escape_toml_string(&cfg.wallet_home_dir),
    )
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
    if let Some(pid) = state.sequencer_pid {
        content.push_str(&format!("sequencer_pid={pid}\n"));
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

        if let Some(rest) = line.strip_prefix("sequencer_pid=") {
            let pid: u32 = rest.parse().map_err(|_| "invalid sequencer pid")?;
            state.sequencer_pid = Some(pid);
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
            remediation: Some("Run `logos-scaffold setup`".to_string()),
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
                    Some("Run `logos-scaffold setup`".to_string())
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

fn check_port_warn(name: &str, addr: &str, remediation: &str) -> CheckRow {
    if port_open(addr) {
        CheckRow {
            status: CheckStatus::Pass,
            name: name.to_string(),
            detail: format!("{addr} reachable"),
            remediation: None,
        }
    } else {
        CheckRow {
            status: CheckStatus::Warn,
            name: name.to_string(),
            detail: format!("{addr} not reachable"),
            remediation: Some(remediation.to_string()),
        }
    }
}

fn check_standalone_support(lssa_path: &Path) -> CheckRow {
    let files = [
        lssa_path.join("Cargo.toml"),
        lssa_path.join("sequencer_runner/Cargo.toml"),
        lssa_path.join("README.md"),
    ];

    for path in files {
        if let Ok(text) = fs::read_to_string(path) {
            if text.contains("standalone") {
                return CheckRow {
                    status: CheckStatus::Pass,
                    name: "standalone support marker".to_string(),
                    detail: "found `standalone` marker in lssa repository".to_string(),
                    remediation: None,
                };
            }
        }
    }

    CheckRow {
        status: CheckStatus::Fail,
        name: "standalone support marker".to_string(),
        detail: "could not find `standalone` marker in lssa repo".to_string(),
        remediation: Some(format!(
            "Use an lssa source that contains standalone mode and pin {}",
            DEFAULT_LSSA_PIN
        )),
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

fn suggest_command(cmd: &str) -> Option<&'static str> {
    let known = [
        "create", "new", "build", "setup", "localnet", "doctor", "help",
    ];
    let mut best: Option<(&str, usize)> = None;
    for candidate in known {
        let dist = edit_distance(cmd, candidate);
        match best {
            Some((_, best_dist)) if dist >= best_dist => {}
            _ => best = Some((candidate, dist)),
        }
    }
    match best {
        Some((candidate, dist)) if dist <= 3 => Some(candidate),
        _ => None,
    }
}

fn edit_distance(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let mut dp = vec![vec![0usize; b_chars.len() + 1]; a_chars.len() + 1];
    for (i, row) in dp.iter_mut().enumerate().take(a_chars.len() + 1) {
        row[0] = i;
    }
    for j in 0..=b_chars.len() {
        dp[0][j] = j;
    }
    for i in 1..=a_chars.len() {
        for j in 1..=b_chars.len() {
            let cost = if a_chars[i - 1] == b_chars[j - 1] {
                0
            } else {
                1
            };
            let del = dp[i - 1][j] + 1;
            let ins = dp[i][j - 1] + 1;
            let sub = dp[i - 1][j - 1] + cost;
            dp[i][j] = del.min(ins).min(sub);
        }
    }
    dp[a_chars.len()][b_chars.len()]
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

fn render_project_template_cargo(crate_name: &str, lssa_pin: &str) -> String {
    format!(
        "[package]\nname = \"{crate_name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\nlicense = {{ workspace = true }}\n\n[workspace.package]\nlicense = \"MIT or Apache-2.0\"\n\n[workspace]\nresolver = \"3\"\nmembers = [\n  \".\",\n  \"methods\",\n  \"methods/guest\",\n]\n\n[workspace.dependencies]\nnssa = {{ git = \"https://github.com/logos-blockchain/lssa.git\", rev = \"{lssa_pin}\" }}\nnssa_core = {{ git = \"https://github.com/logos-blockchain/lssa.git\", rev = \"{lssa_pin}\" }}\nwallet = {{ git = \"https://github.com/logos-blockchain/lssa.git\", rev = \"{lssa_pin}\" }}\n\nrisc0-zkvm = {{ version = \"3.0.5\", features = [\"std\"] }}\nrisc0-build = \"3.0.5\"\n\nhex = \"0.4.3\"\nbytemuck = \"1.24.0\"\ntokio = {{ version = \"1.28.2\", features = [\"macros\", \"net\", \"rt-multi-thread\", \"sync\", \"fs\"] }}\nclap = {{ version = \"4.5.42\", features = [\"derive\", \"env\"] }}\n\n[dependencies]\nnssa.workspace = true\nnssa_core.workspace = true\nwallet.workspace = true\nexample_program_deployment_methods = {{ path = \"methods\" }}\n\nclap.workspace = true\ntokio = {{ workspace = true, features = [\"macros\"] }}\n"
    )
}

fn render_scaffolded_project_readme() -> String {
    r#"# Program Deployment Scaffold

This project was generated by `logos-scaffold` for LSSA standalone mode only.

## Prerequisites

- `git`, `rustc`, `cargo`
- `wallet` binary (installed by `logos-scaffold setup`)
- Docker running (required by guest method builds)

## First Run

```bash
logos-scaffold setup
logos-scaffold localnet start
export NSSA_WALLET_HOME_DIR=$(pwd)/.scaffold/wallet
logos-scaffold build
export EXAMPLE_PROGRAMS_BUILD_DIR=$(pwd)/target/riscv-guest/example_program_deployment_methods/example_program_deployment_programs/riscv32im-risc0-zkvm-elf/release
wallet check-health
```

## Deploy Guest Programs

```bash
wallet deploy-program "$EXAMPLE_PROGRAMS_BUILD_DIR/hello_world.bin"
wallet deploy-program "$EXAMPLE_PROGRAMS_BUILD_DIR/hello_world_with_authorization.bin"
wallet deploy-program "$EXAMPLE_PROGRAMS_BUILD_DIR/hello_world_with_move_function.bin"
wallet deploy-program "$EXAMPLE_PROGRAMS_BUILD_DIR/simple_tail_call.bin"
wallet deploy-program "$EXAMPLE_PROGRAMS_BUILD_DIR/tail_call_with_pda.bin"
```

## Create Accounts

Create fresh accounts and assign their IDs to shell vars:

```bash
# copy IDs from wallet output
PUBLIC_HELLO_ACCOUNT_ID="<public-account-id>"
PRIVATE_HELLO_ACCOUNT_ID="<private-account-id>"
PUBLIC_AUTH_ACCOUNT_ID="<public-account-id>"
PUBLIC_MOVE_ACCOUNT_ID="<public-account-id>"
PRIVATE_MOVE_ACCOUNT_ID="<private-account-id>"
```

Use `wallet account new public` and `wallet account new private` enough times to fill those values.

## Run All Example Binaries

```bash
cargo run --bin run_hello_world -- $PUBLIC_HELLO_ACCOUNT_ID

cargo run --bin run_hello_world_private -- $PRIVATE_HELLO_ACCOUNT_ID
wallet account sync-private

cargo run --bin run_hello_world_with_authorization -- $PUBLIC_AUTH_ACCOUNT_ID

cargo run --bin run_hello_world_with_move_function -- \
  write-public \
  $PUBLIC_MOVE_ACCOUNT_ID \
  "hello-from-public"

cargo run --bin run_hello_world_with_move_function -- \
  write-private \
  $PRIVATE_MOVE_ACCOUNT_ID \
  "hello-from-private"
wallet account sync-private

cargo run --bin run_hello_world_with_move_function -- \
  move-data-public-to-private \
  $PUBLIC_MOVE_ACCOUNT_ID \
  $PRIVATE_MOVE_ACCOUNT_ID
wallet account sync-private

cargo run --bin run_hello_world_through_tail_call -- $PUBLIC_HELLO_ACCOUNT_ID

cargo run --bin run_hello_world_through_tail_call_private -- $PRIVATE_MOVE_ACCOUNT_ID
wallet account sync-private

cargo run --bin run_hello_world_with_authorization_through_tail_call_with_pda
```

## Optional Program Path Overrides

For testing custom binaries, pass explicit paths:

```bash
cargo run --bin run_hello_world -- --program-path "$EXAMPLE_PROGRAMS_BUILD_DIR/hello_world.bin" $PUBLIC_HELLO_ACCOUNT_ID
cargo run --bin run_hello_world_through_tail_call_private -- --simple-tail-call-path "$EXAMPLE_PROGRAMS_BUILD_DIR/simple_tail_call.bin" --hello-world-path "$EXAMPLE_PROGRAMS_BUILD_DIR/hello_world.bin" $PRIVATE_MOVE_ACCOUNT_ID
```

## Main Scaffold Commands

```bash
logos-scaffold setup
logos-scaffold build [project-path]
logos-scaffold localnet start
logos-scaffold localnet status
logos-scaffold localnet logs --tail 200
logos-scaffold localnet stop
logos-scaffold doctor
```

## Notes

- Standalone-only: no `logos-blockchain` dependency and no `deps` or `example` CLI groups.
- Use `export NSSA_WALLET_HOME_DIR=$(pwd)/.scaffold/wallet` before wallet commands.
- `logos-scaffold build` runs `setup` and then `cargo build --workspace`.
- LSSA pin is enforced by `logos-scaffold setup`.
- `simple_tail_call` hardcodes `HELLO_WORLD_PROGRAM_ID_HEX`. If tail-call runs fail, set it to the `hello_world.bin` ImageID from the latest `cargo risczero build` output, then rebuild methods.
"#
    .to_string()
}

fn apply_scaffold_dx_overrides(target: &Path) -> DynResult<()> {
    fs::create_dir_all(target.join("src/bin"))?;

    write_text(
        &target.join(".gitignore"),
        ".scaffold/\ntarget/\nCargo.lock.bak\n.env.local\n",
    )?;

    write_text(&target.join("src/lib.rs"), &render_runner_support_lib())?;
    write_text(
        &target.join("src/bin/run_hello_world.rs"),
        &render_runner_run_hello_world(),
    )?;
    write_text(
        &target.join("src/bin/run_hello_world_private.rs"),
        &render_runner_run_hello_world_private(),
    )?;
    write_text(
        &target.join("src/bin/run_hello_world_with_authorization.rs"),
        &render_runner_run_hello_world_with_authorization(),
    )?;
    write_text(
        &target.join("src/bin/run_hello_world_through_tail_call.rs"),
        &render_runner_run_hello_world_through_tail_call(),
    )?;
    write_text(
        &target.join("src/bin/run_hello_world_through_tail_call_private.rs"),
        &render_runner_run_hello_world_through_tail_call_private(),
    )?;
    write_text(
        &target.join("src/bin/run_hello_world_with_authorization_through_tail_call_with_pda.rs"),
        &render_runner_run_hello_world_with_authorization_through_tail_call_with_pda(),
    )?;
    write_text(
        &target.join("src/bin/run_hello_world_with_move_function.rs"),
        &render_runner_run_hello_world_with_move_function(),
    )?;

    Ok(())
}

fn render_runner_support_lib() -> String {
    r#"#[allow(dead_code)]
pub mod runner_support {
    use nssa::{AccountId, program::Program};

    pub fn parse_account_id(raw: &str) -> AccountId {
        let normalized = raw
            .strip_prefix("Public/")
            .or_else(|| raw.strip_prefix("Private/"))
            .unwrap_or(raw);

        normalized
            .parse()
            .unwrap_or_else(|err| panic!("invalid account_id `{raw}`: {err}"))
    }

    pub fn load_program(program_path: Option<&str>, embedded_elf: &[u8], label: &str) -> Program {
        let bytes = if let Some(path) = program_path {
            std::fs::read(path)
                .unwrap_or_else(|err| panic!("failed to read {label} binary at `{path}`: {err}"))
        } else {
            embedded_elf.to_vec()
        };

        Program::new(bytes).unwrap_or_else(|err| panic!("failed to parse {label} program: {err}"))
    }
}
"#
    .to_string()
}

fn render_runner_run_hello_world() -> String {
    r#"use clap::Parser;
use example_program_deployment_methods::HELLO_WORLD_ELF;
use nssa::{
    PublicTransaction,
    public_transaction::{Message, WitnessSet},
};
use wallet::WalletCore;

#[path = "../lib.rs"]
mod scaffold_lib;
use scaffold_lib::runner_support::{load_program, parse_account_id};

#[derive(Parser, Debug)]
struct Cli {
    #[arg(long)]
    program_path: Option<String>,
    account_id: String,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let wallet_core = WalletCore::from_env().unwrap();

    let program = load_program(cli.program_path.as_deref(), HELLO_WORLD_ELF, "hello_world");
    let account_id = parse_account_id(&cli.account_id);

    let greeting: Vec<u8> = vec![72, 111, 108, 97, 32, 109, 117, 110, 100, 111, 33];
    let message = Message::try_new(program.id(), vec![account_id], vec![], greeting).unwrap();
    let witness_set = WitnessSet::for_message(&message, &[]);
    let tx = PublicTransaction::new(message, witness_set);

    let _response = wallet_core.sequencer_client.send_tx_public(tx).await.unwrap();
}
"#
    .to_string()
}

fn render_runner_run_hello_world_private() -> String {
    r#"use clap::Parser;
use example_program_deployment_methods::HELLO_WORLD_ELF;
use nssa::program::Program;
use wallet::{PrivacyPreservingAccount, WalletCore};

#[path = "../lib.rs"]
mod scaffold_lib;
use scaffold_lib::runner_support::{load_program, parse_account_id};

#[derive(Parser, Debug)]
struct Cli {
    #[arg(long)]
    program_path: Option<String>,
    account_id: String,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let wallet_core = WalletCore::from_env().unwrap();

    let program = load_program(cli.program_path.as_deref(), HELLO_WORLD_ELF, "hello_world");
    let account_id = parse_account_id(&cli.account_id);

    let greeting: Vec<u8> = vec![72, 111, 108, 97, 32, 109, 117, 110, 100, 111, 33];
    let accounts = vec![PrivacyPreservingAccount::PrivateOwned(account_id)];

    wallet_core
        .send_privacy_preserving_tx(
            accounts,
            Program::serialize_instruction(greeting).unwrap(),
            &program.into(),
        )
        .await
        .unwrap();
}
"#
    .to_string()
}

fn render_runner_run_hello_world_with_authorization() -> String {
    r#"use clap::Parser;
use example_program_deployment_methods::HELLO_WORLD_WITH_AUTHORIZATION_ELF;
use nssa::{
    PublicTransaction,
    public_transaction::{Message, WitnessSet},
};
use wallet::WalletCore;

#[path = "../lib.rs"]
mod scaffold_lib;
use scaffold_lib::runner_support::{load_program, parse_account_id};

#[derive(Parser, Debug)]
struct Cli {
    #[arg(long)]
    program_path: Option<String>,
    account_id: String,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let wallet_core = WalletCore::from_env().unwrap();

    let program = load_program(
        cli.program_path.as_deref(),
        HELLO_WORLD_WITH_AUTHORIZATION_ELF,
        "hello_world_with_authorization",
    );
    let account_id = parse_account_id(&cli.account_id);

    let signing_key = wallet_core
        .storage()
        .user_data
        .get_pub_account_signing_key(account_id)
        .expect("Input account should be a self owned public account");

    let greeting: Vec<u8> = vec![72, 111, 108, 97, 32, 109, 117, 110, 100, 111, 33];
    let nonces = wallet_core
        .get_accounts_nonces(vec![account_id])
        .await
        .expect("Node should be reachable to query account data");
    let message = Message::try_new(program.id(), vec![account_id], nonces, greeting).unwrap();
    let witness_set = WitnessSet::for_message(&message, &[signing_key]);
    let tx = PublicTransaction::new(message, witness_set);

    let _response = wallet_core.sequencer_client.send_tx_public(tx).await.unwrap();
}
"#
    .to_string()
}

fn render_runner_run_hello_world_through_tail_call() -> String {
    r#"use clap::Parser;
use example_program_deployment_methods::SIMPLE_TAIL_CALL_ELF;
use nssa::{
    PublicTransaction,
    public_transaction::{Message, WitnessSet},
};
use wallet::WalletCore;

#[path = "../lib.rs"]
mod scaffold_lib;
use scaffold_lib::runner_support::{load_program, parse_account_id};

#[derive(Parser, Debug)]
struct Cli {
    #[arg(long)]
    program_path: Option<String>,
    account_id: String,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let wallet_core = WalletCore::from_env().unwrap();

    let program = load_program(
        cli.program_path.as_deref(),
        SIMPLE_TAIL_CALL_ELF,
        "simple_tail_call",
    );
    let account_id = parse_account_id(&cli.account_id);

    let message = Message::try_new(program.id(), vec![account_id], vec![], ()).unwrap();
    let witness_set = WitnessSet::for_message(&message, &[]);
    let tx = PublicTransaction::new(message, witness_set);

    let _response = wallet_core.sequencer_client.send_tx_public(tx).await.unwrap();
}
"#
    .to_string()
}

fn render_runner_run_hello_world_through_tail_call_private() -> String {
    r#"use std::collections::HashMap;

use clap::Parser;
use example_program_deployment_methods::{HELLO_WORLD_ELF, SIMPLE_TAIL_CALL_ELF};
use nssa::{
    ProgramId, privacy_preserving_transaction::circuit::ProgramWithDependencies, program::Program,
};
use wallet::{PrivacyPreservingAccount, WalletCore};

#[path = "../lib.rs"]
mod scaffold_lib;
use scaffold_lib::runner_support::{load_program, parse_account_id};

#[derive(Parser, Debug)]
struct Cli {
    #[arg(long)]
    simple_tail_call_path: Option<String>,
    #[arg(long)]
    hello_world_path: Option<String>,
    account_id: String,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let wallet_core = WalletCore::from_env().unwrap();

    let simple_tail_call = load_program(
        cli.simple_tail_call_path.as_deref(),
        SIMPLE_TAIL_CALL_ELF,
        "simple_tail_call",
    );
    let hello_world = load_program(
        cli.hello_world_path.as_deref(),
        HELLO_WORLD_ELF,
        "hello_world",
    );

    let dependencies: HashMap<ProgramId, Program> =
        [(hello_world.id(), hello_world)].into_iter().collect();
    let program_with_dependencies = ProgramWithDependencies::new(simple_tail_call, dependencies);
    let account_id = parse_account_id(&cli.account_id);
    let accounts = vec![PrivacyPreservingAccount::PrivateOwned(account_id)];

    wallet_core
        .send_privacy_preserving_tx(
            accounts,
            Program::serialize_instruction(()).unwrap(),
            &program_with_dependencies,
        )
        .await
        .unwrap();
}
"#
    .to_string()
}

fn render_runner_run_hello_world_with_authorization_through_tail_call_with_pda() -> String {
    r#"use clap::Parser;
use example_program_deployment_methods::TAIL_CALL_WITH_PDA_ELF;
use nssa::{
    AccountId, PublicTransaction,
    public_transaction::{Message, WitnessSet},
};
use nssa_core::program::PdaSeed;
use wallet::WalletCore;

#[path = "../lib.rs"]
mod scaffold_lib;
use scaffold_lib::runner_support::load_program;

const PDA_SEED: PdaSeed = PdaSeed::new([37; 32]);

#[derive(Parser, Debug)]
struct Cli {
    #[arg(long)]
    program_path: Option<String>,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let wallet_core = WalletCore::from_env().unwrap();

    let program = load_program(
        cli.program_path.as_deref(),
        TAIL_CALL_WITH_PDA_ELF,
        "tail_call_with_pda",
    );

    let pda = AccountId::from((&program.id(), &PDA_SEED));
    let message = Message::try_new(program.id(), vec![pda], vec![], ()).unwrap();
    let witness_set = WitnessSet::for_message(&message, &[]);
    let tx = PublicTransaction::new(message, witness_set);

    let _response = wallet_core.sequencer_client.send_tx_public(tx).await.unwrap();
    println!("The program derived account id is: {pda}");
}
"#
    .to_string()
}

fn render_runner_run_hello_world_with_move_function() -> String {
    r#"use clap::{Parser, Subcommand};
use example_program_deployment_methods::HELLO_WORLD_WITH_MOVE_FUNCTION_ELF;
use nssa::{PublicTransaction, program::Program, public_transaction};
use wallet::{PrivacyPreservingAccount, WalletCore};

#[path = "../lib.rs"]
mod scaffold_lib;
use scaffold_lib::runner_support::{load_program, parse_account_id};

type Instruction = (u8, Vec<u8>);
const WRITE_FUNCTION_ID: u8 = 0;
const MOVE_DATA_FUNCTION_ID: u8 = 1;

#[derive(Parser, Debug)]
struct Cli {
    #[arg(long)]
    program_path: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    WritePublic {
        account_id: String,
        greeting: String,
    },
    WritePrivate {
        account_id: String,
        greeting: String,
    },
    MoveDataPublicToPublic {
        from: String,
        to: String,
    },
    MoveDataPublicToPrivate {
        from: String,
        to: String,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let program = load_program(
        cli.program_path.as_deref(),
        HELLO_WORLD_WITH_MOVE_FUNCTION_ELF,
        "hello_world_with_move_function",
    );
    let wallet_core = WalletCore::from_env().unwrap();

    match cli.command {
        Command::WritePublic {
            account_id,
            greeting,
        } => {
            let instruction: Instruction = (WRITE_FUNCTION_ID, greeting.into_bytes());
            let account_id = parse_account_id(&account_id);
            let message = public_transaction::Message::try_new(
                program.id(),
                vec![account_id],
                vec![],
                instruction,
            )
            .unwrap();
            let witness_set = public_transaction::WitnessSet::for_message(&message, &[]);
            let tx = PublicTransaction::new(message, witness_set);
            let _response = wallet_core
                .sequencer_client
                .send_tx_public(tx)
                .await
                .unwrap();
        }
        Command::WritePrivate {
            account_id,
            greeting,
        } => {
            let instruction: Instruction = (WRITE_FUNCTION_ID, greeting.into_bytes());
            let account_id = parse_account_id(&account_id);
            let accounts = vec![PrivacyPreservingAccount::PrivateOwned(account_id)];
            wallet_core
                .send_privacy_preserving_tx(
                    accounts,
                    Program::serialize_instruction(instruction).unwrap(),
                    &program.into(),
                )
                .await
                .unwrap();
        }
        Command::MoveDataPublicToPublic { from, to } => {
            let instruction: Instruction = (MOVE_DATA_FUNCTION_ID, vec![]);
            let from = parse_account_id(&from);
            let to = parse_account_id(&to);
            let message = public_transaction::Message::try_new(
                program.id(),
                vec![from, to],
                vec![],
                instruction,
            )
            .unwrap();
            let witness_set = public_transaction::WitnessSet::for_message(&message, &[]);
            let tx = PublicTransaction::new(message, witness_set);
            let _response = wallet_core
                .sequencer_client
                .send_tx_public(tx)
                .await
                .unwrap();
        }
        Command::MoveDataPublicToPrivate { from, to } => {
            let instruction: Instruction = (MOVE_DATA_FUNCTION_ID, vec![]);
            let from = parse_account_id(&from);
            let to = parse_account_id(&to);
            let accounts = vec![
                PrivacyPreservingAccount::Public(from),
                PrivacyPreservingAccount::PrivateOwned(to),
            ];
            wallet_core
                .send_privacy_preserving_tx(
                    accounts,
                    Program::serialize_instruction(instruction).unwrap(),
                    &program.into(),
                )
                .await
                .unwrap();
        }
    };
}
"#
    .to_string()
}

fn patch_simple_tail_call_program_id(project_root: &Path) -> DynResult<()> {
    let path = project_root.join("methods/guest/src/bin/simple_tail_call.rs");
    if !path.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(&path)?;
    let marker = "const HELLO_WORLD_PROGRAM_ID_HEX: &str =";
    let marker_pos = match content.find(marker) {
        Some(pos) => pos,
        None => return Ok(()),
    };

    let from_marker = &content[marker_pos..];
    let open_quote_rel = from_marker
        .find('"')
        .ok_or("failed to locate opening quote for HELLO_WORLD_PROGRAM_ID_HEX")?;
    let open_quote = marker_pos + open_quote_rel + 1;

    let after_open = &content[open_quote..];
    let close_quote_rel = after_open
        .find('"')
        .ok_or("failed to locate closing quote for HELLO_WORLD_PROGRAM_ID_HEX")?;
    let close_quote = open_quote + close_quote_rel;

    if &content[open_quote..close_quote] == DEFAULT_HELLO_WORLD_IMAGE_ID_HEX {
        return Ok(());
    }

    let mut patched = String::with_capacity(content.len());
    patched.push_str(&content[..open_quote]);
    patched.push_str(DEFAULT_HELLO_WORLD_IMAGE_ID_HEX);
    patched.push_str(&content[close_quote..]);

    write_text(&path, &patched)?;
    Ok(())
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
