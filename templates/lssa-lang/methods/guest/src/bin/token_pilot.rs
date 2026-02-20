use lssa_lang::prelude::*;
use nssa_core::program::{
    AccountPostState, DEFAULT_PROGRAM_ID, ProgramInput, read_nssa_inputs, write_nssa_outputs,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, LssaInstruction)]
pub enum TokenPilotInstruction {
    Initialize,
    Transfer { amount: u128 },
}

#[derive(Clone, Debug, LssaAccounts)]
pub struct InitAccounts {
    #[lssa(mut, auth, claim_if_default, visibility = "public|private_owned")]
    pub target: String,
}

#[derive(Clone, Debug, LssaAccounts)]
pub struct TransferAccounts {
    #[lssa(mut, auth, visibility = "public|private_owned")]
    pub sender: String,
    #[lssa(mut, claim_if_default, visibility = "public|private_owned")]
    pub recipient: String,
}

#[allow(dead_code)]
#[lssa_program(name = "token_pilot", version = "0.1.0")]
mod idl_metadata {
    use super::*;

    #[lssa_instruction(
        instruction = TokenPilotInstruction,
        accounts = InitAccounts,
        name = "init",
        execution = "public,private_owned"
    )]
    fn init_instruction() {}

    #[lssa_instruction(
        instruction = TokenPilotInstruction,
        accounts = TransferAccounts,
        name = "transfer",
        execution = "public,private_owned"
    )]
    fn transfer_instruction() {}
}

fn main() {
    let (
        ProgramInput {
            pre_states,
            instruction,
        },
        instruction_words,
    ) = read_nssa_inputs::<TokenPilotInstruction>();

    let pre_states_clone = pre_states.clone();

    let post_states = match instruction {
        TokenPilotInstruction::Initialize => {
            let [target] = pre_states
                .try_into()
                .unwrap_or_else(|_| panic!("Initialize requires exactly one account"));

            if !target.is_authorized {
                panic!("Missing required authorization");
            }
            if target.account != nssa_core::account::Account::default() {
                panic!("Target account must be uninitialized");
            }

            vec![AccountPostState::new_claimed(target.account)]
        }
        TokenPilotInstruction::Transfer { amount } => {
            let [sender, recipient] = pre_states
                .try_into()
                .unwrap_or_else(|_| panic!("Transfer requires exactly two accounts"));

            if !sender.is_authorized {
                panic!("Sender must be authorized");
            }
            if sender.account.balance < amount {
                panic!("Sender has insufficient balance");
            }

            let mut sender_post = sender.account.clone();
            sender_post.balance -= amount;

            let mut recipient_post = recipient.account.clone();
            recipient_post.balance += amount;

            let recipient_output = if recipient_post.program_owner == DEFAULT_PROGRAM_ID {
                AccountPostState::new_claimed(recipient_post)
            } else {
                AccountPostState::new(recipient_post)
            };

            vec![AccountPostState::new(sender_post), recipient_output]
        }
    };

    write_nssa_outputs(instruction_words, pre_states_clone, post_states);
}

#[cfg(test)]
mod tests {
    #[test]
    fn __lssa_idl_print() {
        println!("--- LSSA IDL BEGIN token_pilot ---");
        println!("{}", super::idl_metadata::__lssa_idl_json());
        println!("--- LSSA IDL END token_pilot ---");
    }
}
