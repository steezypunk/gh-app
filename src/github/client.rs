use std::io::BufReader;
use std::path::Path;
use std::time::Duration;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use ureq::tls::{Certificate, RootCerts, TlsConfig};
use ureq::{Agent, Proxy};

use crate::error::{Error, Result};
use crate::github::host::HostInfo;
use crate::github::models::{App, Installation, MintRequest, RepositoryPage, TokenResponse};

pub struct GitHubClient {
    agent: Agent,
    base: String,
    api_version: Option<&'static str>,
}

impl GitHubClient {
    pub fn new(host: &HostInfo, ca_bundle: Option<&Path>) -> Result<Self> {
        let mut builder = Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(30)))
            .user_agent(concat!("gh-app/", env!("CARGO_PKG_VERSION")))
            .http_status_as_error(false);

        if let Some(proxy) = Proxy::try_from_env() {
            builder = builder.proxy(Some(proxy));
        }
        if let Some(path) = ca_bundle {
            let roots = load_roots(path)?;
            let tls = TlsConfig::builder().root_certs(roots).build();
            builder = builder.tls_config(tls);
        }

        Ok(Self {
            agent: builder.build().into(),
            base: host.api_base.clone(),
            api_version: host.api_version_header(),
        })
    }

    pub fn get_app(&self, jwt: &str) -> Result<App> {
        self.get_json("/app", jwt)
    }

    pub fn get_installation(&self, jwt: &str, installation_id: u64) -> Result<Installation> {
        self.get_json(&format!("/app/installations/{installation_id}"), jwt)
    }

    pub fn list_installations(&self, jwt: &str) -> Result<Vec<Installation>> {
        self.get_json("/app/installations?per_page=100", jwt)
    }

    pub fn mint_token(
        &self,
        jwt: &str,
        installation_id: u64,
        request: Option<&MintRequest>,
    ) -> Result<TokenResponse> {
        let path = format!("/app/installations/{installation_id}/access_tokens");
        let data = self.post_raw(&path, jwt, request)?;
        parse_response(data)
    }

    pub fn list_repositories(&self, token: &str) -> Result<RepositoryPage> {
        self.get_json("/installation/repositories?per_page=1", token)
    }

    fn get_json<T: DeserializeOwned>(&self, path: &str, token: &str) -> Result<T> {
        let url = format!("{}{}", self.base, path);
        let mut request = self
            .agent
            .get(&url)
            .header("Accept", "application/vnd.github+json")
            .header("Authorization", format!("Bearer {token}"));
        if let Some(version) = self.api_version {
            request = request.header("X-GitHub-Api-Version", version);
        }
        let response = request
            .call()
            .map_err(|e| Error::message(format!("GitHub request failed: {e}")))?;
        parse_response(into_response_data(response)?)
    }

    fn post_raw(
        &self,
        path: &str,
        token: &str,
        body: Option<&MintRequest>,
    ) -> Result<ResponseData> {
        let url = format!("{}{}", self.base, path);
        let mut request = self
            .agent
            .post(&url)
            .header("Accept", "application/vnd.github+json")
            .header("Authorization", format!("Bearer {token}"));
        if let Some(version) = self.api_version {
            request = request.header("X-GitHub-Api-Version", version);
        }
        let response = match body {
            Some(body) => request.send_json(body),
            None => request.send_empty(),
        }
        .map_err(|e| Error::message(format!("GitHub request failed: {e}")))?;
        into_response_data(response)
    }
}

struct ResponseData {
    status: u16,
    body: String,
    request_id: Option<String>,
}

fn parse_response<T: DeserializeOwned>(data: ResponseData) -> Result<T> {
    if !(200..300).contains(&data.status) {
        return Err(parse_api_error(data));
    }
    serde_json::from_str(&data.body)
        .map_err(|e| Error::message(format!("could not parse GitHub response: {e}")))
}

fn into_response_data(mut response: http::Response<ureq::Body>) -> Result<ResponseData> {
    let status = response.status().as_u16();
    let request_id = response
        .headers()
        .get("x-github-request-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|e| Error::message(format!("could not read GitHub response: {e}")))?;
    Ok(ResponseData {
        status,
        body,
        request_id,
    })
}

#[derive(Deserialize)]
struct ApiErrorBody {
    message: Option<String>,
}

fn parse_api_error(data: ResponseData) -> Error {
    let message = serde_json::from_str::<ApiErrorBody>(&data.body)
        .ok()
        .and_then(|body| body.message)
        .unwrap_or_else(|| {
            let trimmed = data.body.trim();
            if trimmed.is_empty() {
                "request failed".to_string()
            } else {
                trimmed.chars().take(300).collect()
            }
        });
    Error::api(data.status, message, data.request_id)
}

fn load_roots(path: &Path) -> Result<RootCerts> {
    let file = std::fs::File::open(path).map_err(|e| Error::io(path, e))?;
    let mut reader = BufReader::new(file);
    let certs = rustls_pemfile::certs(&mut reader)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| Error::config(path, format!("could not parse CA bundle: {e}")))?;
    if certs.is_empty() {
        return Err(Error::config(path, "CA bundle contains no certificates"));
    }

    let leaked: Vec<&'static [u8]> = certs
        .into_iter()
        .map(|cert| Box::leak(cert.as_ref().to_vec().into_boxed_slice()) as &'static [u8])
        .collect();
    let certificates: Vec<Certificate<'static>> = leaked
        .iter()
        .map(|der| Certificate::from_der(der))
        .collect();
    Ok(RootCerts::new_with_certs(&certificates))
}
