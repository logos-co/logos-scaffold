use anyhow::Context;
use clap::Parser;
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
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let wallet_core = WalletCore::from_env().context("failed to initialize wallet from environment")?;

    let program = load_program(cli.program_path.as_deref(), HELLO_WORLD_ELF, "hello_world")?;
    let account_id = parse_account_id(&cli.account_id)?;

    // Fetch nonces and signing key for the account
    let nonces = wallet_core
        .get_accounts_nonces(vec![account_id])
        .await
        .context("failed to fetch account nonces")?;
    let signing_key = wallet_core
        .get_account_public_signing_key(account_id)
        .context("no signing key for account — must be a public account owned by this wallet")?;

    let greeting: Vec<u8> = vec![72, 111, 108, 97, 32, 109, 117, 110, 100, 111, 33];
    let message = Message::try_new(program.id(), vec![account_id], nonces, greeting)
        .context("failed to build hello_world transaction message")?;
    let witness_set = WitnessSet::for_message(&message, &[signing_key]);
    let tx = PublicTransaction::new(message, witness_set);

    let response = wallet_core
        .sequencer_client
        .send_tx_public(tx)
        .await
        .context("failed to submit public transaction to localnet")?;

    println!(
        "submitted transaction: status={} tx_hash={}",
        response.status, response.tx_hash
    );
    println!("verification hint: wallet account get --account-id {}", cli.account_id);

    Ok(())
}
