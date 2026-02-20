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
