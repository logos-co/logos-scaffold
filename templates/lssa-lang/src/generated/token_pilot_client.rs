use common::rpc_primitives::requests::SendTxResponse;
use nssa::{
    AccountId, PrivateKey, PublicTransaction,
    privacy_preserving_transaction::circuit::ProgramWithDependencies,
    program::Program,
    public_transaction::{Message, WitnessSet},
};
use nssa_core::SharedSecretKey;
use wallet::{PrivacyPreservingAccount, WalletCore};

#[derive(Clone, Copy, Debug)]
pub enum AccountRef {
    Public(AccountId),
    PrivateOwned(AccountId),
}

impl AccountRef {
    fn account_id(self) -> AccountId {
        match self {
            AccountRef::Public(id) | AccountRef::PrivateOwned(id) => id,
        }
    }

    fn is_private_owned(self) -> bool {
        matches!(self, AccountRef::PrivateOwned(_))
    }
}

pub enum LssaSubmitResult {
    Public(SendTxResponse),
    PrivateOwned {
        response: SendTxResponse,
        shared_secrets: Vec<SharedSecretKey>,
    },
}

#[derive(Clone, Debug, serde::Serialize)]
enum TokenPilotInstruction {
    Initialize,
    Transfer {
        amount: u128,
    },
}

pub struct InitAccounts {
    pub target: AccountRef,
}

pub struct TransferAccounts {
    pub sender: AccountRef,
    pub recipient: AccountRef,
}

struct AccountInput {
    name: &'static str,
    account_ref: AccountRef,
    auth: bool,
}

pub struct TokenPilotClient<'w> {
    wallet_core: &'w WalletCore,
    program: Program,
}

impl<'w> TokenPilotClient<'w> {
    pub fn new(wallet_core: &'w WalletCore, program: Program) -> Self {
        Self { wallet_core, program }
    }

    pub async fn init(
        &self,
        accounts: InitAccounts,
    ) -> Result<LssaSubmitResult, Box<dyn std::error::Error>> {
        let instruction = TokenPilotInstruction::Initialize;
        self.submit(instruction, vec![
            AccountInput { name: "target", account_ref: accounts.target, auth: true },
        ]).await
    }

    pub async fn transfer(
        &self,
        accounts: TransferAccounts,
        amount: u128,
    ) -> Result<LssaSubmitResult, Box<dyn std::error::Error>> {
        let instruction = TokenPilotInstruction::Transfer {
            amount,
        };
        self.submit(instruction, vec![
            AccountInput { name: "sender", account_ref: accounts.sender, auth: true },
            AccountInput { name: "recipient", account_ref: accounts.recipient, auth: false },
        ]).await
    }

    async fn submit(
        &self,
        instruction: TokenPilotInstruction,
        account_inputs: Vec<AccountInput>,
    ) -> Result<LssaSubmitResult, Box<dyn std::error::Error>> {
        let has_private = account_inputs.iter().any(|input| input.account_ref.is_private_owned());

        if has_private {
            let privacy_accounts = account_inputs
                .iter()
                .map(|input| match input.account_ref {
                    AccountRef::Public(id) => PrivacyPreservingAccount::Public(id),
                    AccountRef::PrivateOwned(id) => PrivacyPreservingAccount::PrivateOwned(id),
                })
                .collect::<Vec<_>>();

            let instruction_data = Program::serialize_instruction(instruction)?;
            let program_with_dependencies = ProgramWithDependencies::from(self.program.clone());
            let (response, shared_secrets) = self
                .wallet_core
                .send_privacy_preserving_tx(privacy_accounts, instruction_data, &program_with_dependencies)
                .await?;

            return Ok(LssaSubmitResult::PrivateOwned { response, shared_secrets });
        }

        let account_ids = account_inputs
            .iter()
            .map(|input| input.account_ref.account_id())
            .collect::<Vec<_>>();

        let auth_public_accounts = account_inputs
            .iter()
            .filter_map(|input| {
                if !input.auth {
                    return None;
                }
                match input.account_ref {
                    AccountRef::Public(id) => Some((input.name, id)),
                    AccountRef::PrivateOwned(_) => None,
                }
            })
            .collect::<Vec<_>>();

        let auth_account_ids = auth_public_accounts.iter().map(|(_, id)| *id).collect::<Vec<_>>();

        let nonces = if auth_account_ids.is_empty() {
            Vec::new()
        } else {
            self.wallet_core.get_accounts_nonces(auth_account_ids).await?
        };

        let mut signing_keys: Vec<&PrivateKey> = Vec::with_capacity(auth_public_accounts.len());
        for (name, account_id) in &auth_public_accounts {
            let signing_key = self
                .wallet_core
                .storage()
                .user_data
                .get_pub_account_signing_key(*account_id)
                .ok_or_else(|| std::io::Error::other(format!("missing public signing key for `{name}` account {account_id}")))?;
            signing_keys.push(signing_key);
        }

        let message = Message::try_new(self.program.id(), account_ids, nonces, instruction)?;
        let witness_set = WitnessSet::for_message(&message, &signing_keys);
        let tx = PublicTransaction::new(message, witness_set);
        let response = self.wallet_core.sequencer_client.send_tx_public(tx).await?;

        Ok(LssaSubmitResult::Public(response))
    }
}
