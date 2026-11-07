use thiserror::Error;

/// Every failure a computation can report. Computations never panic; they return one of these with a plain-English message.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum MathError {
    #[error("{0}")]
    Domain(String),
    #[error("{0}")]
    Unsupported(String),
    #[error("{0}")]
    TooLarge(String),
    #[error("computation cancelled")]
    Cancelled,
    #[error("computation timed out")]
    Timeout,
}

pub type MathResult<T> = Result<T, MathError>;

impl MathError {
    pub fn domain(msg: impl Into<String>) -> Self {
        MathError::Domain(msg.into())
    }
    pub fn unsupported(msg: impl Into<String>) -> Self {
        MathError::Unsupported(msg.into())
    }
    pub fn too_large(msg: impl Into<String>) -> Self {
        MathError::TooLarge(msg.into())
    }
}
