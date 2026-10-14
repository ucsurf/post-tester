//! Errors. Every message says what went wrong and, where possible, how to fix it.

use std::path::PathBuf;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid URL `{url}`: {reason}")]
    InvalidUrl { url: String, reason: String },

    #[error("invalid request: {0}")]
    InvalidRequest(String),

    #[error("could not resolve host `{host}`: {source}")]
    Dns { host: String, source: std::io::Error },

    #[error("could not connect to {addr}: {source}")]
    Connect { addr: String, source: std::io::Error },

    #[error("TLS handshake with `{host}` failed: {reason}. If this is a self-signed certificate, set `options.verify_tls: false`")]
    Tls { host: String, reason: String },

    #[error("HTTP error: {0}")]
    Http(String),

    #[error("no response within {0} ms. Increase `options.timeout_ms` if the server is slow")]
    Timeout(u64),

    #[error("cancelled")]
    Cancelled,

    #[error("too many redirects (more than {0}). Check for a redirect loop, or raise `options.max_redirects`")]
    TooManyRedirects(u32),

    #[error("invalid expression `{expr}`: {reason}")]
    Expression { expr: String, reason: String },

    #[error("template error: {0}")]
    Template(String),

    #[error("{path}: {reason}")]
    File { path: PathBuf, reason: String },

    #[error("not found: {0}")]
    NotFound(String),

    #[error("plugin error: {0}")]
    Plugin(String),

    #[error("secret `{name}` is not set. Store it with `pt secret set {name}` or set the environment variable `PT_SECRET_{env}`")]
    MissingSecret { name: String, env: String },

    #[error("import failed: {0}")]
    Import(String),

    #[error("WebSocket error: {0}")]
    WebSocket(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl Error {
    pub fn file(path: impl Into<PathBuf>, reason: impl std::fmt::Display) -> Self {
        Error::File { path: path.into(), reason: reason.to_string() }
    }

    pub fn missing_secret(name: &str) -> Self {
        Error::MissingSecret { name: name.to_string(), env: crate::secrets::env_var_name(name) }
    }
}
