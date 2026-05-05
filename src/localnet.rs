//! Public API for localnet operations.
//!
//! # Example
//!
//! ```no_run
//! use logos_scaffold::localnet;
//!
//! localnet::start(20).expect("failed to start localnet");
//! let status = localnet::status().expect("failed to get status");
//! println!("ready: {}", status.ready);
//! localnet::stop().expect("failed to stop localnet");
//! ```

use crate::commands::localnet::{build_localnet_status_for_project, cmd_localnet, LocalnetAction};
use crate::model::LocalnetStatusReport;
use crate::project::load_project;
use crate::DynResult;

/// Start the local sequencer and wait until it is ready.
pub fn start(timeout_sec: u64) -> DynResult<()> {
    cmd_localnet(LocalnetAction::Start { timeout_sec })
}

/// Stop the local sequencer.
pub fn stop() -> DynResult<()> {
    cmd_localnet(LocalnetAction::Stop)
}

/// Return the current localnet status.
pub fn status() -> DynResult<LocalnetStatusReport> {
    let project = load_project()?;
    Ok(build_localnet_status_for_project(&project))
}
