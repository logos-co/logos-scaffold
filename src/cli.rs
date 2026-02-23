use std::path::PathBuf;

use anyhow::anyhow;
use clap::{CommandFactory, Parser, Subcommand};

use crate::commands::build::cmd_build_shortcut;
use crate::commands::doctor::cmd_doctor;
use crate::commands::localnet::{cmd_localnet, LocalnetAction};
use crate::commands::new::{cmd_new, NewCommand};
use crate::commands::setup::{cmd_setup, SetupCommand, WalletInstallMode};
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
    Localnet(LocalnetArgs),
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
    project_path: Option<PathBuf>,
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

pub(crate) fn run(args: Vec<String>) -> DynResult<()> {
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
        Some(Commands::Build(args)) => cmd_build_shortcut(args.project_path),
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
