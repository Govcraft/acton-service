//! Error types and HTTP response conversion

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

/// Result type alias using the framework error
pub type Result<T> = std::result::Result<T, Error>;

/// Main error type for the framework
///
/// Large error variants are boxed to reduce stack size
#[derive(Debug, Error)]
pub enum Error {
    /// Configuration error
    #[error("Configuration error: {0}")]
    Config(Box<figment::Error>),

    /// Database error
    #[cfg(feature = "database")]
    #[error("Database error: {0}")]
    Database(Box<sqlx::Error>),

    /// Redis error
    #[cfg(feature = "cache")]
    #[error("Redis error: {0}")]
    Redis(Box<redis::RedisError>),

    /// NATS error
    #[cfg(feature = "events")]
    #[error("NATS error: {0}")]
    Nats(String),

    /// Turso/libsql error
    #[cfg(feature = "turso")]
    #[error("Turso error: {0}")]
    Turso(String),

    /// JWT error
    #[error("JWT error: {0}")]
    Jwt(Box<jsonwebtoken::errors::Error>),

    /// HTTP error
    #[error("HTTP error: {0}")]
    Http(Box<axum::http::Error>),

    /// I/O error
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Authentication error
    #[error("Authentication failed: {0}")]
    Unauthorized(String),

    /// Authorization error
    #[error("Forbidden: {0}")]
    Forbidden(String),

    /// Resource not found
    #[error("Not found: {0}")]
    NotFound(String),

    /// Bad request
    #[error("Bad request: {0}")]
    BadRequest(String),

    /// Rate limit exceeded
    #[error("Rate limit exceeded")]
    RateLimitExceeded,

    /// Resource conflict (409)
    #[error("Conflict: {0}")]
    Conflict(String),

    /// Validation error (422)
    #[error("Validation error: {0}")]
    ValidationError(String),

    /// Internal server error
    #[error("Internal server error: {0}")]
    Internal(String),

    /// Generic error
    #[error("{0}")]
    Other(String),
}

/// Error response body
#[derive(Debug, Serialize, Deserialize)]
pub struct ErrorResponse {
    /// Error message
    pub error: String,

    /// Optional error code
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,

    /// HTTP status code
    pub status: u16,
}

impl ErrorResponse {
    /// Create a new error response
    pub fn new(status: StatusCode, error: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            code: None,
            status: status.as_u16(),
        }
    }

    /// Create error response with a code
    pub fn with_code(status: StatusCode, code: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            code: Some(code.into()),
            status: status.as_u16(),
        }
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let (status, error_response) = match self {
            Error::Config(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                ErrorResponse::with_code(StatusCode::INTERNAL_SERVER_ERROR, "CONFIG_ERROR", e.to_string()),
            ),

            #[cfg(feature = "database")]
            Error::Database(e) => {
                tracing::error!("Database error: {}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ErrorResponse::with_code(StatusCode::INTERNAL_SERVER_ERROR, "DATABASE_ERROR", "Database operation failed"),
                )
            }

            #[cfg(feature = "cache")]
            Error::Redis(e) => {
                tracing::error!("Redis error: {}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ErrorResponse::with_code(StatusCode::INTERNAL_SERVER_ERROR, "CACHE_ERROR", "Cache operation failed"),
                )
            }

            #[cfg(feature = "events")]
            Error::Nats(e) => {
                tracing::error!("NATS error: {}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ErrorResponse::with_code(StatusCode::INTERNAL_SERVER_ERROR, "NATS_ERROR", "Event system error"),
                )
            }

            #[cfg(feature = "turso")]
            Error::Turso(e) => {
                tracing::error!("Turso error: {}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ErrorResponse::with_code(StatusCode::INTERNAL_SERVER_ERROR, "TURSO_ERROR", "Database operation failed"),
                )
            }

            Error::Jwt(e) => (
                StatusCode::UNAUTHORIZED,
                ErrorResponse::with_code(StatusCode::UNAUTHORIZED, "INVALID_TOKEN", e.to_string()),
            ),

            Error::Http(e) => (
                StatusCode::BAD_REQUEST,
                ErrorResponse::with_code(StatusCode::BAD_REQUEST, "HTTP_ERROR", e.to_string()),
            ),

            Error::Io(e) => {
                tracing::error!("I/O error: {}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ErrorResponse::with_code(StatusCode::INTERNAL_SERVER_ERROR, "IO_ERROR", "I/O operation failed"),
                )
            }

            Error::Unauthorized(msg) => (
                StatusCode::UNAUTHORIZED,
                ErrorResponse::with_code(StatusCode::UNAUTHORIZED, "UNAUTHORIZED", msg),
            ),

            Error::Forbidden(msg) => (
                StatusCode::FORBIDDEN,
                ErrorResponse::with_code(StatusCode::FORBIDDEN, "FORBIDDEN", msg),
            ),

            Error::NotFound(msg) => (
                StatusCode::NOT_FOUND,
                ErrorResponse::with_code(StatusCode::NOT_FOUND, "NOT_FOUND", msg),
            ),

            Error::BadRequest(msg) => (
                StatusCode::BAD_REQUEST,
                ErrorResponse::with_code(StatusCode::BAD_REQUEST, "BAD_REQUEST", msg),
            ),

            Error::RateLimitExceeded => (
                StatusCode::TOO_MANY_REQUESTS,
                ErrorResponse::with_code(StatusCode::TOO_MANY_REQUESTS, "RATE_LIMIT_EXCEEDED", "Too many requests"),
            ),

            Error::Conflict(msg) => (
                StatusCode::CONFLICT,
                ErrorResponse::with_code(StatusCode::CONFLICT, "CONFLICT", msg),
            ),

            Error::ValidationError(msg) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                ErrorResponse::with_code(StatusCode::UNPROCESSABLE_ENTITY, "VALIDATION_ERROR", msg),
            ),

            Error::Internal(msg) => {
                tracing::error!("Internal error: {}", msg);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ErrorResponse::with_code(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR", "Internal server error"),
                )
            }

            Error::Other(msg) => {
                tracing::error!("Unexpected error: {}", msg);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ErrorResponse::new(StatusCode::INTERNAL_SERVER_ERROR, "An unexpected error occurred"),
                )
            }
        };

        (status, Json(error_response)).into_response()
    }
}

impl fmt::Display for ErrorResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.error)
    }
}

// Manual From implementations for boxed errors
impl From<figment::Error> for Error {
    fn from(err: figment::Error) -> Self {
        Error::Config(Box::new(err))
    }
}

#[cfg(feature = "database")]
impl From<sqlx::Error> for Error {
    fn from(err: sqlx::Error) -> Self {
        Error::Database(Box::new(err))
    }
}

#[cfg(feature = "cache")]
impl From<redis::RedisError> for Error {
    fn from(err: redis::RedisError) -> Self {
        Error::Redis(Box::new(err))
    }
}

#[cfg(feature = "turso")]
impl From<libsql::Error> for Error {
    fn from(err: libsql::Error) -> Self {
        Error::Turso(err.to_string())
    }
}

impl From<jsonwebtoken::errors::Error> for Error {
    fn from(err: jsonwebtoken::errors::Error) -> Self {
        Error::Jwt(Box::new(err))
    }
}

impl From<axum::http::Error> for Error {
    fn from(err: axum::http::Error) -> Self {
        Error::Http(Box::new(err))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_response() {
        let err = ErrorResponse::new(StatusCode::NOT_FOUND, "User not found");
        assert_eq!(err.status, 404);
        assert_eq!(err.error, "User not found");
        assert!(err.code.is_none());
    }

    #[test]
    fn test_error_response_with_code() {
        let err = ErrorResponse::with_code(StatusCode::BAD_REQUEST, "INVALID_EMAIL", "Email format is invalid");
        assert_eq!(err.status, 400);
        assert_eq!(err.error, "Email format is invalid");
        assert_eq!(err.code, Some("INVALID_EMAIL".to_string()));
    }
}
