use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const CONFIG_VERSION: u32 = 1;
pub const DEFAULT_HOST: &str = "github.com";
pub const DEFAULT_REFRESH_SKEW_SECONDS: u64 = 300;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub settings: Settings,
    #[serde(default)]
    pub hosts: BTreeMap<String, HostConfig>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            settings: Settings::default(),
            hosts: BTreeMap::new(),
            extra: BTreeMap::new(),
        }
    }
}

impl Config {
    pub fn validate(&self) -> std::result::Result<(), String> {
        if self.version != CONFIG_VERSION {
            return Err(format!(
                "unsupported config version {} (this build supports {})",
                self.version, CONFIG_VERSION
            ));
        }
        validate_hostname(&self.settings.default_host)?;
        if self.settings.refresh_skew_seconds > 3600 {
            return Err("settings.refresh_skew_seconds must be at most 3600".into());
        }
        for (host, config) in &self.hosts {
            validate_hostname(host)?;
            config
                .validate()
                .map_err(|message| format!("hosts.{host}: {message}"))?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "default_host")]
    pub default_host: String,
    #[serde(default = "default_refresh_skew")]
    pub refresh_skew_seconds: u64,
    #[serde(default = "default_cache_tokens")]
    pub cache_tokens: bool,
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            default_host: default_host(),
            refresh_skew_seconds: default_refresh_skew(),
            cache_tokens: default_cache_tokens(),
            extra: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostConfig {
    pub app_id: u64,
    pub installation_id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_slug: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_login: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_key_keyring: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_key_env: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_key_command: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repositories: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permissions: Option<BTreeMap<String, String>>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeySourceKind {
    File,
    Keyring,
    Env,
    Command,
}

impl HostConfig {
    pub fn key_source_kind(&self) -> Result<KeySourceKind, String> {
        let mut found = Vec::new();
        if self.private_key.is_some() {
            found.push(KeySourceKind::File);
        }
        if self.private_key_keyring.is_some() {
            found.push(KeySourceKind::Keyring);
        }
        if self.private_key_env.is_some() {
            found.push(KeySourceKind::Env);
        }
        if self.private_key_command.is_some() {
            found.push(KeySourceKind::Command);
        }
        match found.as_slice() {
            [kind] => Ok(*kind),
            [] => Err("exactly one private key source is required".into()),
            _ => Err("private key sources are mutually exclusive".into()),
        }
    }

    pub fn key_source_label(&self) -> Result<String, String> {
        Ok(match self.key_source_kind()? {
            KeySourceKind::File => {
                format!("file {}", self.private_key.as_deref().unwrap_or_default())
            }
            KeySourceKind::Keyring => format!(
                "keyring {}",
                self.private_key_keyring.as_deref().unwrap_or_default()
            ),
            KeySourceKind::Env => format!(
                "env {}",
                self.private_key_env.as_deref().unwrap_or_default()
            ),
            KeySourceKind::Command => "command".to_string(),
        })
    }

    pub fn validate(&self) -> std::result::Result<(), String> {
        if self.app_id == 0 {
            return Err("app_id must be greater than zero".into());
        }
        if self.installation_id == 0 {
            return Err("installation_id must be greater than zero".into());
        }
        self.key_source_kind()?;
        if let Some(url) = &self.api_url
            && !url.starts_with("https://")
            && !url.starts_with("http://")
        {
            return Err("api_url must start with https:// or http://".into());
        }
        Ok(())
    }
}

pub fn validate_hostname(host: &str) -> std::result::Result<(), String> {
    if host.trim().is_empty() {
        return Err("hostname must not be empty".into());
    }
    if host != host.trim() {
        return Err("hostname must not have leading or trailing whitespace".into());
    }
    if host != host.to_ascii_lowercase() {
        return Err("hostname must be lowercase".into());
    }
    if host.contains('/') || host.contains(':') || host.contains('\\') {
        return Err("hostname must not contain a scheme, path, or port".into());
    }
    Ok(())
}

fn default_version() -> u32 {
    CONFIG_VERSION
}

fn default_host() -> String {
    DEFAULT_HOST.to_string()
}

fn default_refresh_skew() -> u64 {
    DEFAULT_REFRESH_SKEW_SECONDS
}

fn default_cache_tokens() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host() -> HostConfig {
        HostConfig {
            app_id: 1,
            installation_id: 2,
            client_id: None,
            app_slug: None,
            account_login: None,
            private_key: Some("keys/github.com-1.pem".into()),
            private_key_keyring: None,
            private_key_env: None,
            private_key_command: None,
            api_url: None,
            repositories: None,
            permissions: None,
            extra: BTreeMap::new(),
        }
    }

    #[test]
    fn valid_config() {
        let mut config = Config::default();
        config.hosts.insert("github.com".into(), host());
        assert!(config.validate().is_ok());
    }

    #[test]
    fn rejects_unknown_version() {
        let config = Config {
            version: 2,
            ..Config::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn rejects_missing_key_source() {
        let mut config = Config::default();
        let mut host = host();
        host.private_key = None;
        config.hosts.insert("github.com".into(), host);
        assert!(config.validate().is_err());
    }

    #[test]
    fn rejects_multiple_key_sources() {
        let mut config = Config::default();
        let mut host = host();
        host.private_key_env = Some("GH_APP_PRIVATE_KEY".into());
        config.hosts.insert("github.com".into(), host);
        assert!(config.validate().is_err());
    }

    #[test]
    fn rejects_host_with_port() {
        assert!(validate_hostname("github.com:443").is_err());
        assert!(validate_hostname("https://github.com").is_err());
        assert!(validate_hostname("GitHub.com").is_err());
    }
}
