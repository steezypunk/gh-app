use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "gh-app",
    version,
    about = "GitHub App credential wrapper for gh and git"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Manage GitHub App credentials.
    Auth {
        #[command(subcommand)]
        command: AuthCommand,
    },
    /// Install the gh shim and configure git.
    Setup(SetupArgs),
    /// Check configuration, keys, and environment.
    Doctor(DoctorArgs),
    /// Print the gh-app version.
    Version,
    /// Hidden git credential helper entry point.
    #[command(hide = true)]
    GitCredential { operation: String },
    /// Execute the real gh explicitly.
    Gh {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum AuthCommand {
    /// Capture GitHub App credentials.
    Login(LoginArgs),
    /// Show configured hosts and token state.
    Status(StatusArgs),
    /// Print an installation token.
    Token(TokenArgs),
    /// Refresh a cached installation token.
    Refresh(RefreshArgs),
    /// Remove credentials for a host.
    Logout(LogoutArgs),
    /// Configure git to use gh-app as a credential helper.
    SetupGit(SetupGitArgs),
    /// List configured hosts.
    List,
    /// Hidden compatibility alias for git-credential.
    #[command(hide = true)]
    GitCredential { operation: String },
}

#[derive(Debug, Args)]
pub struct LoginArgs {
    /// GitHub hostname.
    #[arg(long)]
    pub hostname: Option<String>,
    /// GitHub App ID.
    #[arg(long)]
    pub app_id: Option<u64>,
    /// Installation ID.
    #[arg(long)]
    pub installation_id: Option<u64>,
    /// Private key PEM file, or - for stdin.
    #[arg(long, value_name = "PATH|-")]
    pub private_key: Option<String>,
    /// Store the private key in the OS keyring.
    #[arg(long)]
    pub keyring: bool,
    /// Reference the key in place instead of copying it into the config directory.
    #[arg(long)]
    pub reference_key: bool,
    /// Client ID to use as the JWT issuer.
    #[arg(long)]
    pub client_id: Option<String>,
    /// Override the GitHub API base URL.
    #[arg(long)]
    pub api_url: Option<String>,
    /// Additional PEM CA bundle.
    #[arg(long)]
    pub cacert: Option<PathBuf>,
    /// Store without verifying against GitHub.
    #[arg(long)]
    pub no_verify: bool,
    /// Overwrite an existing host entry.
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct StatusArgs {
    /// Check only this hostname.
    #[arg(long)]
    pub hostname: Option<String>,
    /// Emit JSON.
    #[arg(long)]
    pub json: bool,
    /// Force a token mint and API validation.
    #[arg(long)]
    pub verify: bool,
}

#[derive(Debug, Args)]
pub struct TokenArgs {
    /// GitHub hostname.
    #[arg(long)]
    pub hostname: Option<String>,
    /// Force a new token.
    #[arg(long)]
    pub refresh: bool,
}

#[derive(Debug, Args)]
pub struct RefreshArgs {
    /// GitHub hostname.
    #[arg(long)]
    pub hostname: Option<String>,
}

#[derive(Debug, Args)]
pub struct LogoutArgs {
    /// GitHub hostname.
    #[arg(long, conflicts_with = "all")]
    pub hostname: Option<String>,
    /// Remove all hosts.
    #[arg(long)]
    pub all: bool,
    /// Do not prompt for confirmation.
    #[arg(long)]
    pub yes: bool,
    /// Leave git credential helper configuration in place.
    #[arg(long)]
    pub keep_git_config: bool,
}

#[derive(Debug, Args)]
pub struct SetupGitArgs {
    /// Configure only this hostname.
    #[arg(long)]
    pub hostname: Option<String>,
    /// Configure even if the host is not known.
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct SetupArgs {
    /// Directory for the gh symlink.
    #[arg(long)]
    pub shim_dir: Option<PathBuf>,
    /// Overwrite an existing file at the shim path.
    #[arg(long)]
    pub force: bool,
    /// Do not install the gh shim.
    #[arg(long)]
    pub no_shim: bool,
    /// Do not configure git.
    #[arg(long)]
    pub no_git: bool,
}

#[derive(Debug, Args)]
pub struct DoctorArgs {
    /// Skip network checks.
    #[arg(long)]
    pub offline: bool,
    /// Emit JSON.
    #[arg(long)]
    pub json: bool,
}
