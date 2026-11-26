use thiserror::Error;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid configuration: {0}")]
    Config(String),
    #[error("network error: {0}")]
    Network(String),
    #[error("provider error ({status}): {message}")]
    Provider { status: u16, message: String },
    #[error("provider returned an invalid response: {0}")]
    ProviderResponse(String),
    #[error("audio error: {0}")]
    Audio(String),
    #[error("text insertion failed: {0}")]
    Insert(String),
    #[error("database error: {0}")]
    Db(String),
    #[error("unsupported on this platform: {0}")]
    Unsupported(String),
    #[error("operation timed out: {0}")]
    Timeout(String),
    #[error("operation cancelled")]
    Cancelled,
}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Error::Db(e.to_string())
    }
}
