use anyhow::Context;
use clap::Parser;
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
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let wallet_core = WalletCore::from_env().context("failed to initialize wallet from environment")?;

    let program = load_program(
        cli.program_path.as_deref(),
        TAIL_CALL_WITH_PDA_ELF,
        "tail_call_with_pda",
    )?;

    let pda = AccountId::from((&program.id(), &PDA_SEED));
    let message = Message::try_new(program.id(), vec![pda], vec![], ())
        .context("failed to build pda transaction message")?;
    let nonces = wallet_core
        .get_accounts_nonces(vec![account_id])
        .await
        .context("failed to fetch account nonces")?;
    let signing_key = wallet_core
        .get_account_public_signing_key(account_id)
        .context("no signing key for account — must be a public account owned by this wallet")?;
    let witness_set = WitnessSet::for_message(&message, &[signing_key]);
    let tx = PublicTransaction::new(message, witness_set);

    let response = wallet_core
        .sequencer_client
        .send_transaction(NSSATransaction::Public(tx))
        .await
        .context("failed to submit public transaction to localnet")?;

    println!(
        "submitted transaction: tx_hash={}",
        response
    );
    println!("the program derived account id is: {pda}");

    Ok(())
}
