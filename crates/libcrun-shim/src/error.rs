use std::fmt;

#[derive(Debug)]
pub enum ShimError {
    Runtime {
        message: String,
        context: Option<String>,
    },
    Io {
        error: std::io::Error,
        context: Option<String>,
    },
    Serialization {
        message: String,
        context: Option<String>,
    },
    NotFound {
        resource: String,
        context: Option<String>,
    },
    Validation {
        field: String,
        message: String,
    },
}

impl ShimError {
    pub fn runtime<S: Into<String>>(msg: S) -> Self {
        ShimError::Runtime {
            message: msg.into(),
            context: None,
        }
    }

    pub fn runtime_with_context<S1: Into<String>, S2: Into<String>>(msg: S1, ctx: S2) -> Self {
        ShimError::Runtime {
            message: msg.into(),
            context: Some(ctx.into()),
        }
    }

    pub fn not_found<S: Into<String>>(resource: S) -> Self {
        ShimError::NotFound {
            resource: resource.into(),
            context: None,
        }
    }

    pub fn validation<S1: Into<String>, S2: Into<String>>(field: S1, msg: S2) -> Self {
        ShimError::Validation {
            field: field.into(),
            message: msg.into(),
        }
    }
}

impl fmt::Display for ShimError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ShimError::Runtime { message, context } => {
                write!(f, "Runtime error: {}", message)?;
                if let Some(ctx) = context {
                    write!(f, " (context: {})", ctx)?;
                }
                Ok(())
            }
            ShimError::Io { error, context } => {
                write!(f, "IO error: {}", error)?;
                if let Some(ctx) = context {
                    write!(f, " (context: {})", ctx)?;
                }
                Ok(())
            }
            ShimError::Serialization { message, context } => {
                write!(f, "Serialization error: {}", message)?;
                if let Some(ctx) = context {
                    write!(f, " (context: {})", ctx)?;
                }
                Ok(())
            }
            ShimError::NotFound { resource, context } => {
                write!(f, "Resource not found: {}", resource)?;
                if let Some(ctx) = context {
                    write!(f, " (context: {})", ctx)?;
                }
                Ok(())
            }
            ShimError::Validation { field, message } => {
                write!(f, "Validation error for field '{}': {}", field, message)
            }
        }
    }
}

impl std::error::Error for ShimError {}

impl From<std::io::Error> for ShimError {
    fn from(e: std::io::Error) -> Self {
        ShimError::Io {
            error: e,
            context: None,
        }
    }
}

impl From<serde_json::Error> for ShimError {
    fn from(e: serde_json::Error) -> Self {
        ShimError::Serialization {
            message: e.to_string(),
            context: Some("JSON parsing error".to_string()),
        }
    }
}

pub type Result<T> = std::result::Result<T, ShimError>;
