use std::ffi::OsStr;
use std::path::Path;
use std::process::ExitCode;

use clap::Parser;

mod cli;
mod commands;
mod config;
mod error;
mod gh;
mod git_credential;
mod github;
mod jwt;
mod key;
mod keyring_store;
mod paths;
mod token;

fn main() -> ExitCode {
    #[cfg(unix)]
    unsafe {
        libc::umask(0o077);
    }

    let argv0 = std::env::args_os().next().unwrap_or_default();
    let base = basename(&argv0);
    let result = match base.as_str() {
        "gh" => gh::run(std::env::args().skip(1).collect()),
        "git-credential-gh-app" => git_credential::run(std::env::args().skip(1).collect()),
        _ => {
            let args: Vec<String> = std::env::args().skip(1).collect();
            if args.first().is_none_or(|arg| is_gh_app_command(arg)) {
                commands::dispatch(cli::Cli::parse())
            } else {
                gh::run(args)
            }
        }
    };

    match result {
        Ok(code) => ExitCode::from(code.clamp(0, 255) as u8),
        Err(error) => {
            eprintln!("gh-app: {error}");
            ExitCode::from(1)
        }
    }
}

fn is_gh_app_command(arg: &str) -> bool {
    matches!(
        arg,
        "auth"
            | "setup"
            | "doctor"
            | "version"
            | "gh"
            | "git-credential"
            | "help"
            | "--help"
            | "-h"
            | "--version"
            | "-V"
    )
}

fn basename(argv0: &OsStr) -> String {
    Path::new(argv0)
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or("gh-app")
        .to_string()
}
