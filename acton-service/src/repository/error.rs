//! Repository error types
//!
//! This module provides structured error types for repository operations,
//! allowing fine-grained error handling and meaningful error messages.
//!
//! # Example
//!
//! ```rust
//! use acton_service::repository::{RepositoryError, RepositoryOperation, RepositoryErrorKind};
//!
//! let error = RepositoryError::not_found("User", "usr_123");
//! assert!(matches!(error.kind, RepositoryErrorKind::NotFound));
//! assert!(error.entity_id.is_some());
//! ```

use std::fmt;

/// Operation being performed when the repository error occurred
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RepositoryOperation {
    /// Finding a single entity by ID
    FindById,
    /// Finding multiple entities with filters
    FindAll,
    /// Counting entities matching filters
    Count,
    /// Checking if an entity exists
    Exists,
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
    /// Batch loading related entities
    BatchLoad,
}

impl fmt::Display for RepositoryOperation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FindById => write!(f, "find_by_id"),
            Self::FindAll => write!(f, "find_all"),
            Self::Count => write!(f, "count"),
            Self::Exists => write!(f, "exists"),
            Self::Create => write!(f, "create"),
            Self::Update => write!(f, "update"),
            Self::Delete => write!(f, "delete"),
            Self::SoftDelete => write!(f, "soft_delete"),
            Self::Restore => write!(f, "restore"),
            Self::BatchLoad => write!(f, "batch_load"),
        }
    }
}

/// Category of repository error
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RepositoryErrorKind {
    /// Entity was not found
    NotFound,
    /// Entity already exists (duplicate key)
    AlreadyExists,
    /// Database constraint violation
    ConstraintViolation,
    /// Validation failed before database operation
    ValidationFailed,
    /// Failed to connect to database
    ConnectionFailed,
    /// Operation timed out
    Timeout,
    /// Underlying database error
    DatabaseError,
    /// Serialization or deserialization error
    SerializationError,
    /// Other unclassified error
    Other,
}

impl fmt::Display for RepositoryErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => write!(f, "not_found"),
            Self::AlreadyExists => write!(f, "already_exists"),
            Self::ConstraintViolation => write!(f, "constraint_violation"),
            Self::ValidationFailed => write!(f, "validation_failed"),
            Self::ConnectionFailed => write!(f, "connection_failed"),
            Self::Timeout => write!(f, "timeout"),
            Self::DatabaseError => write!(f, "database_error"),
            Self::SerializationError => write!(f, "serialization_error"),
            Self::Other => write!(f, "other"),
        }
    }
}

/// Structured repository error with operation context
///
/// Provides detailed information about what operation failed, why it failed,
/// and which entity was involved.
///
/// # Example
///
/// ```rust
/// use acton_service::repository::{RepositoryError, RepositoryOperation, RepositoryErrorKind};
///
/// // Create a not found error
/// let error = RepositoryError::not_found("User", "usr_abc123");
/// println!("{}", error); // "Repository not_found error during find_by_id: Entity not found [User: usr_abc123]"
///
/// // Check if the error is retriable
/// if error.is_retriable() {
///     // Retry the operation
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryError {
    /// The operation being performed when the error occurred
    pub operation: RepositoryOperation,
    /// The category of error
    pub kind: RepositoryErrorKind,
    /// Human-readable error message
    pub message: String,
    /// The type of entity involved (e.g., "User", "Order")
    pub entity_type: Option<String>,
    /// The ID of the entity involved
    pub entity_id: Option<String>,
}

impl RepositoryError {
    /// Create a new repository error
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::repository::{RepositoryError, RepositoryOperation, RepositoryErrorKind};
    ///
    /// let error = RepositoryError::new(
    ///     RepositoryOperation::Create,
    ///     RepositoryErrorKind::ConstraintViolation,
    ///     "Email already in use",
    /// );
    /// ```
    pub fn new(
        operation: RepositoryOperation,
        kind: RepositoryErrorKind,
        message: impl Into<String>,
    ) -> Self {
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
    /// use acton_service::repository::RepositoryError;
    ///
    /// let error = RepositoryError::not_found("User", "usr_123");
    /// assert_eq!(error.entity_type, Some("User".to_string()));
    /// ```
    pub fn not_found(entity_type: impl Into<String>, entity_id: impl Into<String>) -> Self {
        let entity_type = entity_type.into();
        let entity_id = entity_id.into();
        Self {
            operation: RepositoryOperation::FindById,
            kind: RepositoryErrorKind::NotFound,
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
    /// use acton_service::repository::RepositoryError;
    ///
    /// let error = RepositoryError::already_exists("User", "duplicate@example.com");
    /// ```
    pub fn already_exists(entity_type: impl Into<String>, identifier: impl Into<String>) -> Self {
        let entity_type = entity_type.into();
        let identifier = identifier.into();
        Self {
            operation: RepositoryOperation::Create,
            kind: RepositoryErrorKind::AlreadyExists,
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
    /// use acton_service::repository::RepositoryError;
    ///
    /// let error = RepositoryError::validation_failed("Email format is invalid");
    /// ```
    pub fn validation_failed(message: impl Into<String>) -> Self {
        Self {
            operation: RepositoryOperation::Create,
            kind: RepositoryErrorKind::ValidationFailed,
            message: message.into(),
            entity_type: None,
            entity_id: None,
        }
    }

    /// Create a constraint violation error
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::repository::{RepositoryError, RepositoryOperation};
    ///
    /// let error = RepositoryError::constraint_violation(
    ///     RepositoryOperation::Update,
    ///     "Foreign key constraint failed",
    /// );
    /// ```
    pub fn constraint_violation(
        operation: RepositoryOperation,
        message: impl Into<String>,
    ) -> Self {
        Self {
            operation,
            kind: RepositoryErrorKind::ConstraintViolation,
            message: message.into(),
            entity_type: None,
            entity_id: None,
        }
    }

    /// Create a connection failed error
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::repository::RepositoryError;
    ///
    /// let error = RepositoryError::connection_failed("Database connection refused");
    /// ```
    pub fn connection_failed(message: impl Into<String>) -> Self {
        Self {
            operation: RepositoryOperation::FindById,
            kind: RepositoryErrorKind::ConnectionFailed,
            message: message.into(),
            entity_type: None,
            entity_id: None,
        }
    }

    /// Create a timeout error
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::repository::{RepositoryError, RepositoryOperation};
    ///
    /// let error = RepositoryError::timeout(RepositoryOperation::FindAll, "Query timed out after 30s");
    /// ```
    pub fn timeout(operation: RepositoryOperation, message: impl Into<String>) -> Self {
        Self {
            operation,
            kind: RepositoryErrorKind::Timeout,
            message: message.into(),
            entity_type: None,
            entity_id: None,
        }
    }

    /// Create a database error
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::repository::{RepositoryError, RepositoryOperation};
    ///
    /// let error = RepositoryError::database_error(RepositoryOperation::Create, "Syntax error in query");
    /// ```
    pub fn database_error(operation: RepositoryOperation, message: impl Into<String>) -> Self {
        Self {
            operation,
            kind: RepositoryErrorKind::DatabaseError,
            message: message.into(),
            entity_type: None,
            entity_id: None,
        }
    }

    /// Create a serialization error
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::repository::{RepositoryError, RepositoryOperation};
    ///
    /// let error = RepositoryError::serialization_error(
    ///     RepositoryOperation::FindById,
    ///     "Failed to deserialize JSON field",
    /// );
    /// ```
    pub fn serialization_error(operation: RepositoryOperation, message: impl Into<String>) -> Self {
        Self {
            operation,
            kind: RepositoryErrorKind::SerializationError,
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
    /// use acton_service::repository::{RepositoryError, RepositoryOperation, RepositoryErrorKind};
    ///
    /// let error = RepositoryError::new(
    ///     RepositoryOperation::Update,
    ///     RepositoryErrorKind::NotFound,
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
    /// use acton_service::repository::{RepositoryError, RepositoryOperation};
    ///
    /// let error = RepositoryError::connection_failed("Connection refused")
    ///     .with_operation(RepositoryOperation::Create);
    /// ```
    #[must_use]
    pub fn with_operation(mut self, operation: RepositoryOperation) -> Self {
        self.operation = operation;
        self
    }

    /// Check if this error is retriable (transient errors that may succeed on retry)
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::repository::RepositoryError;
    ///
    /// let timeout_error = RepositoryError::connection_failed("Connection reset");
    /// assert!(timeout_error.is_retriable());
    ///
    /// let not_found_error = RepositoryError::not_found("User", "123");
    /// assert!(!not_found_error.is_retriable());
    /// ```
    pub fn is_retriable(&self) -> bool {
        matches!(
            self.kind,
            RepositoryErrorKind::ConnectionFailed | RepositoryErrorKind::Timeout
        )
    }
}

impl fmt::Display for RepositoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Repository {} error during {}: {}",
            self.kind, self.operation, self.message
        )?;
        if let (Some(ref entity_type), Some(ref entity_id)) = (&self.entity_type, &self.entity_id) {
            write!(f, " [{}: {}]", entity_type, entity_id)?;
        }
        Ok(())
    }
}

impl std::error::Error for RepositoryError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repository_operation_display() {
        assert_eq!(format!("{}", RepositoryOperation::FindById), "find_by_id");
        assert_eq!(format!("{}", RepositoryOperation::FindAll), "find_all");
        assert_eq!(format!("{}", RepositoryOperation::Count), "count");
        assert_eq!(format!("{}", RepositoryOperation::Exists), "exists");
        assert_eq!(format!("{}", RepositoryOperation::Create), "create");
        assert_eq!(format!("{}", RepositoryOperation::Update), "update");
        assert_eq!(format!("{}", RepositoryOperation::Delete), "delete");
        assert_eq!(
            format!("{}", RepositoryOperation::SoftDelete),
            "soft_delete"
        );
        assert_eq!(format!("{}", RepositoryOperation::Restore), "restore");
        assert_eq!(format!("{}", RepositoryOperation::BatchLoad), "batch_load");
    }

    #[test]
    fn test_repository_error_kind_display() {
        assert_eq!(format!("{}", RepositoryErrorKind::NotFound), "not_found");
        assert_eq!(
            format!("{}", RepositoryErrorKind::AlreadyExists),
            "already_exists"
        );
        assert_eq!(
            format!("{}", RepositoryErrorKind::ConstraintViolation),
            "constraint_violation"
        );
        assert_eq!(
            format!("{}", RepositoryErrorKind::ValidationFailed),
            "validation_failed"
        );
        assert_eq!(
            format!("{}", RepositoryErrorKind::ConnectionFailed),
            "connection_failed"
        );
        assert_eq!(format!("{}", RepositoryErrorKind::Timeout), "timeout");
        assert_eq!(
            format!("{}", RepositoryErrorKind::DatabaseError),
            "database_error"
        );
        assert_eq!(
            format!("{}", RepositoryErrorKind::SerializationError),
            "serialization_error"
        );
        assert_eq!(format!("{}", RepositoryErrorKind::Other), "other");
    }

    #[test]
    fn test_repository_error_new() {
        let error = RepositoryError::new(
            RepositoryOperation::FindAll,
            RepositoryErrorKind::DatabaseError,
            "Query failed",
        );
        assert_eq!(error.operation, RepositoryOperation::FindAll);
        assert_eq!(error.kind, RepositoryErrorKind::DatabaseError);
        assert_eq!(error.message, "Query failed");
        assert!(error.entity_type.is_none());
        assert!(error.entity_id.is_none());
    }

    #[test]
    fn test_not_found_convenience() {
        let error = RepositoryError::not_found("User", "usr_123");
        assert_eq!(error.operation, RepositoryOperation::FindById);
        assert_eq!(error.kind, RepositoryErrorKind::NotFound);
        assert_eq!(error.entity_type, Some("User".to_string()));
        assert_eq!(error.entity_id, Some("usr_123".to_string()));
    }

    #[test]
    fn test_already_exists_convenience() {
        let error = RepositoryError::already_exists("User", "user@example.com");
        assert_eq!(error.operation, RepositoryOperation::Create);
        assert_eq!(error.kind, RepositoryErrorKind::AlreadyExists);
        assert_eq!(error.entity_type, Some("User".to_string()));
        assert_eq!(error.entity_id, Some("user@example.com".to_string()));
    }

    #[test]
    fn test_validation_failed_convenience() {
        let error = RepositoryError::validation_failed("Email format invalid");
        assert_eq!(error.operation, RepositoryOperation::Create);
        assert_eq!(error.kind, RepositoryErrorKind::ValidationFailed);
        assert_eq!(error.message, "Email format invalid");
    }

    #[test]
    fn test_constraint_violation_convenience() {
        let error =
            RepositoryError::constraint_violation(RepositoryOperation::Update, "FK violation");
        assert_eq!(error.operation, RepositoryOperation::Update);
        assert_eq!(error.kind, RepositoryErrorKind::ConstraintViolation);
    }

    #[test]
    fn test_connection_failed_convenience() {
        let error = RepositoryError::connection_failed("Connection refused");
        assert_eq!(error.kind, RepositoryErrorKind::ConnectionFailed);
    }

    #[test]
    fn test_timeout_convenience() {
        let error = RepositoryError::timeout(RepositoryOperation::FindAll, "Query timed out");
        assert_eq!(error.operation, RepositoryOperation::FindAll);
        assert_eq!(error.kind, RepositoryErrorKind::Timeout);
    }

    #[test]
    fn test_database_error_convenience() {
        let error = RepositoryError::database_error(RepositoryOperation::Create, "Syntax error");
        assert_eq!(error.operation, RepositoryOperation::Create);
        assert_eq!(error.kind, RepositoryErrorKind::DatabaseError);
    }

    #[test]
    fn test_serialization_error_convenience() {
        let error =
            RepositoryError::serialization_error(RepositoryOperation::FindById, "JSON parse error");
        assert_eq!(error.operation, RepositoryOperation::FindById);
        assert_eq!(error.kind, RepositoryErrorKind::SerializationError);
    }

    #[test]
    fn test_with_entity() {
        let error = RepositoryError::new(
            RepositoryOperation::Update,
            RepositoryErrorKind::NotFound,
            "Not found",
        )
        .with_entity("Order", "ord_456");

        assert_eq!(error.entity_type, Some("Order".to_string()));
        assert_eq!(error.entity_id, Some("ord_456".to_string()));
    }

    #[test]
    fn test_with_operation() {
        let error = RepositoryError::connection_failed("Connection refused")
            .with_operation(RepositoryOperation::Create);

        assert_eq!(error.operation, RepositoryOperation::Create);
    }

    #[test]
    fn test_is_retriable_transient_errors() {
        assert!(RepositoryError::connection_failed("refused").is_retriable());
        assert!(RepositoryError::timeout(RepositoryOperation::FindAll, "timeout").is_retriable());
    }

    #[test]
    fn test_is_retriable_permanent_errors() {
        assert!(!RepositoryError::not_found("User", "123").is_retriable());
        assert!(!RepositoryError::already_exists("User", "email").is_retriable());
        assert!(!RepositoryError::validation_failed("invalid").is_retriable());
        assert!(
            !RepositoryError::constraint_violation(RepositoryOperation::Create, "fk")
                .is_retriable()
        );
        assert!(
            !RepositoryError::database_error(RepositoryOperation::Create, "syntax").is_retriable()
        );
        assert!(
            !RepositoryError::serialization_error(RepositoryOperation::FindById, "json")
                .is_retriable()
        );
    }

    #[test]
    fn test_display_without_entity() {
        let error = RepositoryError::new(
            RepositoryOperation::Create,
            RepositoryErrorKind::DatabaseError,
            "Query failed",
        );
        let display = format!("{}", error);
        assert!(display.contains("database_error"));
        assert!(display.contains("create"));
        assert!(display.contains("Query failed"));
        assert!(!display.contains("["));
    }

    #[test]
    fn test_display_with_entity() {
        let error = RepositoryError::not_found("User", "usr_123");
        let display = format!("{}", error);
        assert!(display.contains("not_found"));
        assert!(display.contains("find_by_id"));
        assert!(display.contains("[User: usr_123]"));
    }

    #[test]
    fn test_error_equality() {
        let err1 = RepositoryError::not_found("User", "123");
        let err2 = RepositoryError::not_found("User", "123");
        assert_eq!(err1, err2);
    }

    #[test]
    fn test_error_clone() {
        let err = RepositoryError::not_found("User", "123");
        let cloned = err.clone();
        assert_eq!(err, cloned);
    }

    #[test]
    fn test_error_is_error_trait() {
        let error: Box<dyn std::error::Error> = Box::new(RepositoryError::not_found("User", "123"));
        assert!(error.to_string().contains("not_found"));
    }
}
