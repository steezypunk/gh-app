use std::collections::HashMap;
use std::io::{self, Read};

use base64::Engine;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use url::Url;

use crate::commands::auth;
use crate::config::io::load;
use crate::error::{Error, Result};
use crate::github::GitHubClient;
use crate::paths;
use crate::token;

pub fn run(args: Vec<String>) -> Result<i32> {
    let operation = args.first().map(String::as_str).unwrap_or("get");
    match operation {
        "store" | "erase" => Ok(0),
        "get" => get(),
        other => Err(Error::message(format!(
            "git-credential: unsupported operation {other:?}"
        ))),
    }
}

fn get() -> Result<i32> {
    let request = read_request()?;
    let protocol = request
        .get("protocol")
        .cloned()
        .unwrap_or_else(|| "https".to_string());
    if protocol != "https" && !(protocol == "http" && truthy("GH_APP_ALLOW_HTTP")) {
        return Ok(0);
    }
    let Some(host) = request.get("host").cloned() else {
        return Ok(0);
    };
    if host.is_empty() {
        return Ok(0);
    }

    let config = load(&paths::config_file()?)?;
    let normalized = normalize_host(&host);
    let Some((hostname, host_config)) = config
        .hosts
        .iter()
        .find(|(name, _)| normalize_host(name) == normalized)
        .map(|(name, config)| (name.clone(), config.clone()))
    else {
        return Ok(0);
    };

    let signer = auth::signer_for(&host_config)?;
    let host_info = auth::host_info(&hostname, &host_config)?;
    let client = GitHubClient::new(&host_info, auth::ca_bundle(&host_config).as_deref())?;
    let token = token::get_or_mint(
        &host_info,
        &host_config,
        &signer,
        &client,
        false,
        config.settings.refresh_skew_seconds,
        token::cache_enabled(config.settings.cache_tokens),
    )?;

    let username = "x-access-token";
    if request.has_authtype_capability() {
        let encoded =
            base64::engine::general_purpose::STANDARD.encode(format!("{username}:{}", token.token));
        println!("capability[]=authtype");
        println!("protocol={protocol}");
        println!("host={host}");
        println!("username={username}");
        println!("authtype=Basic");
        println!("credential={encoded}");
        println!("ephemeral=1");
    } else {
        println!("protocol={protocol}");
        println!("host={host}");
        println!("username={username}");
        println!("password={}", token.token);
        if let Ok(expires) = OffsetDateTime::parse(&token.expires_at, &Rfc3339) {
            println!("password_expiry_utc={}", expires.unix_timestamp());
        }
    }
    Ok(0)
}

struct CredentialRequest {
    fields: HashMap<String, String>,
    capabilities: Vec<String>,
}

impl CredentialRequest {
    fn get(&self, name: &str) -> Option<&String> {
        self.fields.get(name)
    }

    fn has_authtype_capability(&self) -> bool {
        self.capabilities
            .iter()
            .any(|capability| capability == "authtype")
    }
}

fn read_request() -> Result<CredentialRequest> {
    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .map_err(|e| Error::message(format!("could not read credential request: {e}")))?;
    let mut fields = HashMap::new();
    let mut capabilities = Vec::new();
    for line in input.lines() {
        if line.is_empty() {
            break;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key {
            "capability[]" => capabilities.push(value.to_string()),
            "url" => apply_url(&mut fields, value),
            _ => {
                fields.insert(key.to_string(), value.to_string());
            }
        }
    }
    Ok(CredentialRequest {
        fields,
        capabilities,
    })
}

fn apply_url(fields: &mut HashMap<String, String>, value: &str) {
    let Ok(url) = Url::parse(value) else {
        return;
    };
    fields.insert("protocol".to_string(), url.scheme().to_string());
    let host = match url.host_str() {
        Some(host) => match url.port() {
            Some(port) => format!("{host}:{port}"),
            None => host.to_string(),
        },
        None => return,
    };
    fields.insert("host".to_string(), host);
    fields.insert("path".to_string(), url.path().to_string());
    if !url.username().is_empty() {
        fields.insert("username".to_string(), url.username().to_string());
    }
    if let Some(password) = url.password() {
        fields.insert("password".to_string(), password.to_string());
    }
}

fn normalize_host(host: &str) -> String {
    let mut host = host.trim().to_ascii_lowercase();
    if let Some(value) = host.strip_suffix(":443") {
        host = value.to_string();
    } else if let Some(value) = host.strip_suffix(":80") {
        host = value.to_string();
    }
    host
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
    fn normalizes_default_ports() {
        assert_eq!(normalize_host("GitHub.com:443"), "github.com");
        assert_eq!(normalize_host("ghe.example.com:80"), "ghe.example.com");
        assert_eq!(
            normalize_host("ghe.example.com:8443"),
            "ghe.example.com:8443"
        );
    }

    #[test]
    fn parses_url_input() {
        let mut fields = HashMap::new();
        apply_url(&mut fields, "https://github.com/owner/repo");
        assert_eq!(fields["protocol"], "https");
        assert_eq!(fields["host"], "github.com");
        assert_eq!(fields["path"], "/owner/repo");
    }
}
