use std::path::PathBuf;

use anyhow::anyhow;
use clap::{CommandFactory, Parser, Subcommand};

use crate::commands::build::cmd_build_shortcut;
use crate::commands::client::cmd_client;
use crate::commands::deploy::cmd_deploy;
use crate::commands::doctor::cmd_doctor;
use crate::commands::idl::cmd_idl;
use crate::commands::localnet::{cmd_localnet, LocalnetAction};
use crate::commands::new::{cmd_new, NewCommand};
use crate::commands::setup::{cmd_setup, SetupCommand, WalletInstallMode};
use crate::commands::wallet::{cmd_wallet, WalletAction};
use crate::constants::VERSION;
use crate::DynResult;

#[derive(Debug, Parser)]
#[command(
    name = "logos-scaffold",
    version = VERSION,
    disable_help_subcommand = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    #[command(about = "Create a new logos-scaffold project")]
    Create(NewArgs),
    #[command(about = "Alias for `create`")]
    New(NewArgs),
    Setup(SetupArgs),
    Build(BuildArgs),
    Deploy(DeployArgs),
    Localnet(LocalnetArgs),
    Wallet(WalletArgs),
    Doctor(DoctorArgs),
    #[command(hide = true)]
    Help,
}

#[derive(Debug, clap::Args)]
struct NewArgs {
    name: String,
    #[arg(long)]
    vendor_deps: bool,
    #[arg(long)]
    lssa_path: Option<PathBuf>,
    #[arg(long)]
    cache_root: Option<PathBuf>,
    #[arg(long, default_value = "default")]
    template: String,
}

#[derive(Debug, clap::Args)]
struct SetupArgs {
    #[arg(long, value_enum, default_value_t = WalletInstallMode::Auto)]
    wallet_install: WalletInstallMode,
}

#[derive(Debug, clap::Args)]
struct BuildArgs {
    #[command(subcommand)]
    subcommand: Option<BuildSubcommand>,
    project_path: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
enum BuildSubcommand {
    #[command(about = "Build IDL files from the current project")]
    Idl(BuildSubArgs),
    #[command(about = "Build client code from IDL files")]
    Client(BuildSubArgs),
}

#[derive(Debug, clap::Args)]
struct BuildSubArgs {
    project_path: Option<PathBuf>,
}

#[derive(Debug, clap::Args)]
struct DeployArgs {
    program_name: Option<String>,
}

#[derive(Debug, clap::Args)]
struct DoctorArgs {
    #[arg(long)]
    json: bool,
}

#[derive(Debug, clap::Args)]
struct LocalnetArgs {
    #[command(subcommand)]
    command: LocalnetSubcommand,
}

#[derive(Debug, Subcommand)]
enum LocalnetSubcommand {
    Start(LocalnetStartArgs),
    Stop,
    Status(LocalnetStatusArgs),
    Logs(LocalnetLogsArgs),
}

#[derive(Debug, clap::Args)]
struct LocalnetStartArgs {
    #[arg(long, default_value_t = 20)]
    timeout_sec: u64,
}

#[derive(Debug, clap::Args)]
struct LocalnetStatusArgs {
    #[arg(long)]
    json: bool,
}

#[derive(Debug, clap::Args)]
struct LocalnetLogsArgs {
    #[arg(long, default_value_t = 200)]
    tail: usize,
}

#[derive(Debug, clap::Args)]
struct WalletArgs {
    #[command(subcommand)]
    command: WalletSubcommand,
}

#[derive(Debug, Subcommand)]
enum WalletSubcommand {
    #[command(about = "List wallet accounts (same as `wallet account list`)")]
    List(WalletListArgs),
    #[command(about = "Top up wallet using pinata faucet claim")]
    Topup(WalletTopupArgs),
    #[command(about = "Manage project default wallet")]
    Default(WalletDefaultArgs),
}

#[derive(Debug, clap::Args)]
struct WalletListArgs {
    #[arg(long)]
    long: bool,
}

#[derive(Debug, clap::Args)]
struct WalletTopupArgs {
    #[arg(value_name = "ADDRESS")]
    address: Option<String>,
    #[arg(long = "address", value_name = "ADDRESS")]
    address_flag: Option<String>,
    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug, clap::Args)]
struct WalletDefaultArgs {
    #[command(subcommand)]
    command: WalletDefaultSubcommand,
}

#[derive(Debug, Subcommand)]
enum WalletDefaultSubcommand {
    Set(WalletDefaultSetArgs),
}

#[derive(Debug, clap::Args)]
struct WalletDefaultSetArgs {
    #[arg(value_name = "ADDRESS")]
    address: Option<String>,
    #[arg(long = "address", value_name = "ADDRESS")]
    address_flag: Option<String>,
}

pub(crate) fn run(args: Vec<String>) -> DynResult<()> {
    if let Some(action) = wallet_passthrough_action(&args)? {
        return cmd_wallet(action);
    }

    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(err) => match err.kind() {
            clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion => {
                print!("{err}");
                return Ok(());
            }
            _ => return Err(anyhow!(err.to_string())),
        },
    };

    match cli.command {
        Some(Commands::Create(args)) | Some(Commands::New(args)) => cmd_new(NewCommand {
            name: args.name,
            vendor_deps: args.vendor_deps,
            lssa_path: args.lssa_path,
            cache_root: args.cache_root,
            template: args.template,
        }),
        Some(Commands::Setup(args)) => cmd_setup(SetupCommand {
            wallet_install: args.wallet_install,
        }),
        Some(Commands::Build(args)) => match args.subcommand {
            Some(BuildSubcommand::Idl(sub)) => cmd_idl(
                &sub.project_path
                    .map(|p| vec!["build".to_string(), p.to_string_lossy().to_string()])
                    .unwrap_or_else(|| vec!["build".to_string()]),
            ),
            Some(BuildSubcommand::Client(sub)) => cmd_client(
                &sub.project_path
                    .map(|p| vec!["build".to_string(), p.to_string_lossy().to_string()])
                    .unwrap_or_else(|| vec!["build".to_string()]),
            ),
            None => cmd_build_shortcut(args.project_path),
        },
        Some(Commands::Deploy(args)) => cmd_deploy(args.program_name),
        Some(Commands::Localnet(localnet)) => {
            let action = match localnet.command {
                LocalnetSubcommand::Start(args) => LocalnetAction::Start {
                    timeout_sec: args.timeout_sec,
                },
                LocalnetSubcommand::Stop => LocalnetAction::Stop,
                LocalnetSubcommand::Status(args) => LocalnetAction::Status { json: args.json },
                LocalnetSubcommand::Logs(args) => LocalnetAction::Logs { tail: args.tail },
            };
            cmd_localnet(action)
        }
        Some(Commands::Wallet(args)) => {
            let action = match args.command {
                WalletSubcommand::List(args) => WalletAction::List { long: args.long },
                WalletSubcommand::Topup(args) => WalletAction::Topup {
                    address: merge_optional_address(
                        args.address,
                        args.address_flag,
                        "wallet topup",
                    )?,
                    dry_run: args.dry_run,
                },
                WalletSubcommand::Default(args) => match args.command {
                    WalletDefaultSubcommand::Set(set) => WalletAction::DefaultSet {
                        address: require_address(
                            set.address,
                            set.address_flag,
                            "wallet default set",
                        )?,
                    },
                },
            };
            cmd_wallet(action)
        }
        Some(Commands::Doctor(args)) => cmd_doctor(args.json),
        Some(Commands::Help) => print_help(),
        None => print_help(),
    }
}

pub(crate) fn print_help() -> DynResult<()> {
    let mut cmd = Cli::command();
    cmd.print_help()?;
    println!();
    Ok(())
}

fn wallet_passthrough_action(args: &[String]) -> DynResult<Option<WalletAction>> {
    if args.len() < 3 {
        return Ok(None);
    }

    if args[1] == "wallet" && args[2] == "--" {
        if args.len() == 3 {
            return Err(anyhow!(
                "wallet passthrough requires at least one argument after `--`. Example: `logos-scaffold wallet -- account list`"
            ));
        }

        return Ok(Some(WalletAction::Proxy {
            args: args[3..].to_vec(),
        }));
    }

    Ok(None)
}

fn merge_optional_address(
    positional: Option<String>,
    flagged: Option<String>,
    context: &str,
) -> DynResult<Option<String>> {
    if positional.is_some() && flagged.is_some() {
        return Err(anyhow!(
            "{context}: provide address either as positional argument or `--address`, not both."
        ));
    }

    Ok(positional.or(flagged))
}

fn require_address(
    positional: Option<String>,
    flagged: Option<String>,
    context: &str,
) -> DynResult<String> {
    let merged = merge_optional_address(positional, flagged, context)?;
    merged.ok_or_else(|| {
        anyhow!(
            "{context} requires an address. Examples: `logos-scaffold wallet default set <address>` or `logos-scaffold wallet default set --address <address>`."
        )
    })
}
