//! Public API for wallet operations.
//!
//! # Example
//!
//! ```no_run
//! use logos_scaffold::wallet;
//!
//! wallet::topup(None).expect("topup failed");
//! ```

use crate::commands::wallet::{cmd_wallet, WalletAction};
use crate::DynResult;

/// Top up the wallet using the pinata faucet.
/// Pass `Some(address)` to specify a destination, or `None` to use the default wallet.
pub fn topup(address: Option<String>) -> DynResult<()> {
    cmd_wallet(WalletAction::Topup {
        address,
        dry_run: false,
    })
}

/// List wallet accounts.
pub fn list() -> DynResult<()> {
    cmd_wallet(WalletAction::List { long: true })
}
