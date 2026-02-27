use clap::{Parser, Subcommand};
use example_program_deployment_methods::LEZ_COUNTER_ELF;
use wallet::WalletCore;

#[path = "../lib.rs"]
mod scaffold_lib;
use scaffold_lib::runner_support::{load_program, parse_account_id};

#[derive(Parser, Debug)]
struct Cli {
    #[arg(long)]
    program_path: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    Init {
        #[arg(long)]
        to: String,
    },
    Increment {
        #[arg(long)]
        counter: String,
        #[arg(long)]
        authority: String,
        #[arg(long)]
        amount: u64,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let _wallet_core = WalletCore::from_env().expect("wallet should initialize from environment");
    let _program = load_program(cli.program_path.as_deref(), LEZ_COUNTER_ELF, "lez_counter");

    match cli.command {
        Command::Init { to } => {
            let _account_id = parse_account_id(&to);
            println!("Init counter at account {to}");
            // TODO: submit transaction via wallet
        }
        Command::Increment {
            counter,
            authority,
            amount,
        } => {
            let _counter_id = parse_account_id(&counter);
            let _authority_id = parse_account_id(&authority);
            println!("Increment counter {counter} by {amount} (authority: {authority})");
            // TODO: submit transaction via wallet
        }
    }
}
