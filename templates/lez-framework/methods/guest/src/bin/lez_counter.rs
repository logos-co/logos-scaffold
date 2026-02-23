#![no_main]

use nssa_core::account::AccountWithMetadata;
use nssa_core::program::{
    AccountPostState, DEFAULT_PROGRAM_ID, ProgramInput, read_nssa_inputs, write_nssa_outputs,
};
use lez_framework::prelude::*;
use serde::{Deserialize, Serialize};

risc0_zkvm::guest::entry!(main);

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
