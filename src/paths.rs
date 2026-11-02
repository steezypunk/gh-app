use std::env;
use std::ffi::OsString;
use std::path::PathBuf;

use crate::error::{Error, Result};

const CONFIG_ENV: &str = "GH_APP_CONFIG_DIR";
const CACHE_ENV: &str = "GH_APP_CACHE_DIR";

pub fn config_dir() -> Result<PathBuf> {
    resolve_dir(
        env::var_os(CONFIG_ENV),
        env::var_os("XDG_CONFIG_HOME"),
        dirs::home_dir(),
        false,
    )
}

pub fn cache_dir() -> Result<PathBuf> {
    resolve_dir(
        env::var_os(CACHE_ENV),
        env::var_os("XDG_CACHE_HOME"),
        dirs::home_dir(),
        true,
    )
}

pub fn config_file() -> Result<PathBuf> {
    Ok(config_dir()?.join("config.toml"))
}

pub fn keys_dir() -> Result<PathBuf> {
    Ok(config_dir()?.join("keys"))
}

pub fn tokens_dir() -> Result<PathBuf> {
    Ok(cache_dir()?.join("tokens"))
}

fn resolve_dir(
    override_dir: Option<OsString>,
    xdg_dir: Option<OsString>,
    home: Option<PathBuf>,
    cache: bool,
) -> Result<PathBuf> {
    if let Some(path) = override_dir {
        return Ok(PathBuf::from(path));
    }
    if let Some(path) = xdg_dir {
        let path = PathBuf::from(path);
        if path.is_absolute() {
            return Ok(path.join("gh-app"));
        }
    }
    if !cache {
        #[cfg(windows)]
        if let Some(path) = dirs::config_dir() {
            return Ok(path.join("gh-app"));
        }
    }
    if cache {
        #[cfg(windows)]
        if let Some(path) = dirs::cache_dir() {
            return Ok(path.join("gh-app"));
        }
    }
    let home = home.ok_or_else(|| Error::message("could not determine home directory"))?;
    Ok(home
        .join(if cache { ".cache" } else { ".config" })
        .join("gh-app"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_wins() {
        let path = resolve_dir(
            Some(OsString::from("/custom/gh-app")),
            Some(OsString::from("/xdg")),
            Some(PathBuf::from("/home/u")),
            false,
        )
        .unwrap();
        assert_eq!(path, PathBuf::from("/custom/gh-app"));
    }

    #[test]
    fn absolute_xdg_is_used() {
        let path = resolve_dir(
            None,
            Some(OsString::from("/xdg")),
            Some(PathBuf::from("/home/u")),
            false,
        )
        .unwrap();
        assert_eq!(path, PathBuf::from("/xdg/gh-app"));
    }

    #[test]
    fn relative_xdg_is_ignored() {
        let path = resolve_dir(
            None,
            Some(OsString::from("relative")),
            Some(PathBuf::from("/home/u")),
            false,
        )
        .unwrap();
        assert_eq!(path, PathBuf::from("/home/u/.config/gh-app"));
    }

    #[test]
    fn home_fallback_for_cache() {
        let path = resolve_dir(None, None, Some(PathBuf::from("/home/u")), true).unwrap();
        assert_eq!(path, PathBuf::from("/home/u/.cache/gh-app"));
    }
}
