//! API error types for handler operations
//!
//! This module provides structured error types for REST handler operations,
//! with automatic HTTP status code mapping via `IntoResponse`.
//!
//! # Example
//!
//! ```rust
//! use acton_service::handlers::{ApiError, ApiOperation, ApiErrorKind};
//!
//! let error = ApiError::not_found("User", "usr_123");
//! assert!(matches!(error.kind, ApiErrorKind::NotFound));
//! assert_eq!(error.entity_id, Some("usr_123".to_string()));
//! ```

use std::fmt;

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};

use crate::repository::{RepositoryError, RepositoryErrorKind, RepositoryOperation};

/// Operation being performed when the API error occurred
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ApiOperation {
    /// Listing entities
    List,
    /// Getting a single entity by ID
    Get,
    /// Creating a new entity
    Create,
    /// Updating an existing entity
    Update,
    /// Deleting an entity (hard delete)
    Delete,
    /// Soft deleting an entity
    SoftDelete,
    /// Restoring a soft-deleted entity
    Restore,
    /// Listing entities including soft-deleted ones
    ListWithDeleted,
}

impl fmt::Display for ApiOperation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::List => write!(f, "list"),
            Self::Get => write!(f, "get"),
            Self::Create => write!(f, "create"),
            Self::Update => write!(f, "update"),
            Self::Delete => write!(f, "delete"),
            Self::SoftDelete => write!(f, "soft_delete"),
            Self::Restore => write!(f, "restore"),
            Self::ListWithDeleted => write!(f, "list_with_deleted"),
        }
    }
}

/// Category of API error
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ApiErrorKind {
    /// Entity was not found
    NotFound,
    /// Entity already exists
    AlreadyExists,
    /// Request validation failed
    ValidationFailed,
    /// Authentication required
    Unauthorized,
    /// Access denied
    Forbidden,
    /// Invalid request format or parameters
    BadRequest,
    /// Operation conflicts with current state
    Conflict,
    /// Internal server error
    InternalError,
    /// Service temporarily unavailable
    ServiceUnavailable,
}

impl fmt::Display for ApiErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => write!(f, "not_found"),
            Self::AlreadyExists => write!(f, "already_exists"),
            Self::ValidationFailed => write!(f, "validation_failed"),
            Self::Unauthorized => write!(f, "unauthorized"),
            Self::Forbidden => write!(f, "forbidden"),
            Self::BadRequest => write!(f, "bad_request"),
            Self::Conflict => write!(f, "conflict"),
            Self::InternalError => write!(f, "internal_error"),
            Self::ServiceUnavailable => write!(f, "service_unavailable"),
        }
    }
}

impl ApiErrorKind {
    /// Get the HTTP status code for this error kind
    #[must_use]
    pub const fn status_code(&self) -> StatusCode {
        match self {
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::AlreadyExists | Self::Conflict => StatusCode::CONFLICT,
            Self::ValidationFailed => StatusCode::UNPROCESSABLE_ENTITY,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::BadRequest => StatusCode::BAD_REQUEST,
            Self::InternalError => StatusCode::INTERNAL_SERVER_ERROR,
            Self::ServiceUnavailable => StatusCode::SERVICE_UNAVAILABLE,
        }
    }

    /// Get the error code string for this error kind
    #[must_use]
    pub fn error_code(&self) -> String {
        format!("{}", self).to_uppercase()
    }
}

/// Structured API error with operation context
///
/// Provides detailed information about what operation failed, why it failed,
/// and which entity was involved.
///
/// # Example
///
/// ```rust
/// use acton_service::handlers::{ApiError, ApiOperation, ApiErrorKind};
///
/// // Create a not found error
/// let error = ApiError::not_found("User", "usr_abc123");
/// println!("{}", error); // "API not_found error during get: Entity not found [User: usr_abc123]"
///
/// // Check if the error is retriable
/// if error.is_retriable() {
///     // Retry the operation
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    /// The operation being performed when the error occurred
    pub operation: ApiOperation,
    /// The category of error
    pub kind: ApiErrorKind,
    /// Human-readable error message
    pub message: String,
    /// The type of entity involved (e.g., "User", "Order")
    pub entity_type: Option<String>,
    /// The ID of the entity involved
    pub entity_id: Option<String>,
}

impl ApiError {
    /// Create a new API error
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::handlers::{ApiError, ApiOperation, ApiErrorKind};
    ///
    /// let error = ApiError::new(
    ///     ApiOperation::Create,
    ///     ApiErrorKind::Conflict,
    ///     "Email already in use",
    /// );
    /// ```
    pub fn new(operation: ApiOperation, kind: ApiErrorKind, message: impl Into<String>) -> Self {
        Self {
            operation,
            kind,
            message: message.into(),
            entity_type: None,
            entity_id: None,
        }
    }

    /// Create a "not found" error with entity context
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::handlers::ApiError;
    ///
    /// let error = ApiError::not_found("User", "usr_123");
    /// assert_eq!(error.entity_type, Some("User".to_string()));
    /// ```
    pub fn not_found(entity_type: impl Into<String>, entity_id: impl Into<String>) -> Self {
        let entity_type = entity_type.into();
        let entity_id = entity_id.into();
        Self {
            operation: ApiOperation::Get,
            kind: ApiErrorKind::NotFound,
            message: "Entity not found".to_string(),
            entity_type: Some(entity_type),
            entity_id: Some(entity_id),
        }
    }

    /// Create an "already exists" error with entity context
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::handlers::ApiError;
    ///
    /// let error = ApiError::already_exists("User", "duplicate@example.com");
    /// ```
    pub fn already_exists(entity_type: impl Into<String>, identifier: impl Into<String>) -> Self {
        let entity_type = entity_type.into();
        let identifier = identifier.into();
        Self {
            operation: ApiOperation::Create,
            kind: ApiErrorKind::AlreadyExists,
            message: "Entity already exists".to_string(),
            entity_type: Some(entity_type),
            entity_id: Some(identifier),
        }
    }

    /// Create a validation failed error
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::handlers::ApiError;
    ///
    /// let error = ApiError::validation_failed("Email format is invalid");
    /// ```
    pub fn validation_failed(message: impl Into<String>) -> Self {
        Self {
            operation: ApiOperation::Create,
            kind: ApiErrorKind::ValidationFailed,
            message: message.into(),
            entity_type: None,
            entity_id: None,
        }
    }

    /// Create a bad request error
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::handlers::ApiError;
    ///
    /// let error = ApiError::bad_request("Invalid query parameter 'page'");
    /// ```
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self {
            operation: ApiOperation::List,
            kind: ApiErrorKind::BadRequest,
            message: message.into(),
            entity_type: None,
            entity_id: None,
        }
    }

    /// Create an unauthorized error
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::handlers::ApiError;
    ///
    /// let error = ApiError::unauthorized("Authentication required");
    /// ```
    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            operation: ApiOperation::Get,
            kind: ApiErrorKind::Unauthorized,
            message: message.into(),
            entity_type: None,
            entity_id: None,
        }
    }

    /// Create a forbidden error
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::handlers::ApiError;
    ///
    /// let error = ApiError::forbidden("Access denied to this resource");
    /// ```
    pub fn forbidden(message: impl Into<String>) -> Self {
        Self {
            operation: ApiOperation::Get,
            kind: ApiErrorKind::Forbidden,
            message: message.into(),
            entity_type: None,
            entity_id: None,
        }
    }

    /// Create a conflict error
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::handlers::{ApiError, ApiOperation};
    ///
    /// let error = ApiError::conflict(ApiOperation::Update, "Concurrent modification detected");
    /// ```
    pub fn conflict(operation: ApiOperation, message: impl Into<String>) -> Self {
        Self {
            operation,
            kind: ApiErrorKind::Conflict,
            message: message.into(),
            entity_type: None,
            entity_id: None,
        }
    }

    /// Create an internal error
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::handlers::ApiError;
    ///
    /// let error = ApiError::internal("Unexpected error occurred");
    /// ```
    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            operation: ApiOperation::Get,
            kind: ApiErrorKind::InternalError,
            message: message.into(),
            entity_type: None,
            entity_id: None,
        }
    }

    /// Create a service unavailable error
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::handlers::ApiError;
    ///
    /// let error = ApiError::service_unavailable("Database connection failed");
    /// ```
    pub fn service_unavailable(message: impl Into<String>) -> Self {
        Self {
            operation: ApiOperation::Get,
            kind: ApiErrorKind::ServiceUnavailable,
            message: message.into(),
            entity_type: None,
            entity_id: None,
        }
    }

    /// Add entity context to an existing error
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::handlers::{ApiError, ApiOperation, ApiErrorKind};
    ///
    /// let error = ApiError::new(
    ///     ApiOperation::Update,
    ///     ApiErrorKind::NotFound,
    ///     "Entity not found",
    /// ).with_entity("Order", "ord_456");
    /// ```
    #[must_use]
    pub fn with_entity(
        mut self,
        entity_type: impl Into<String>,
        entity_id: impl Into<String>,
    ) -> Self {
        self.entity_type = Some(entity_type.into());
        self.entity_id = Some(entity_id.into());
        self
    }

    /// Set the operation that caused the error
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::handlers::{ApiError, ApiOperation};
    ///
    /// let error = ApiError::service_unavailable("Connection refused")
    ///     .with_operation(ApiOperation::Create);
    /// ```
    #[must_use]
    pub fn with_operation(mut self, operation: ApiOperation) -> Self {
        self.operation = operation;
        self
    }

    /// Check if this error is retriable (transient errors that may succeed on retry)
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::handlers::ApiError;
    ///
    /// let unavailable_error = ApiError::service_unavailable("Connection reset");
    /// assert!(unavailable_error.is_retriable());
    ///
    /// let not_found_error = ApiError::not_found("User", "123");
    /// assert!(!not_found_error.is_retriable());
    /// ```
    pub fn is_retriable(&self) -> bool {
        matches!(self.kind, ApiErrorKind::ServiceUnavailable)
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "API {} error during {}: {}",
            self.kind, self.operation, self.message
        )?;
        if let (Some(ref entity_type), Some(ref entity_id)) = (&self.entity_type, &self.entity_id) {
            write!(f, " [{}: {}]", entity_type, entity_id)?;
        }
        Ok(())
    }
}

impl std::error::Error for ApiError {}

/// Response body for API errors
#[derive(Debug, Serialize, Deserialize)]
struct ApiErrorResponse {
    error: String,
    code: String,
    status: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    operation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    entity_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    entity_id: Option<String>,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.kind.status_code();
        let code = self.kind.error_code();

        // Log the error with structured context
        tracing::error!(
            operation = %self.operation,
            kind = %self.kind,
            entity_type = ?self.entity_type,
            entity_id = ?self.entity_id,
            retriable = self.is_retriable(),
            "API error: {}", self.message
        );

        let response = ApiErrorResponse {
            error: self.message,
            code,
            status: status.as_u16(),
            operation: Some(self.operation.to_string()),
            entity_type: self.entity_type,
            entity_id: self.entity_id,
        };

        (status, Json(response)).into_response()
    }
}

/// Convert RepositoryOperation to ApiOperation
fn repository_operation_to_api_operation(op: RepositoryOperation) -> ApiOperation {
    match op {
        RepositoryOperation::FindById => ApiOperation::Get,
        RepositoryOperation::FindAll => ApiOperation::List,
        RepositoryOperation::Count => ApiOperation::List,
        RepositoryOperation::Exists => ApiOperation::Get,
        RepositoryOperation::Create => ApiOperation::Create,
        RepositoryOperation::Update => ApiOperation::Update,
        RepositoryOperation::Delete => ApiOperation::Delete,
        RepositoryOperation::SoftDelete => ApiOperation::SoftDelete,
        RepositoryOperation::Restore => ApiOperation::Restore,
        RepositoryOperation::BatchLoad => ApiOperation::List,
    }
}

impl From<RepositoryError> for ApiError {
    fn from(err: RepositoryError) -> Self {
        let operation = repository_operation_to_api_operation(err.operation);

        let kind = match err.kind {
            RepositoryErrorKind::NotFound => ApiErrorKind::NotFound,
            RepositoryErrorKind::AlreadyExists => ApiErrorKind::AlreadyExists,
            RepositoryErrorKind::ConstraintViolation => ApiErrorKind::Conflict,
            RepositoryErrorKind::ValidationFailed => ApiErrorKind::ValidationFailed,
            RepositoryErrorKind::ConnectionFailed | RepositoryErrorKind::Timeout => {
                ApiErrorKind::ServiceUnavailable
            }
            RepositoryErrorKind::DatabaseError
            | RepositoryErrorKind::SerializationError
            | RepositoryErrorKind::Other => ApiErrorKind::InternalError,
        };

        // User-facing message (don't expose internal details for internal errors)
        let message = match kind {
            ApiErrorKind::InternalError | ApiErrorKind::ServiceUnavailable => {
                match kind {
                    ApiErrorKind::ServiceUnavailable => "Service temporarily unavailable",
                    _ => "An internal error occurred",
                }
                .to_string()
            }
            _ => err.message,
        };

        Self {
            operation,
            kind,
            message,
            entity_type: err.entity_type,
            entity_id: err.entity_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_operation_display() {
        assert_eq!(format!("{}", ApiOperation::List), "list");
        assert_eq!(format!("{}", ApiOperation::Get), "get");
        assert_eq!(format!("{}", ApiOperation::Create), "create");
        assert_eq!(format!("{}", ApiOperation::Update), "update");
        assert_eq!(format!("{}", ApiOperation::Delete), "delete");
        assert_eq!(format!("{}", ApiOperation::SoftDelete), "soft_delete");
        assert_eq!(format!("{}", ApiOperation::Restore), "restore");
        assert_eq!(format!("{}", ApiOperation::ListWithDeleted), "list_with_deleted");
    }

    #[test]
    fn test_api_error_kind_display() {
        assert_eq!(format!("{}", ApiErrorKind::NotFound), "not_found");
        assert_eq!(format!("{}", ApiErrorKind::AlreadyExists), "already_exists");
        assert_eq!(format!("{}", ApiErrorKind::ValidationFailed), "validation_failed");
        assert_eq!(format!("{}", ApiErrorKind::Unauthorized), "unauthorized");
        assert_eq!(format!("{}", ApiErrorKind::Forbidden), "forbidden");
        assert_eq!(format!("{}", ApiErrorKind::BadRequest), "bad_request");
        assert_eq!(format!("{}", ApiErrorKind::Conflict), "conflict");
        assert_eq!(format!("{}", ApiErrorKind::InternalError), "internal_error");
        assert_eq!(format!("{}", ApiErrorKind::ServiceUnavailable), "service_unavailable");
    }

    #[test]
    fn test_api_error_kind_status_codes() {
        assert_eq!(ApiErrorKind::NotFound.status_code(), StatusCode::NOT_FOUND);
        assert_eq!(ApiErrorKind::AlreadyExists.status_code(), StatusCode::CONFLICT);
        assert_eq!(
            ApiErrorKind::ValidationFailed.status_code(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
        assert_eq!(ApiErrorKind::Unauthorized.status_code(), StatusCode::UNAUTHORIZED);
        assert_eq!(ApiErrorKind::Forbidden.status_code(), StatusCode::FORBIDDEN);
        assert_eq!(ApiErrorKind::BadRequest.status_code(), StatusCode::BAD_REQUEST);
        assert_eq!(ApiErrorKind::Conflict.status_code(), StatusCode::CONFLICT);
        assert_eq!(
            ApiErrorKind::InternalError.status_code(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(
            ApiErrorKind::ServiceUnavailable.status_code(),
            StatusCode::SERVICE_UNAVAILABLE
        );
    }

    #[test]
    fn test_api_error_kind_error_codes() {
        assert_eq!(ApiErrorKind::NotFound.error_code(), "NOT_FOUND");
        assert_eq!(ApiErrorKind::AlreadyExists.error_code(), "ALREADY_EXISTS");
        assert_eq!(ApiErrorKind::ValidationFailed.error_code(), "VALIDATION_FAILED");
        assert_eq!(ApiErrorKind::Unauthorized.error_code(), "UNAUTHORIZED");
        assert_eq!(ApiErrorKind::Forbidden.error_code(), "FORBIDDEN");
        assert_eq!(ApiErrorKind::BadRequest.error_code(), "BAD_REQUEST");
        assert_eq!(ApiErrorKind::Conflict.error_code(), "CONFLICT");
        assert_eq!(ApiErrorKind::InternalError.error_code(), "INTERNAL_ERROR");
        assert_eq!(ApiErrorKind::ServiceUnavailable.error_code(), "SERVICE_UNAVAILABLE");
    }

    #[test]
    fn test_api_error_new() {
        let error = ApiError::new(
            ApiOperation::Create,
            ApiErrorKind::Conflict,
            "Email already in use",
        );
        assert_eq!(error.operation, ApiOperation::Create);
        assert_eq!(error.kind, ApiErrorKind::Conflict);
        assert_eq!(error.message, "Email already in use");
        assert!(error.entity_type.is_none());
        assert!(error.entity_id.is_none());
    }

    #[test]
    fn test_not_found_convenience() {
        let error = ApiError::not_found("User", "usr_123");
        assert_eq!(error.operation, ApiOperation::Get);
        assert_eq!(error.kind, ApiErrorKind::NotFound);
        assert_eq!(error.entity_type, Some("User".to_string()));
        assert_eq!(error.entity_id, Some("usr_123".to_string()));
    }

    #[test]
    fn test_already_exists_convenience() {
        let error = ApiError::already_exists("User", "user@example.com");
        assert_eq!(error.operation, ApiOperation::Create);
        assert_eq!(error.kind, ApiErrorKind::AlreadyExists);
        assert_eq!(error.entity_type, Some("User".to_string()));
        assert_eq!(error.entity_id, Some("user@example.com".to_string()));
    }

    #[test]
    fn test_validation_failed_convenience() {
        let error = ApiError::validation_failed("Email format invalid");
        assert_eq!(error.operation, ApiOperation::Create);
        assert_eq!(error.kind, ApiErrorKind::ValidationFailed);
        assert_eq!(error.message, "Email format invalid");
    }

    #[test]
    fn test_bad_request_convenience() {
        let error = ApiError::bad_request("Invalid page parameter");
        assert_eq!(error.operation, ApiOperation::List);
        assert_eq!(error.kind, ApiErrorKind::BadRequest);
    }

    #[test]
    fn test_unauthorized_convenience() {
        let error = ApiError::unauthorized("Token expired");
        assert_eq!(error.kind, ApiErrorKind::Unauthorized);
    }

    #[test]
    fn test_forbidden_convenience() {
        let error = ApiError::forbidden("Admin access required");
        assert_eq!(error.kind, ApiErrorKind::Forbidden);
    }

    #[test]
    fn test_conflict_convenience() {
        let error = ApiError::conflict(ApiOperation::Update, "Concurrent modification");
        assert_eq!(error.operation, ApiOperation::Update);
        assert_eq!(error.kind, ApiErrorKind::Conflict);
    }

    #[test]
    fn test_internal_convenience() {
        let error = ApiError::internal("Unexpected error");
        assert_eq!(error.kind, ApiErrorKind::InternalError);
    }

    #[test]
    fn test_service_unavailable_convenience() {
        let error = ApiError::service_unavailable("Database down");
        assert_eq!(error.kind, ApiErrorKind::ServiceUnavailable);
    }

    #[test]
    fn test_with_entity() {
        let error = ApiError::new(ApiOperation::Update, ApiErrorKind::NotFound, "Not found")
            .with_entity("Order", "ord_456");

        assert_eq!(error.entity_type, Some("Order".to_string()));
        assert_eq!(error.entity_id, Some("ord_456".to_string()));
    }

    #[test]
    fn test_with_operation() {
        let error =
            ApiError::service_unavailable("Connection refused").with_operation(ApiOperation::Create);

        assert_eq!(error.operation, ApiOperation::Create);
    }

    #[test]
    fn test_is_retriable_transient_errors() {
        assert!(ApiError::service_unavailable("unavailable").is_retriable());
    }

    #[test]
    fn test_is_retriable_permanent_errors() {
        assert!(!ApiError::not_found("User", "123").is_retriable());
        assert!(!ApiError::already_exists("User", "email").is_retriable());
        assert!(!ApiError::validation_failed("invalid").is_retriable());
        assert!(!ApiError::bad_request("bad").is_retriable());
        assert!(!ApiError::unauthorized("unauth").is_retriable());
        assert!(!ApiError::forbidden("forbidden").is_retriable());
        assert!(!ApiError::conflict(ApiOperation::Update, "conflict").is_retriable());
        assert!(!ApiError::internal("internal").is_retriable());
    }

    #[test]
    fn test_display_without_entity() {
        let error = ApiError::new(
            ApiOperation::Create,
            ApiErrorKind::ValidationFailed,
            "Validation failed",
        );
        let display = format!("{}", error);
        assert!(display.contains("validation_failed"));
        assert!(display.contains("create"));
        assert!(display.contains("Validation failed"));
        assert!(!display.contains("["));
    }

    #[test]
    fn test_display_with_entity() {
        let error = ApiError::not_found("User", "usr_123");
        let display = format!("{}", error);
        assert!(display.contains("not_found"));
        assert!(display.contains("get"));
        assert!(display.contains("[User: usr_123]"));
    }

    #[test]
    fn test_error_equality() {
        let err1 = ApiError::not_found("User", "123");
        let err2 = ApiError::not_found("User", "123");
        assert_eq!(err1, err2);
    }

    #[test]
    fn test_error_clone() {
        let err = ApiError::not_found("User", "123");
        let cloned = err.clone();
        assert_eq!(err, cloned);
    }

    #[test]
    fn test_error_is_error_trait() {
        let error: Box<dyn std::error::Error> = Box::new(ApiError::not_found("User", "123"));
        assert!(error.to_string().contains("not_found"));
    }

    #[test]
    fn test_from_repository_error_not_found() {
        let repo_err = RepositoryError::not_found("User", "usr_123");
        let api_err: ApiError = repo_err.into();

        assert_eq!(api_err.operation, ApiOperation::Get);
        assert_eq!(api_err.kind, ApiErrorKind::NotFound);
        assert_eq!(api_err.entity_type, Some("User".to_string()));
        assert_eq!(api_err.entity_id, Some("usr_123".to_string()));
    }

    #[test]
    fn test_from_repository_error_already_exists() {
        let repo_err = RepositoryError::already_exists("User", "email@example.com");
        let api_err: ApiError = repo_err.into();

        assert_eq!(api_err.operation, ApiOperation::Create);
        assert_eq!(api_err.kind, ApiErrorKind::AlreadyExists);
    }

    #[test]
    fn test_from_repository_error_constraint_violation() {
        let repo_err =
            RepositoryError::constraint_violation(RepositoryOperation::Update, "FK violation");
        let api_err: ApiError = repo_err.into();

        assert_eq!(api_err.operation, ApiOperation::Update);
        assert_eq!(api_err.kind, ApiErrorKind::Conflict);
    }

    #[test]
    fn test_from_repository_error_validation_failed() {
        let repo_err = RepositoryError::validation_failed("Invalid email");
        let api_err: ApiError = repo_err.into();

        assert_eq!(api_err.kind, ApiErrorKind::ValidationFailed);
    }

    #[test]
    fn test_from_repository_error_connection_failed() {
        let repo_err = RepositoryError::connection_failed("Connection refused");
        let api_err: ApiError = repo_err.into();

        assert_eq!(api_err.kind, ApiErrorKind::ServiceUnavailable);
        // Should hide internal error details
        assert_eq!(api_err.message, "Service temporarily unavailable");
    }

    #[test]
    fn test_from_repository_error_timeout() {
        let repo_err = RepositoryError::timeout(RepositoryOperation::FindAll, "Query timed out");
        let api_err: ApiError = repo_err.into();

        assert_eq!(api_err.kind, ApiErrorKind::ServiceUnavailable);
    }

    #[test]
    fn test_from_repository_error_database_error() {
        let repo_err = RepositoryError::database_error(RepositoryOperation::Create, "Syntax error");
        let api_err: ApiError = repo_err.into();

        assert_eq!(api_err.kind, ApiErrorKind::InternalError);
        // Should hide internal error details
        assert_eq!(api_err.message, "An internal error occurred");
    }

    #[test]
    fn test_from_repository_error_serialization_error() {
        let repo_err =
            RepositoryError::serialization_error(RepositoryOperation::FindById, "JSON parse error");
        let api_err: ApiError = repo_err.into();

        assert_eq!(api_err.kind, ApiErrorKind::InternalError);
    }
}
