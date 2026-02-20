pub(crate) fn render_runner_support_lib() -> String {
    r#"#[allow(dead_code)]
pub mod runner_support {
    use nssa::{AccountId, program::Program};

    pub fn parse_account_id(raw: &str) -> AccountId {
        let normalized = raw
            .strip_prefix("Public/")
            .or_else(|| raw.strip_prefix("Private/"))
            .unwrap_or(raw);

        normalized
            .parse()
            .unwrap_or_else(|err| panic!("invalid account_id `{raw}`: {err}"))
    }

    pub fn load_program(program_path: Option<&str>, embedded_elf: &[u8], label: &str) -> Program {
        let bytes = if let Some(path) = program_path {
            std::fs::read(path)
                .unwrap_or_else(|err| panic!("failed to read {label} binary at `{path}`: {err}"))
        } else {
            embedded_elf.to_vec()
        };

        Program::new(bytes).unwrap_or_else(|err| panic!("failed to parse {label} program: {err}"))
    }
}
"#
    .to_string()
}

pub(crate) fn render_runner_run_hello_world() -> String {
    r#"use clap::Parser;
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
async fn main() {
    let cli = Cli::parse();
    let wallet_core = WalletCore::from_env().unwrap();

    let program = load_program(cli.program_path.as_deref(), HELLO_WORLD_ELF, "hello_world");
    let account_id = parse_account_id(&cli.account_id);

    let greeting: Vec<u8> = vec![72, 111, 108, 97, 32, 109, 117, 110, 100, 111, 33];
    let message = Message::try_new(program.id(), vec![account_id], vec![], greeting).unwrap();
    let witness_set = WitnessSet::for_message(&message, &[]);
    let tx = PublicTransaction::new(message, witness_set);

    let _response = wallet_core.sequencer_client.send_tx_public(tx).await.unwrap();
}
"#
    .to_string()
}

pub(crate) fn render_runner_run_hello_world_private() -> String {
    r#"use clap::Parser;
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
"#
    .to_string()
}

pub(crate) fn render_runner_run_hello_world_with_authorization() -> String {
    r#"use clap::Parser;
use example_program_deployment_methods::HELLO_WORLD_WITH_AUTHORIZATION_ELF;
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
        HELLO_WORLD_WITH_AUTHORIZATION_ELF,
        "hello_world_with_authorization",
    );
    let account_id = parse_account_id(&cli.account_id);

    let signing_key = wallet_core
        .storage()
        .user_data
        .get_pub_account_signing_key(account_id)
        .expect("Input account should be a self owned public account");

    let greeting: Vec<u8> = vec![72, 111, 108, 97, 32, 109, 117, 110, 100, 111, 33];
    let nonces = wallet_core
        .get_accounts_nonces(vec![account_id])
        .await
        .expect("Node should be reachable to query account data");
    let message = Message::try_new(program.id(), vec![account_id], nonces, greeting).unwrap();
    let witness_set = WitnessSet::for_message(&message, &[signing_key]);
    let tx = PublicTransaction::new(message, witness_set);

    let _response = wallet_core.sequencer_client.send_tx_public(tx).await.unwrap();
}
"#
    .to_string()
}

pub(crate) fn render_runner_run_hello_world_through_tail_call() -> String {
    r#"use clap::Parser;
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
"#
    .to_string()
}

pub(crate) fn render_runner_run_hello_world_through_tail_call_private() -> String {
    r#"use std::collections::HashMap;

use clap::Parser;
use example_program_deployment_methods::{HELLO_WORLD_ELF, SIMPLE_TAIL_CALL_ELF};
use nssa::{
    ProgramId, privacy_preserving_transaction::circuit::ProgramWithDependencies, program::Program,
};
use wallet::{PrivacyPreservingAccount, WalletCore};

#[path = "../lib.rs"]
mod scaffold_lib;
use scaffold_lib::runner_support::{load_program, parse_account_id};

#[derive(Parser, Debug)]
struct Cli {
    #[arg(long)]
    simple_tail_call_path: Option<String>,
    #[arg(long)]
    hello_world_path: Option<String>,
    account_id: String,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let wallet_core = WalletCore::from_env().unwrap();

    let simple_tail_call = load_program(
        cli.simple_tail_call_path.as_deref(),
        SIMPLE_TAIL_CALL_ELF,
        "simple_tail_call",
    );
    let hello_world = load_program(
        cli.hello_world_path.as_deref(),
        HELLO_WORLD_ELF,
        "hello_world",
    );

    let dependencies: HashMap<ProgramId, Program> =
        [(hello_world.id(), hello_world)].into_iter().collect();
    let program_with_dependencies = ProgramWithDependencies::new(simple_tail_call, dependencies);
    let account_id = parse_account_id(&cli.account_id);
    let accounts = vec![PrivacyPreservingAccount::PrivateOwned(account_id)];

    wallet_core
        .send_privacy_preserving_tx(
            accounts,
            Program::serialize_instruction(()).unwrap(),
            &program_with_dependencies,
        )
        .await
        .unwrap();
}
"#
    .to_string()
}

pub(crate) fn render_runner_run_hello_world_with_authorization_through_tail_call_with_pda() -> String
{
    r#"use clap::Parser;
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
"#
    .to_string()
}

pub(crate) fn render_runner_run_hello_world_with_move_function() -> String {
    r#"use clap::{Parser, Subcommand};
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
"#
    .to_string()
}
