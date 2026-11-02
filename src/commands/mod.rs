use crate::cli::Cli;
use crate::cli::{AuthCommand, Command};
use crate::error::{Error, Result};

pub mod auth;
pub mod doctor;
mod stubs;

pub fn dispatch(cli: Cli) -> Result<i32> {
    match cli.command {
        Command::Auth { command } => auth::run(command),
        Command::Doctor(args) => doctor::run(args),
        Command::Setup(args) => stubs::setup(args),
        Command::Version => {
            println!("gh-app {}", env!("CARGO_PKG_VERSION"));
            Ok(0)
        }
        Command::GitCredential { operation } => stubs::git_credential(vec![operation]),
        Command::Gh { args } => stubs::gh(args),
    }
}

pub fn gh(args: Vec<String>) -> Result<i32> {
    stubs::gh(args)
}

pub fn git_credential(args: Vec<String>) -> Result<i32> {
    stubs::git_credential(args)
}

pub fn auth_git_credential(operation: String) -> Result<i32> {
    stubs::git_credential(vec![operation])
}

fn not_implemented(name: &str) -> Error {
    Error::message(format!(
        "{name} is not implemented yet; the design and plan are in docs/"
    ))
}

fn stub_auth_command(command: &str) -> Result<i32> {
    Err(not_implemented(command))
}

pub(crate) fn stub_auth(command: AuthCommand) -> Result<i32> {
    match command {
        AuthCommand::List => auth::list(),
        AuthCommand::GitCredential { operation } => auth_git_credential(operation),
        AuthCommand::Login(_) => stub_auth_command("auth login"),
        AuthCommand::Status(_) => stub_auth_command("auth status"),
        AuthCommand::Token(_) => stub_auth_command("auth token"),
        AuthCommand::Refresh(_) => stub_auth_command("auth refresh"),
        AuthCommand::Logout(_) => stub_auth_command("auth logout"),
        AuthCommand::SetupGit(_) => stub_auth_command("auth setup-git"),
    }
}
