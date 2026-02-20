use clap::Parser;
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
