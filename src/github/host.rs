use crate::config::schema::validate_hostname;
use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostInfo {
    pub hostname: String,
    pub api_base: String,
    pub token_env: &'static str,
    pub enterprise_server: bool,
}

impl HostInfo {
    pub fn new(hostname: &str) -> Result<Self> {
        let hostname = hostname.trim().to_ascii_lowercase();
        validate_hostname(&hostname).map_err(Error::message)?;

        let (api_base, token_env, enterprise_server) = if hostname == "github.com" {
            ("https://api.github.com".to_string(), "GH_TOKEN", false)
        } else if hostname == "github.localhost" {
            (
                "http://api.github.localhost".to_string(),
                "GH_ENTERPRISE_TOKEN",
                true,
            )
        } else if hostname.ends_with(".ghe.com") {
            (format!("https://api.{hostname}"), "GH_TOKEN", false)
        } else {
            (
                format!("https://{hostname}/api/v3"),
                "GH_ENTERPRISE_TOKEN",
                true,
            )
        };

        Ok(Self {
            hostname,
            api_base,
            token_env,
            enterprise_server,
        })
    }

    pub fn with_api_url(mut self, api_url: Option<&str>) -> Result<Self> {
        if let Some(api_url) = api_url {
            let trimmed = api_url.trim_end_matches('/');
            if !trimmed.starts_with("https://") && !trimmed.starts_with("http://") {
                return Err(Error::message(
                    "api_url must start with https:// or http://",
                ));
            }
            self.api_base = trimmed.to_string();
        }
        Ok(self)
    }

    pub fn api_version_header(&self) -> Option<&'static str> {
        if self.enterprise_server {
            None
        } else {
            Some("2026-03-10")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_com() {
        let host = HostInfo::new("github.com").unwrap();
        assert_eq!(host.api_base, "https://api.github.com");
        assert_eq!(host.token_env, "GH_TOKEN");
        assert!(!host.enterprise_server);
        assert_eq!(host.api_version_header(), Some("2026-03-10"));
    }

    #[test]
    fn ghe_com() {
        let host = HostInfo::new("acme.ghe.com").unwrap();
        assert_eq!(host.api_base, "https://api.acme.ghe.com");
        assert_eq!(host.token_env, "GH_TOKEN");
        assert!(!host.enterprise_server);
    }

    #[test]
    fn enterprise_server() {
        let host = HostInfo::new("ghe.example.com").unwrap();
        assert_eq!(host.api_base, "https://ghe.example.com/api/v3");
        assert_eq!(host.token_env, "GH_ENTERPRISE_TOKEN");
        assert!(host.enterprise_server);
        assert_eq!(host.api_version_header(), None);
    }

    #[test]
    fn rejects_host_with_scheme() {
        assert!(HostInfo::new("https://github.com").is_err());
    }

    #[test]
    fn api_override() {
        let host = HostInfo::new("github.com")
            .unwrap()
            .with_api_url(Some("http://127.0.0.1:8080/"))
            .unwrap();
        assert_eq!(host.api_base, "http://127.0.0.1:8080");
    }
}
