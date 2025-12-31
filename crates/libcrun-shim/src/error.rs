use std::fmt;

#[derive(Debug)]
pub enum ShimError {
    Runtime(String),
    Io(std::io::Error),
    Serialization(String),
    NotFound(String),
}

impl fmt::Display for ShimError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ShimError::Runtime(msg) => write!(f, "Runtime error: {}", msg),
            ShimError::Io(e) => write!(f, "IO error: {}", e),
            ShimError::Serialization(msg) => write!(f, "Serialization error: {}", msg),
            ShimError::NotFound(id) => write!(f, "Container not found: {}", id),
        }
    }
}

impl std::error::Error for ShimError {}

impl From<std::io::Error> for ShimError {
    fn from(e: std::io::Error) -> Self {
        ShimError::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, ShimError>;

