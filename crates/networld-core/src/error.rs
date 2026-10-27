//! Error types shared by every module of the Networld VPN core.

use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),

    #[error("tls error: {0}")]
    Tls(String),

    #[error("ssh error: {0}")]
    Ssh(String),

    #[error("authentication failed for user '{0}'")]
    AuthFailed(String),

    #[error("payload rejected by upstream: {0}")]
    PayloadRejected(String),

    #[error("proxy error: {0}")]
    Proxy(String),

    #[error("configuration error: {0}")]
    Config(String),

    #[error("could not parse {what}: {reason}")]
    Parse { what: String, reason: String },

    #[error("engine '{engine}' unavailable: {reason}")]
    EngineUnavailable { engine: String, reason: String },

    #[error("required binary '{0}' was not found in the resources/bin folder")]
    MissingBinary(String),

    #[error("operation timed out after {0:?}")]
    Timeout(Duration),

    #[error("operation cancelled")]
    Cancelled,

    #[error("not supported on this platform: {0}")]
    Unsupported(String),

    #[error("elevation required: {0}")]
    ElevationRequired(String),

    #[error("{0}")]
    Other(String),
}

impl CoreError {
    pub fn other(msg: impl Into<String>) -> Self {
        CoreError::Other(msg.into())
    }

    pub fn parse(what: impl Into<String>, reason: impl Into<String>) -> Self {
        CoreError::Parse {
            what: what.into(),
            reason: reason.into(),
        }
    }

    pub fn config(msg: impl Into<String>) -> Self {
        CoreError::Config(msg.into())
    }

    /// A short, user-facing hint shown in the UI status strip.
    pub fn user_hint(&self) -> &'static str {
        match self {
            CoreError::AuthFailed(_) => "Check the username and password on this config.",
            CoreError::PayloadRejected(_) => {
                "The proxy/payload was rejected — try another bug host or payload template."
            }
            CoreError::Timeout(_) => "The server did not answer in time. Try another server.",
            CoreError::MissingBinary(_) => "A bundled engine binary is missing from this install.",
            CoreError::ElevationRequired(_) => "Run Networld VPN as administrator for this mode.",
            CoreError::Tls(_) => "TLS handshake failed — check the SNI / bug host.",
            _ => "See the Logs tab for the full trace.",
        }
    }
}

impl From<russh::Error> for CoreError {
    fn from(e: russh::Error) -> Self {
        CoreError::Ssh(e.to_string())
    }
}

impl From<serde_json::Error> for CoreError {
    fn from(e: serde_json::Error) -> Self {
        CoreError::Parse {
            what: "json".into(),
            reason: e.to_string(),
        }
    }
}

impl From<rustls::Error> for CoreError {
    fn from(e: rustls::Error) -> Self {
        CoreError::Tls(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, CoreError>;
