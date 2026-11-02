use std::ffi::OsStr;
use std::path::Path;
use std::process::ExitCode;

use clap::Parser;

mod cli;
mod commands;
mod config;
mod error;
mod paths;

fn main() -> ExitCode {
    #[cfg(unix)]
    unsafe {
        libc::umask(0o077);
    }

    let argv0 = std::env::args_os().next().unwrap_or_default();
    let base = basename(&argv0);
    let result = match base.as_str() {
        "gh" => commands::gh(std::env::args().skip(1).collect()),
        "git-credential-gh-app" => commands::git_credential(std::env::args().skip(1).collect()),
        _ => commands::dispatch(cli::Cli::parse()),
    };

    match result {
        Ok(code) => ExitCode::from(code.clamp(0, 255) as u8),
        Err(error) => {
            eprintln!("gh-app: {error}");
            ExitCode::from(1)
        }
    }
}

fn basename(argv0: &OsStr) -> String {
    Path::new(argv0)
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or("gh-app")
        .to_string()
}
