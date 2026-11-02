use std::path::{Path, PathBuf};

use crate::cli::DoctorArgs;
use crate::config::io::load;
use crate::config::permissions::read_private_key;
use crate::config::schema::KeySourceKind;
use crate::error::Result;
use crate::key::Signer;
use crate::keyring_store;
use crate::paths;

pub fn run(args: DoctorArgs) -> Result<i32> {
    let config_path = paths::config_file()?;
    println!("Config: {}", config_path.display());

    if !config_path.exists() {
        println!("Status: not configured (run: gh-app auth login)");
        return Ok(0);
    }

    let config = match load(&config_path) {
        Ok(config) => config,
        Err(error) => {
            println!("Status: FAIL");
            println!("  {error}");
            return Ok(1);
        }
    };

    println!("Status: {} host(s) configured", config.hosts.len());
    let base = paths::config_dir()?;
    println!("Keys: {}", paths::keys_dir()?.display());
    println!("Cache: {}", paths::cache_dir()?.display());
    println!("Tokens: {}", paths::tokens_dir()?.display());
    let mut failures = 0;

    for (host, host_config) in &config.hosts {
        println!(
            "{host}: app {} installation {}",
            host_config.app_id, host_config.installation_id
        );
        match host_config.key_source_kind() {
            Ok(KeySourceKind::File) => {
                let value = host_config.private_key.as_deref().unwrap_or_default();
                let path = resolve_key_path(value, &base);
                match read_private_key(&path) {
                    Ok(bytes) => {
                        println!("  key: ok ({} bytes, {})", bytes.len(), path.display())
                    }
                    Err(error) => {
                        println!("  key: FAIL ({})", path.display());
                        println!("    {error}");
                        failures += 1;
                    }
                }
            }
            Ok(KeySourceKind::Keyring) => {
                let value = host_config
                    .private_key_keyring
                    .as_deref()
                    .unwrap_or_default();
                match keyring_store::load(value).and_then(|pem| Signer::from_pem(&pem)) {
                    Ok(signer) => println!("  key: ok (keyring {}, {} bits)", value, signer.bits()),
                    Err(error) => {
                        println!("  key: FAIL (keyring {value})");
                        println!("    {error}");
                        failures += 1;
                    }
                }
            }
            Ok(KeySourceKind::Env) => {
                let name = host_config.private_key_env.as_deref().unwrap_or_default();
                match Signer::from_env(name) {
                    Ok(signer) => println!("  key: ok (env {name}, {} bits)", signer.bits()),
                    Err(error) => {
                        println!("  key: FAIL (env {name})");
                        println!("    {error}");
                        failures += 1;
                    }
                }
            }
            Ok(KeySourceKind::Command) => {
                println!("  key: external command (not checked in this milestone)");
            }
            Err(error) => {
                println!("  key: FAIL ({error})");
                failures += 1;
            }
        }
    }

    if args.offline {
        println!("Network: skipped (--offline)");
    } else {
        println!("Network: checks are not implemented in this milestone");
    }

    Ok(if failures == 0 { 0 } else { 1 })
}

fn resolve_key_path(value: &str, base: &Path) -> PathBuf {
    if let Some(rest) = value.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest);
    }
    let path = Path::new(value);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_relative_path() {
        let path = resolve_key_path("keys/github.com-1.pem", Path::new("/cfg/gh-app"));
        assert_eq!(path, PathBuf::from("/cfg/gh-app/keys/github.com-1.pem"));
    }

    #[test]
    fn resolves_absolute_path() {
        let path = resolve_key_path("/tmp/key.pem", Path::new("/cfg/gh-app"));
        assert_eq!(path, PathBuf::from("/tmp/key.pem"));
    }
}
