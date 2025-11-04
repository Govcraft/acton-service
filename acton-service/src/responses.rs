//! HTTP response builders with correct status codes
//!
//! This module provides type-safe response builders for common HTTP patterns,
//! ensuring proper status code usage according to RFC 9110 (HTTP Semantics).
//!
//! ## Status Codes Provided
//!
//! - **200 OK** - Standard successful response
//! - **201 Created** - Resource successfully created (POST)
//! - **204 No Content** - Successful operation with no response body (DELETE, PUT)
//! - **409 Conflict** - Resource conflict (duplicate creation)
//! - **422 Unprocessable Entity** - Validation errors
//!
//! ## Example Usage
//!
//! ```rust,ignore
//! use acton_service::responses::{Created, NoContent, Conflict, ValidationError};
//! use acton_service::prelude::*;
//!
//! // 201 Created
//! async fn create_user() -> Created<UserResponse> {
//!     let user = UserResponse { id: 1, name: "Alice".to_string() };
//!     Created::new(user).with_location("/users/1")
//! }
//!
//! // 204 No Content
//! async fn delete_user() -> NoContent {
//!     // Delete logic...
//!     NoContent
//! }
//!
//! // 409 Conflict
//! async fn create_duplicate() -> Result<Created<UserResponse>, Conflict> {
//!     Err(Conflict::new("User with this email already exists"))
//! }
//!
//! // 422 Validation Error
//! async fn create_invalid() -> Result<Created<UserResponse>, ValidationError> {
//!     let mut errors = ValidationError::new("Validation failed");
//!     errors.add_field_error("email", "INVALID_FORMAT", "Invalid email format");
//!     Err(errors)
//! }
//! ```

use axum::{
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ============================================================================
// 201 Created
// ============================================================================

/// HTTP 201 Created response
///
/// Used when a new resource has been successfully created (typically for POST requests).
/// Optionally includes a `Location` header pointing to the new resource.
///
/// # Example
///
/// ```rust,ignore
/// use acton_service::responses::Created;
/// use acton_service::prelude::*;
///
/// #[derive(Serialize)]
/// struct User {
///     id: u64,
///     name: String,
/// }
///
/// async fn create_user() -> Created<User> {
///     let user = User { id: 123, name: "Alice".to_string() };
///     Created::new(user).with_location("/users/123")
/// }
/// ```
#[derive(Debug)]
pub struct Created<T> {
    data: T,
    location: Option<String>,
}

impl<T> Created<T> {
    /// Create a new 201 Created response
    pub fn new(data: T) -> Self {
        Self {
            data,
            location: None,
        }
    }

    /// Add a Location header pointing to the created resource
    pub fn with_location(mut self, location: impl Into<String>) -> Self {
        self.location = Some(location.into());
        self
    }
}

impl<T: Serialize> IntoResponse for Created<T> {
    fn into_response(self) -> Response {
        let mut response = (StatusCode::CREATED, Json(&self.data)).into_response();

        // Add Location header if provided
        if let Some(location) = self.location {
            if let Ok(header_value) = HeaderValue::from_str(&location) {
                response.headers_mut().insert(header::LOCATION, header_value);
            }
        }

        response
    }
}

// ============================================================================
// 204 No Content
// ============================================================================

/// HTTP 204 No Content response
///
/// Used when an operation succeeds but there's no response body to return.
/// Common for DELETE operations or PUT operations that don't return the updated resource.
///
/// # Example
///
/// ```rust,ignore
/// use acton_service::responses::NoContent;
/// use acton_service::prelude::*;
///
/// async fn delete_user(Path(id): Path<u64>) -> NoContent {
///     // Delete user logic...
///     NoContent
/// }
/// ```
#[derive(Debug, Clone, Copy)]
pub struct NoContent;

impl IntoResponse for NoContent {
    fn into_response(self) -> Response {
        StatusCode::NO_CONTENT.into_response()
    }
}

// ============================================================================
// 409 Conflict
// ============================================================================

/// HTTP 409 Conflict response
///
/// Used when a request conflicts with the current state of the server.
/// Common examples:
/// - Attempting to create a resource that already exists
/// - Concurrent modification conflicts
/// - Business rule violations
///
/// # Example
///
/// ```rust,ignore
/// use acton_service::responses::{Created, Conflict};
/// use acton_service::prelude::*;
///
/// async fn create_user() -> Result<Created<User>, Conflict> {
///     // Check if user exists
///     if user_exists {
///         return Err(Conflict::new("User with this email already exists")
///             .with_code("DUPLICATE_EMAIL")
///             .with_detail("A user with email alice@example.com is already registered"));
///     }
///     Ok(Created::new(user))
/// }
/// ```
#[derive(Debug, Serialize, Deserialize)]
pub struct Conflict {
    error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    status: u16,
}

impl Conflict {
    /// Create a new 409 Conflict response
    pub fn new(error: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            code: None,
            detail: None,
            status: StatusCode::CONFLICT.as_u16(),
        }
    }

    /// Add an error code
    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }

    /// Add detailed information
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

impl IntoResponse for Conflict {
    fn into_response(self) -> Response {
        (StatusCode::CONFLICT, Json(self)).into_response()
    }
}

// ============================================================================
// 422 Unprocessable Entity (Validation Errors)
// ============================================================================

/// Field-level validation error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldError {
    /// Field name
    pub field: String,
    /// Error code (e.g., "REQUIRED", "INVALID_FORMAT", "TOO_SHORT")
    pub code: String,
    /// Human-readable error message
    pub message: String,
}

/// HTTP 422 Unprocessable Entity response
///
/// Used when the request is well-formed but contains semantic errors
/// (validation failures). Provides field-level error details.
///
/// # Example
///
/// ```rust,ignore
/// use acton_service::responses::{Created, ValidationError};
/// use acton_service::prelude::*;
///
/// async fn create_user(Json(payload): Json<CreateUserRequest>) -> Result<Created<User>, ValidationError> {
///     let mut errors = ValidationError::new("Validation failed");
///
///     if payload.email.is_empty() {
///         errors.add_field_error("email", "REQUIRED", "Email is required");
///     }
///
///     if !payload.email.contains('@') {
///         errors.add_field_error("email", "INVALID_FORMAT", "Invalid email format");
///     }
///
///     if payload.password.len() < 8 {
///         errors.add_field_error("password", "TOO_SHORT", "Password must be at least 8 characters");
///     }
///
///     if errors.has_errors() {
///         return Err(errors);
///     }
///
///     Ok(Created::new(user))
/// }
/// ```
#[derive(Debug, Serialize, Deserialize)]
pub struct ValidationError {
    error: String,
    code: String,
    status: u16,
    /// Field-level validation errors
    pub errors: HashMap<String, Vec<FieldError>>,
}

impl ValidationError {
    /// Create a new validation error response
    pub fn new(error: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            code: "VALIDATION_ERROR".to_string(),
            status: StatusCode::UNPROCESSABLE_ENTITY.as_u16(),
            errors: HashMap::new(),
        }
    }

    /// Add a field-level error
    pub fn add_field_error(
        &mut self,
        field: impl Into<String>,
        code: impl Into<String>,
        message: impl Into<String>,
    ) {
        let field = field.into();
        let error = FieldError {
            field: field.clone(),
            code: code.into(),
            message: message.into(),
        };

        self.errors
            .entry(field)
            .or_default()
            .push(error);
    }

    /// Check if there are any validation errors
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Get the number of field errors
    pub fn error_count(&self) -> usize {
        self.errors.values().map(|v| v.len()).sum()
    }
}

impl IntoResponse for ValidationError {
    fn into_response(self) -> Response {
        (StatusCode::UNPROCESSABLE_ENTITY, Json(self)).into_response()
    }
}

// ============================================================================
// Success Response Wrapper
// ============================================================================

/// Standard success response wrapper (200 OK)
///
/// Provides a consistent response format for successful operations.
///
/// # Example
///
/// ```rust,ignore
/// use acton_service::responses::Success;
/// use acton_service::prelude::*;
///
/// async fn get_user() -> Success<User> {
///     let user = User { id: 1, name: "Alice".to_string() };
///     Success::new(user).with_message("User retrieved successfully")
/// }
/// ```
#[derive(Debug, Serialize, Deserialize)]
pub struct Success<T> {
    data: T,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

impl<T> Success<T> {
    /// Create a new success response
    pub fn new(data: T) -> Self {
        Self {
            data,
            message: None,
        }
    }

    /// Add an optional success message
    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }
}

impl<T: Serialize> IntoResponse for Success<T> {
    fn into_response(self) -> Response {
        (StatusCode::OK, Json(self)).into_response()
    }
}

// ============================================================================
// Accepted Response (202)
// ============================================================================

/// HTTP 202 Accepted response
///
/// Used when a request has been accepted for processing but processing has not completed.
/// Common for async operations, background jobs, or long-running tasks.
///
/// # Example
///
/// ```rust,ignore
/// use acton_service::responses::Accepted;
/// use acton_service::prelude::*;
///
/// async fn process_report() -> Accepted {
///     // Queue the report generation
///     Accepted::new()
///         .with_message("Report generation started")
///         .with_status_url("/reports/123/status")
/// }
/// ```
#[derive(Debug, Serialize, Deserialize)]
pub struct Accepted {
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    status_url: Option<String>,
    status: u16,
}

impl Accepted {
    /// Create a new 202 Accepted response
    pub fn new() -> Self {
        Self {
            message: "Request accepted for processing".to_string(),
            status_url: None,
            status: StatusCode::ACCEPTED.as_u16(),
        }
    }

    /// Set a custom message
    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = message.into();
        self
    }

    /// Add a URL where the client can check the status
    pub fn with_status_url(mut self, url: impl Into<String>) -> Self {
        self.status_url = Some(url.into());
        self
    }
}

impl Default for Accepted {
    fn default() -> Self {
        Self::new()
    }
}

impl IntoResponse for Accepted {
    fn into_response(self) -> Response {
        (StatusCode::ACCEPTED, Json(self)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize, Deserialize)]
    struct TestData {
        id: u64,
        name: String,
    }

    #[test]
    fn test_created_response() {
        let data = TestData {
            id: 1,
            name: "Test".to_string(),
        };
        let response = Created::new(data).with_location("/test/1");
        let response = response.into_response();
        assert_eq!(response.status(), StatusCode::CREATED);
    }

    #[test]
    fn test_no_content_response() {
        let response = NoContent.into_response();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    #[test]
    fn test_conflict_response() {
        let conflict = Conflict::new("Resource already exists")
            .with_code("DUPLICATE")
            .with_detail("A resource with this ID already exists");
        assert_eq!(conflict.status, 409);
        assert_eq!(conflict.code, Some("DUPLICATE".to_string()));
    }

    #[test]
    fn test_validation_error() {
        let mut error = ValidationError::new("Validation failed");
        error.add_field_error("email", "REQUIRED", "Email is required");
        error.add_field_error("email", "INVALID_FORMAT", "Invalid email format");
        error.add_field_error("password", "TOO_SHORT", "Password too short");

        assert!(error.has_errors());
        assert_eq!(error.error_count(), 3);
        assert_eq!(error.errors.get("email").unwrap().len(), 2);
        assert_eq!(error.errors.get("password").unwrap().len(), 1);
    }

    #[test]
    fn test_success_response() {
        let data = TestData {
            id: 1,
            name: "Test".to_string(),
        };
        let success = Success::new(data).with_message("Operation successful");
        assert_eq!(success.message, Some("Operation successful".to_string()));
    }

    #[test]
    fn test_accepted_response() {
        let accepted = Accepted::new()
            .with_message("Processing started")
            .with_status_url("/status/123");
        assert_eq!(accepted.status, 202);
        assert_eq!(accepted.status_url, Some("/status/123".to_string()));
    }
}
