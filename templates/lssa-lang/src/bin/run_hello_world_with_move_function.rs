use clap::{Parser, Subcommand};
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
