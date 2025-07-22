use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Error, Debug, Clone, Serialize, Deserialize)]
pub enum IbkrError {
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("API error: {0}")]
    ApiError(String),

    #[error("Not connected")]
    NotConnected,

    #[error("Request failed: {0}")]
    RequestFailed(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Unknown error: {0}")]
    Unknown(String),
}

impl From<ibapi::Error> for IbkrError {
    fn from(err: ibapi::Error) -> Self {
        IbkrError::ApiError(err.to_string())
    }
}

impl From<serde_json::Error> for IbkrError {
    fn from(err: serde_json::Error) -> Self {
        IbkrError::SerializationError(err.to_string())
    }
}

pub type Result<T> = std::result::Result<T, IbkrError>;
