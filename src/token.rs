use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

use crate::config::permissions::{check_no_group_other, ensure_dir_700};
use crate::config::schema::HostConfig;
use crate::error::{Error, Result};
use crate::github::host::HostInfo;
use crate::github::{GitHubClient, MintRequest, TokenResponse};
use crate::jwt;
use crate::key::Signer;
use crate::paths;

const CACHE_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedToken {
    pub version: u32,
    pub host: String,
    pub app_id: u64,
    pub installation_id: u64,
    pub scope_hash: String,
    pub token: String,
    pub expires_at: String,
    pub minted_at: String,
    pub permissions: BTreeMap<String, String>,
    pub repository_selection: String,
}

impl CachedToken {
    pub fn is_fresh(&self, skew_seconds: u64) -> bool {
        let Ok(expires) = OffsetDateTime::parse(&self.expires_at, &Rfc3339) else {
            return false;
        };
        expires - OffsetDateTime::now_utc() > Duration::seconds(skew_seconds as i64)
    }

    pub fn remaining_seconds(&self) -> i64 {
        OffsetDateTime::parse(&self.expires_at, &Rfc3339)
            .map(|expires| (expires - OffsetDateTime::now_utc()).whole_seconds())
            .unwrap_or(0)
    }

    pub fn masked_token(&self) -> String {
        if self.token.starts_with("ghs_") {
            "ghs_******".to_string()
        } else {
            let visible: String = self.token.chars().take(4).collect();
            format!("{visible}******")
        }
    }
}

pub fn get_or_mint(
    host: &HostInfo,
    config: &HostConfig,
    signer: &Signer,
    client: &GitHubClient,
    force: bool,
    skew_seconds: u64,
) -> Result<CachedToken> {
    let scope_hash = scope_hash(config);
    let key = cache_key(&host.hostname, config, &scope_hash);

    if !force && let Some(cached) = load(&key, skew_seconds)? {
        return Ok(cached);
    }

    let _lock = lock(&key)?;

    if !force && let Some(cached) = load(&key, skew_seconds)? {
        return Ok(cached);
    }

    let issuer = config
        .client_id
        .clone()
        .unwrap_or_else(|| config.app_id.to_string());
    let token = jwt::build(signer, &issuer, jwt::unix_now()?)?;
    let request = mint_request(config);
    let response = client.mint_token(&token, config.installation_id, Some(&request))?;
    let cached = CachedToken::from_response(host, config, &scope_hash, response)?;
    save(&key, &cached)?;
    Ok(cached)
}

pub fn store_response(
    host: &HostInfo,
    config: &HostConfig,
    response: TokenResponse,
) -> Result<CachedToken> {
    let scope_hash = scope_hash(config);
    let key = cache_key(&host.hostname, config, &scope_hash);
    let cached = CachedToken::from_response(host, config, &scope_hash, response)?;
    save(&key, &cached)?;
    Ok(cached)
}

pub fn load(key: &str, skew_seconds: u64) -> Result<Option<CachedToken>> {
    let path = cache_path(key)?;
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(Error::io(&path, e)),
    };
    check_no_group_other(&path)?;
    let cached: CachedToken = match serde_json::from_str(&text) {
        Ok(cached) => cached,
        Err(_) => return Ok(None),
    };
    if cached.version != CACHE_VERSION || !cached.is_fresh(skew_seconds) {
        return Ok(None);
    }
    Ok(Some(cached))
}

pub fn save(key: &str, token: &CachedToken) -> Result<()> {
    let path = cache_path(key)?;
    let parent = path
        .parent()
        .ok_or_else(|| Error::message("token cache path has no parent"))?;
    ensure_dir_700(parent)?;
    let text = serde_json::to_string_pretty(token)
        .map_err(|e| Error::message(format!("could not serialize token cache: {e}")))?;
    let mut temp = tempfile::Builder::new()
        .prefix(".token.")
        .tempfile_in(parent)
        .map_err(|e| Error::io(parent, e))?;
    temp.write_all(text.as_bytes())
        .map_err(|e| Error::io(&path, e))?;
    temp.as_file().sync_all().map_err(|e| Error::io(&path, e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o600))
            .map_err(|e| Error::io(temp.path(), e))?;
    }
    temp.persist(&path).map_err(|e| Error::io(&path, e.error))?;
    Ok(())
}

pub fn delete(key: &str) -> Result<()> {
    let path = cache_path(key)?;
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Error::io(path, e)),
    }
}

pub fn delete_all() -> Result<()> {
    let dir = paths::tokens_dir()?;
    match fs::remove_dir_all(&dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Error::io(dir, e)),
    }
}

pub fn cache_key(host: &str, config: &HostConfig, scope_hash: &str) -> String {
    let input = format!(
        "v1\0{}\0{}\0{}\0{}",
        host, config.app_id, config.installation_id, scope_hash
    );
    let digest = Sha256::digest(input.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
}

pub fn scope_hash(config: &HostConfig) -> String {
    let value = serde_json::json!({
        "repositories": config.repositories,
        "permissions": config.permissions,
    });
    let bytes = serde_json::to_vec(&value).unwrap_or_default();
    let digest = Sha256::digest(&bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
}

impl CachedToken {
    fn from_response(
        host: &HostInfo,
        config: &HostConfig,
        scope_hash: &str,
        response: TokenResponse,
    ) -> Result<Self> {
        OffsetDateTime::parse(&response.expires_at, &Rfc3339).map_err(|e| {
            Error::message(format!("GitHub returned an invalid expires_at value: {e}"))
        })?;
        let minted_at = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .map_err(|e| Error::message(format!("could not format time: {e}")))?;
        Ok(Self {
            version: CACHE_VERSION,
            host: host.hostname.clone(),
            app_id: config.app_id,
            installation_id: config.installation_id,
            scope_hash: scope_hash.to_string(),
            token: response.token,
            expires_at: response.expires_at,
            minted_at,
            permissions: response.permissions,
            repository_selection: response.repository_selection,
        })
    }
}

fn mint_request(config: &HostConfig) -> MintRequest {
    MintRequest {
        repositories: config.repositories.clone(),
        permissions: config.permissions.clone(),
    }
}

fn cache_path(key: &str) -> Result<PathBuf> {
    Ok(paths::tokens_dir()?.join(format!("{key}.json")))
}

fn lock(key: &str) -> Result<File> {
    let dir = paths::tokens_dir()?;
    ensure_dir_700(&dir)?;
    let path = dir.join(format!("{key}.lock"));
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|e| Error::io(&path, e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
            .map_err(|e| Error::io(&path, e))?;
    }
    file.lock()
        .map_err(|e| Error::message(format!("could not lock {}: {e}", path.display())))?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host_config() -> HostConfig {
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
            ca_bundle: None,
            repositories: None,
            permissions: None,
            extra: BTreeMap::new(),
        }
    }

    #[test]
    fn cache_key_is_stable() {
        let config = host_config();
        let a = cache_key("github.com", &config, "scope");
        let b = cache_key("github.com", &config, "scope");
        assert_eq!(a, b);
        assert_ne!(a, cache_key("ghe.example.com", &config, "scope"));
    }

    #[test]
    fn scope_hash_changes_with_scope() {
        let mut config = host_config();
        let a = scope_hash(&config);
        config.repositories = Some(vec!["acme/api".into()]);
        let b = scope_hash(&config);
        assert_ne!(a, b);
    }

    #[test]
    fn token_masking() {
        let token = CachedToken {
            version: 1,
            host: "github.com".into(),
            app_id: 1,
            installation_id: 2,
            scope_hash: "x".into(),
            token: "ghs_abcdefgh".into(),
            expires_at: "2030-01-01T00:00:00Z".into(),
            minted_at: "2026-01-01T00:00:00Z".into(),
            permissions: BTreeMap::new(),
            repository_selection: "all".into(),
        };
        assert_eq!(token.masked_token(), "ghs_******");
        assert!(token.is_fresh(300));
    }
}
