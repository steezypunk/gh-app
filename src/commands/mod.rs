use crate::cli::Cli;
use crate::cli::Command;
use crate::error::Result;

pub mod auth;
pub mod doctor;
pub mod setup;

pub fn dispatch(cli: Cli) -> Result<i32> {
    match cli.command {
        Command::Auth { command } => auth::run(command),
        Command::Doctor(args) => doctor::run(args),
        Command::Setup(args) => setup::run(args),
        Command::Version => {
            println!("gh-app {}", env!("CARGO_PKG_VERSION"));
            Ok(0)
        }
        Command::GitCredential { operation } => crate::git_credential::run(vec![operation]),
        Command::Gh { args } => crate::gh::run(args),
    }
}
