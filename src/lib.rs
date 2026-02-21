pub(crate) type DynResult<T> = anyhow::Result<T>;

pub(crate) mod cli;
pub(crate) mod commands;
pub(crate) mod config;
pub(crate) mod constants;
pub(crate) mod doctor_checks;
pub(crate) mod error;
pub(crate) mod model;
pub(crate) mod process;
pub(crate) mod project;
pub(crate) mod repo;
pub(crate) mod state;
pub(crate) mod template;

pub fn run(args: Vec<String>) -> DynResult<()> {
    cli::run(args)
}
