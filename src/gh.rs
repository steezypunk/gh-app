use std::path::{Path, PathBuf};
use std::process::Command;

use crate::commands::auth;
use crate::config::io::load;
use crate::config::schema::Config;
use crate::error::{Error, Result};
use crate::github::{GitHubClient, HostInfo};
use crate::paths;
use crate::token;

#[cfg(unix)]
use std::os::unix::process::CommandExt;

pub fn run(args: Vec<String>) -> Result<i32> {
    if let Some(code) = redirect_auth_command(&args)? {
        return Ok(code);
    }

    let real_gh = find_real_gh()?;
    if is_static_command(&args) {
        return exec(real_gh, args, None);
    }

    let config = load(&paths::config_file()?)?;
    if config.hosts.is_empty() {
        if truthy("GH_APP_STRICT") {
            return Err(Error::message(
                "GH_APP_STRICT is set but no hosts are configured",
            ));
        }
        return exec(real_gh, args, None);
    }
    let hostname = target_host(&args, &config);
    let Some(host_config) = config.hosts.get(&hostname).cloned() else {
        if truthy("GH_APP_STRICT") {
            return Err(Error::message(format!(
                "GH_APP_STRICT is set but {hostname} is not configured"
            )));
        }
        return exec(real_gh, args, None);
    };
    let host = HostInfo::new(&hostname)?.with_api_url(host_config.api_url.as_deref())?;

    if std::env::var_os(host.token_env).is_some() && !truthy("GH_APP_FORCE") {
        return exec(real_gh, args, None);
    }

    let signer = auth::signer_for(&host_config)?;
    let client = GitHubClient::new(&host, auth::ca_bundle(&host_config).as_deref())?;
    let cached = match token::get_or_mint(
        &host,
        &host_config,
        &signer,
        &client,
        false,
        config.settings.refresh_skew_seconds,
        token::cache_enabled(config.settings.cache_tokens),
    ) {
        Ok(cached) => cached,
        Err(error) => {
            if truthy("GH_APP_FALLBACK") {
                return exec(real_gh, args, None);
            }
            return Err(error);
        }
    };
    exec(real_gh, args, Some((host.token_env, cached.token)))
}

fn redirect_auth_command(args: &[String]) -> Result<Option<i32>> {
    if args.first().map(String::as_str) != Some("auth") {
        return Ok(None);
    }
    let Some(subcommand) = args.get(1).map(String::as_str) else {
        return Ok(None);
    };
    match subcommand {
        "status" | "token" => Ok(None),
        "login" | "logout" | "refresh" | "setup-git" => {
            let exe = std::env::current_exe()
                .map_err(|e| Error::message(format!("could not find current executable: {e}")))?;
            let mut forwarded = vec!["auth".to_string(), subcommand.to_string()];
            forwarded.extend(args[2..].iter().map(|arg| {
                if arg == "-h" {
                    "--hostname".to_string()
                } else {
                    arg.clone()
                }
            }));
            let status = Command::new(exe)
                .args(forwarded)
                .status()
                .map_err(|e| Error::message(format!("could not run auth command: {e}")))?;
            Ok(Some(status.code().unwrap_or(1)))
        }
        "switch" => Err(Error::message(
            "gh-app has one credential per host; gh auth switch is not supported",
        )),
        _ => Ok(None),
    }
}

fn is_static_command(args: &[String]) -> bool {
    matches!(
        args.first().map(String::as_str),
        None | Some(
            "--version"
                | "version"
                | "--help"
                | "-h"
                | "help"
                | "completion"
                | "__complete"
                | "__completeNoDesc"
        )
    )
}

fn target_host(args: &[String], config: &Config) -> String {
    if let Some(host) = std::env::var("GH_APP_HOST")
        .ok()
        .filter(|value| !value.is_empty())
    {
        return host.to_ascii_lowercase();
    }
    if let Some(host) = flag_value(args, "--hostname") {
        return host.to_ascii_lowercase();
    }
    if let Some(repo) = flag_value(args, "--repo").or_else(|| flag_value(args, "-R")) {
        let parts: Vec<&str> = repo.split('/').collect();
        if parts.len() >= 3 {
            return parts[0].to_ascii_lowercase();
        }
    }
    if let Some(host) = std::env::var("GH_HOST")
        .ok()
        .filter(|value| !value.is_empty())
    {
        return host.to_ascii_lowercase();
    }
    if let Some(host) = git_remote_host()
        && config.hosts.contains_key(&host)
    {
        return host;
    }
    config.settings.default_host.clone()
}

fn git_remote_host() -> Option<String> {
    let output = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8(output.stdout).ok()?;
    parse_remote_host(value.trim())
}

fn parse_remote_host(value: &str) -> Option<String> {
    if let Ok(url) = url::Url::parse(value) {
        return url.host_str().map(|host| host.to_ascii_lowercase());
    }
    let rest = value.strip_prefix("git@")?;
    let (host, _) = rest.split_once(':')?;
    Some(host.to_ascii_lowercase())
}

fn flag_value(args: &[String], name: &str) -> Option<String> {
    let prefix = format!("{name}=");
    let mut index = 0;
    while index < args.len() {
        if args[index] == name {
            return args.get(index + 1).cloned();
        }
        if let Some(value) = args[index].strip_prefix(&prefix) {
            return Some(value.to_string());
        }
        index += 1;
    }
    None
}

fn find_real_gh() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("GH_APP_REAL_GH") {
        let path = PathBuf::from(path);
        if !path.is_file() {
            return Err(Error::message(format!(
                "GH_APP_REAL_GH does not exist: {}",
                path.display()
            )));
        }
        return Ok(path);
    }

    let current = std::env::current_exe().ok();
    let path = std::env::var_os("PATH")
        .ok_or_else(|| Error::message("PATH is not set; set GH_APP_REAL_GH"))?;
    for directory in std::env::split_paths(&path) {
        for name in ["gh", "gh.exe"] {
            let candidate = directory.join(name);
            if !candidate.is_file() {
                continue;
            }
            if current
                .as_ref()
                .is_some_and(|current| same_file(&candidate, current))
            {
                continue;
            }
            return Ok(candidate);
        }
    }
    Err(Error::message(
        "could not find the real gh executable; install gh or set GH_APP_REAL_GH",
    ))
}

fn same_file(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let (Ok(a), Ok(b)) = (std::fs::metadata(a), std::fs::metadata(b)) {
            return a.dev() == b.dev() && a.ino() == b.ino();
        }
    }
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

fn exec(real_gh: PathBuf, args: Vec<String>, token: Option<(&'static str, String)>) -> Result<i32> {
    let mut command = Command::new(&real_gh);
    command.args(&args);
    if let Some((name, value)) = token {
        command.env(name, value);
    }

    #[cfg(unix)]
    {
        let error = command.exec();
        Err(Error::message(format!(
            "could not execute {}: {error}",
            real_gh.display()
        )))
    }
    #[cfg(not(unix))]
    {
        let status = command
            .status()
            .map_err(|e| Error::message(format!("could not execute gh: {e}")))?;
        Ok(status.code().unwrap_or(1))
    }
}

fn truthy(name: &str) -> bool {
    std::env::var(name)
        .map(|value| matches!(value.as_str(), "1" | "true" | "yes"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_remote_hosts() {
        assert_eq!(
            parse_remote_host("https://github.com/owner/repo.git").as_deref(),
            Some("github.com")
        );
        assert_eq!(
            parse_remote_host("git@github.com:owner/repo.git").as_deref(),
            Some("github.com")
        );
        assert_eq!(
            parse_remote_host("ssh://git@ghe.example.com/owner/repo.git").as_deref(),
            Some("ghe.example.com")
        );
        assert_eq!(parse_remote_host("not a url"), None);
    }
}
