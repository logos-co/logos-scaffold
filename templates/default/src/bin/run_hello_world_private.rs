use clap::Parser;
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
