use std::fs;
use std::io::Write;
use std::path::Path;

use crate::config::permissions::{check_no_group_other, ensure_dir_700};
use crate::config::schema::Config;
use crate::error::{Error, Result};

pub fn load(path: &Path) -> Result<Config> {
    match fs::read_to_string(path) {
        Ok(text) => {
            check_no_group_other(path)?;
            let config: Config =
                toml::from_str(&text).map_err(|e| Error::config(path, e.to_string()))?;
            config
                .validate()
                .map_err(|e| Error::InvalidConfig(format!("{}: {e}", path.display())))?;
            Ok(config)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(Error::io(path, e)),
    }
}

#[allow(dead_code)] // Used by auth login in a later milestone.
pub fn load_required(path: &Path) -> Result<Config> {
    if !path.exists() {
        return Err(Error::message(format!(
            "{} does not exist; run: gh-app auth login",
            path.display()
        )));
    }
    load(path)
}

#[allow(dead_code)] // Used by auth login and logout in a later milestone.
pub fn save(path: &Path, config: &Config) -> Result<()> {
    config
        .validate()
        .map_err(|e| Error::InvalidConfig(e.to_string()))?;
    let parent = path
        .parent()
        .ok_or_else(|| Error::message(format!("{} has no parent directory", path.display())))?;
    ensure_dir_700(parent)?;

    let text = toml::to_string_pretty(config)
        .map_err(|e| Error::config(path, format!("could not serialize config: {e}")))?;

    let mut temp = tempfile::Builder::new()
        .prefix(".config.")
        .tempfile_in(parent)
        .map_err(|e| Error::io(parent, e))?;
    temp.write_all(text.as_bytes())
        .map_err(|e| Error::io(path, e))?;
    temp.as_file().sync_all().map_err(|e| Error::io(path, e))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o600))
            .map_err(|e| Error::io(temp.path(), e))?;
    }

    temp.persist(path).map_err(|e| Error::io(path, e.error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;

    use super::*;
    use crate::config::schema::HostConfig;

    fn host() -> HostConfig {
        HostConfig {
            app_id: 123,
            installation_id: 456,
            client_id: None,
            app_slug: None,
            account_login: None,
            private_key: Some("keys/github.com-123.pem".into()),
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
    fn round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut config = Config::default();
        config.hosts.insert("github.com".into(), host());
        save(&path, &config).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.hosts["github.com"].app_id, 123);
        assert_eq!(loaded.hosts["github.com"].installation_id, 456);
    }

    #[test]
    fn missing_config_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let config = load(&dir.path().join("config.toml")).unwrap();
        assert!(config.hosts.is_empty());
        assert_eq!(config.version, 1);
    }

    #[test]
    fn preserves_unknown_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let text = r#"
version = 1

[settings]
default_host = "github.com"
future_setting = "keep-me"

[hosts."github.com"]
app_id = 123
installation_id = 456
private_key = "keys/github.com-123.pem"
future_host_field = "keep-me-too"
"#;
        fs::write(&path, text).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        }
        let config = load(&path).unwrap();
        save(&path, &config).unwrap();
        let written = fs::read_to_string(&path).unwrap();
        assert!(written.contains("future_setting = \"keep-me\""));
        assert!(written.contains("future_host_field = \"keep-me-too\""));
    }

    #[test]
    fn rejects_unsupported_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "version = 2\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        }
        assert!(load(&path).is_err());
    }
}
