use thiserror::Error;

#[derive(Error, Debug)]
pub enum CoreError {
    #[error("Not found")]
    NotFound,

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Serialization error: {0}")]
    Serialization(String),
}

pub type Result<T> = std::result::Result<T, CoreError>;
