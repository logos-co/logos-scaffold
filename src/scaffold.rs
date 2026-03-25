//! Public API for scaffold project operations.
//!
//! # Example
//!
//! ```no_run
//! use logos_scaffold::scaffold;
//!
//! scaffold::setup().expect("setup failed");
//! ```

use crate::commands::setup::{cmd_setup, SetupCommand, WalletInstallMode};
use crate::DynResult;

/// Run scaffold setup: sync lssa, build sequencer, install wallet.
pub fn setup() -> DynResult<()> {
    cmd_setup(SetupCommand {
        wallet_install: WalletInstallMode::Auto,
    })
}
