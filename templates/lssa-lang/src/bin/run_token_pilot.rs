use clap::{Parser, Subcommand};
use example_program_deployment_methods::TOKEN_PILOT_ELF;
use wallet::WalletCore;

#[path = "../lib.rs"]
mod scaffold_lib;
use scaffold_lib::generated::token_pilot_client::{
    AccountRef, InitAccounts, LssaSubmitResult, TokenPilotClient, TransferAccounts,
};
use scaffold_lib::runner_support::load_program;

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
    Transfer {
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
        #[arg(long)]
        amount: u128,
    },
}

fn parse_account_ref(raw: &str) -> AccountRef {
    if raw.starts_with("Private/") {
        let account_id = raw
            .trim_start_matches("Private/")
            .parse()
            .unwrap_or_else(|err| panic!("invalid private account_id `{raw}`: {err}"));
        AccountRef::PrivateOwned(account_id)
    } else {
        let normalized = raw.trim_start_matches("Public/");
        let account_id = normalized
            .parse()
            .unwrap_or_else(|err| panic!("invalid public account_id `{raw}`: {err}"));
        AccountRef::Public(account_id)
    }
}

fn print_submit_result(result: LssaSubmitResult) {
    match result {
        LssaSubmitResult::Public(response) => {
            println!("Submitted public transaction: {:?}", response.tx_hash);
        }
        LssaSubmitResult::PrivateOwned {
            response,
            shared_secrets,
        } => {
            println!(
                "Submitted private-owned transaction: {:?} (shared_secrets={})",
                response.tx_hash,
                shared_secrets.len()
            );
        }
    }
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let wallet_core = WalletCore::from_env().expect("wallet should initialize from environment");

    let program = load_program(cli.program_path.as_deref(), TOKEN_PILOT_ELF, "token_pilot");
    let client = TokenPilotClient::new(&wallet_core, program);

    let result = match cli.command {
        Command::Init { to } => {
            client
                .init(InitAccounts {
                    target: parse_account_ref(&to),
                })
                .await
                .expect("init transaction should succeed")
        }
        Command::Transfer { from, to, amount } => {
            client
                .transfer(
                    TransferAccounts {
                        sender: parse_account_ref(&from),
                        recipient: parse_account_ref(&to),
                    },
                    amount,
                )
                .await
                .expect("transfer transaction should succeed")
        }
    };

    print_submit_result(result);
}
