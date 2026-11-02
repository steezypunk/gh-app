use std::io;
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Message(String),

    #[error("io error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("config error at {path}: {message}")]
    Config { path: PathBuf, message: String },

    #[error("invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("GitHub API error (HTTP {status}): {message}")]
    Api { status: u16, message: String },
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    pub fn config(path: impl Into<PathBuf>, message: impl Into<String>) -> Self {
        Self::Config {
            path: path.into(),
            message: message.into(),
        }
    }

    pub fn message(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }

    pub fn api(status: u16, message: impl Into<String>, request_id: Option<String>) -> Self {
        let message = match request_id {
            Some(request_id) => format!("{} (request id: {request_id})", message.into()),
            None => message.into(),
        };
        Self::Api { status, message }
    }
}
