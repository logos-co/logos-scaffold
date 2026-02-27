#[allow(dead_code)]
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

// Host-side program definition for IDL extraction and testing.
// The guest binary (methods/guest) handles zkvm execution.
use lez_framework::prelude::*;
use lez_framework::error::{LezError, LezResult};
use lez_framework_core::types::LezOutput;
use nssa_core::program::AccountPostState;
use nssa_core::account::AccountWithMetadata;

#[lez_program]
mod lez_counter {
    #[allow(unused_imports)]
    use super::*;

    #[instruction]
    pub fn initialize(
        #[account(init, pda = literal("counter"))]
        counter: AccountWithMetadata,
        #[account(signer)]
        authority: AccountWithMetadata,
    ) -> LezResult {
        Ok(LezOutput::states_only(vec![
            AccountPostState::new_claimed(counter.account.clone()),
            AccountPostState::new(authority.account.clone()),
        ]))
    }

    #[instruction]
    pub fn increment(
        #[account(mut, pda = literal("counter"))]
        counter: AccountWithMetadata,
        #[account(signer)]
        authority: AccountWithMetadata,
        amount: u64,
    ) -> LezResult {
        let mut counter_post = counter.account.clone();
        counter_post.balance += amount as u128;

        Ok(LezOutput::states_only(vec![
            AccountPostState::new(counter_post),
            AccountPostState::new(authority.account.clone()),
        ]))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn __lssa_idl_print() {
        println!("--- LSSA IDL BEGIN lez_counter ---");
        println!("{}", super::PROGRAM_IDL_JSON);
        println!("--- LSSA IDL END lez_counter ---");
    }
}
