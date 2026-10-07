//! Driver-independent structured storage errors.
use std::fmt;
/// Database operation being performed when the error occurred
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DatabaseOperation {
    /// Establishing a database connection
    Connect,
    /// Executing a query
    Query,
    /// Inserting records
    Insert,
    /// Updating records
    Update,
    /// Deleting records
    Delete,
    /// Transaction operations (begin, commit, rollback)
    Transaction,
    /// Syncing data (e.g., Turso embedded replica sync)
    Sync,
    /// Running database migrations
    Migration,
    /// Acquiring a connection from the pool
    PoolAcquire,
}

impl fmt::Display for DatabaseOperation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connect => write!(f, "connect"),
            Self::Query => write!(f, "query"),
            Self::Insert => write!(f, "insert"),
            Self::Update => write!(f, "update"),
            Self::Delete => write!(f, "delete"),
            Self::Transaction => write!(f, "transaction"),
            Self::Sync => write!(f, "sync"),
            Self::Migration => write!(f, "migration"),
            Self::PoolAcquire => write!(f, "pool_acquire"),
        }
    }
}

/// Category of database error
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DatabaseErrorKind {
    /// Failed to establish connection
    ConnectionFailed,
    /// Record not found
    NotFound,
    /// Constraint violation (unique, foreign key, check)
    ConstraintViolation,
    /// Query execution failed
    QueryFailed,
    /// Transaction failed (begin, commit, or rollback)
    TransactionFailed,
    /// Type conversion error
    TypeConversion,
    /// Sync operation failed (Turso specific)
    SyncFailed,
    /// Configuration error
    Configuration,
    /// Operation timed out
    Timeout,
    /// Permission denied
    PermissionDenied,
    /// Connection pool exhausted
    PoolExhausted,
    /// Other/unknown error
    Other,
}

impl fmt::Display for DatabaseErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConnectionFailed => write!(f, "connection_failed"),
            Self::NotFound => write!(f, "not_found"),
            Self::ConstraintViolation => write!(f, "constraint_violation"),
            Self::QueryFailed => write!(f, "query_failed"),
            Self::TransactionFailed => write!(f, "transaction_failed"),
            Self::TypeConversion => write!(f, "type_conversion"),
            Self::SyncFailed => write!(f, "sync_failed"),
            Self::Configuration => write!(f, "configuration"),
            Self::Timeout => write!(f, "timeout"),
            Self::PermissionDenied => write!(f, "permission_denied"),
            Self::PoolExhausted => write!(f, "pool_exhausted"),
            Self::Other => write!(f, "other"),
        }
    }
}

/// Structured database error with operation context
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseError {
    /// The operation being performed when the error occurred
    pub operation: DatabaseOperation,
    /// The category of error
    pub kind: DatabaseErrorKind,
    /// Human-readable error message
    pub message: String,
    /// Additional context (e.g., table name, query fragment)
    pub context: Option<String>,
}

impl DatabaseError {
    /// Create a new database error
    pub fn new(
        operation: DatabaseOperation,
        kind: DatabaseErrorKind,
        message: impl Into<String>,
    ) -> Self {
        Self {
            operation,
            kind,
            message: message.into(),
            context: None,
        }
    }

    /// Create a new database error with context
    pub fn with_context(
        operation: DatabaseOperation,
        kind: DatabaseErrorKind,
        message: impl Into<String>,
        context: impl Into<String>,
    ) -> Self {
        Self {
            operation,
            kind,
            message: message.into(),
            context: Some(context.into()),
        }
    }

    /// Create a "not found" error
    pub fn not_found(operation: DatabaseOperation, message: impl Into<String>) -> Self {
        Self::new(operation, DatabaseErrorKind::NotFound, message)
    }

    /// Create a connection failed error
    pub fn connection_failed(message: impl Into<String>) -> Self {
        Self::new(
            DatabaseOperation::Connect,
            DatabaseErrorKind::ConnectionFailed,
            message,
        )
    }

    /// Create a constraint violation error
    pub fn constraint_violation(operation: DatabaseOperation, message: impl Into<String>) -> Self {
        Self::new(operation, DatabaseErrorKind::ConstraintViolation, message)
    }

    /// Create a query failed error
    pub fn query_failed(message: impl Into<String>) -> Self {
        Self::new(
            DatabaseOperation::Query,
            DatabaseErrorKind::QueryFailed,
            message,
        )
    }

    /// Create a timeout error
    pub fn timeout(operation: DatabaseOperation, message: impl Into<String>) -> Self {
        Self::new(operation, DatabaseErrorKind::Timeout, message)
    }

    /// Create a pool exhausted error
    pub fn pool_exhausted(message: impl Into<String>) -> Self {
        Self::new(
            DatabaseOperation::PoolAcquire,
            DatabaseErrorKind::PoolExhausted,
            message,
        )
    }

    /// Create a transaction failed error
    pub fn transaction_failed(message: impl Into<String>) -> Self {
        Self::new(
            DatabaseOperation::Transaction,
            DatabaseErrorKind::TransactionFailed,
            message,
        )
    }

    /// Create a sync failed error (Turso specific)
    pub fn sync_failed(message: impl Into<String>) -> Self {
        Self::new(
            DatabaseOperation::Sync,
            DatabaseErrorKind::SyncFailed,
            message,
        )
    }

    /// Check if this error is retriable (transient errors that may succeed on retry)
    pub fn is_retriable(&self) -> bool {
        matches!(
            self.kind,
            DatabaseErrorKind::ConnectionFailed
                | DatabaseErrorKind::Timeout
                | DatabaseErrorKind::PoolExhausted
                | DatabaseErrorKind::SyncFailed
        )
    }

    /// Add context to an existing error
    pub fn add_context(mut self, context: impl Into<String>) -> Self {
        self.context = Some(context.into());
        self
    }
}

impl fmt::Display for DatabaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Database {} error during {}: {}",
            self.kind, self.operation, self.message
        )?;
        if let Some(ref ctx) = self.context {
            write!(f, " [context: {}]", ctx)?;
        }
        Ok(())
    }
}

impl std::error::Error for DatabaseError {}

/// Sanitize a database URL by removing credentials
pub fn sanitize_url(url: &str) -> String {
    // Handle standard database URLs like postgres://user:pass@host/db
    if let Some(at_pos) = url.find('@') {
        if let Some(scheme_end) = url.find("://") {
            let scheme = &url[..scheme_end + 3];
            let after_at = &url[at_pos + 1..];
            return format!("{}<redacted>@{}", scheme, after_at);
        }
    }
    // Handle Turso URLs like libsql://db-org.turso.io?authToken=xxx
    if url.contains("authToken=") || url.contains("auth_token=") {
        let base = url.split('?').next().unwrap_or(url);
        return format!("{}?<credentials redacted>", base);
    }
    url.to_string()
}
/// Failure at the storage boundary.
#[derive(Debug)]
pub enum StorageError {
    /// Storage setup or operation failure.
    Internal(String),
    /// A structured database failure.
    Database(DatabaseError),
    /// An analytical database failure.
    ClickHouse(String),
    /// Operating system I/O failure.
    Io(std::io::Error),
}
impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Internal(message) => write!(f, "Internal server error: {message}"),
            Self::Database(error) => error.fmt(f),
            Self::ClickHouse(message) => write!(f, "ClickHouse error: {message}"),
            Self::Io(error) => write!(f, "I/O error: {error}"),
        }
    }
}
impl std::error::Error for StorageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}
impl From<std::io::Error> for StorageError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}
/// Result of a storage operation.
pub type Result<T> = std::result::Result<T, StorageError>;

impl From<DatabaseError> for StorageError {
    fn from(error: DatabaseError) -> Self {
        Self::Database(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_database_error_new() {
        let err = DatabaseError::new(
            DatabaseOperation::Query,
            DatabaseErrorKind::QueryFailed,
            "Query failed",
        );
        assert_eq!(err.operation, DatabaseOperation::Query);
        assert_eq!(err.kind, DatabaseErrorKind::QueryFailed);
        assert_eq!(err.message, "Query failed");
        assert!(err.context.is_none());
    }

    #[test]
    fn test_database_error_with_context() {
        let err = DatabaseError::with_context(
            DatabaseOperation::Insert,
            DatabaseErrorKind::ConstraintViolation,
            "Unique constraint violated",
            "users.email",
        );
        assert_eq!(err.operation, DatabaseOperation::Insert);
        assert_eq!(err.kind, DatabaseErrorKind::ConstraintViolation);
        assert_eq!(err.message, "Unique constraint violated");
        assert_eq!(err.context, Some("users.email".to_string()));
    }

    #[test]
    fn test_not_found_convenience() {
        let err = DatabaseError::not_found(DatabaseOperation::Query, "User not found");
        assert_eq!(err.operation, DatabaseOperation::Query);
        assert_eq!(err.kind, DatabaseErrorKind::NotFound);
        assert_eq!(err.message, "User not found");
    }

    #[test]
    fn test_connection_failed_convenience() {
        let err = DatabaseError::connection_failed("Connection refused");
        assert_eq!(err.operation, DatabaseOperation::Connect);
        assert_eq!(err.kind, DatabaseErrorKind::ConnectionFailed);
        assert_eq!(err.message, "Connection refused");
    }

    #[test]
    fn test_constraint_violation_convenience() {
        let err = DatabaseError::constraint_violation(DatabaseOperation::Update, "FK constraint");
        assert_eq!(err.operation, DatabaseOperation::Update);
        assert_eq!(err.kind, DatabaseErrorKind::ConstraintViolation);
    }

    #[test]
    fn test_query_failed_convenience() {
        let err = DatabaseError::query_failed("Syntax error");
        assert_eq!(err.operation, DatabaseOperation::Query);
        assert_eq!(err.kind, DatabaseErrorKind::QueryFailed);
    }

    #[test]
    fn test_timeout_convenience() {
        let err = DatabaseError::timeout(DatabaseOperation::Query, "Query timed out");
        assert_eq!(err.operation, DatabaseOperation::Query);
        assert_eq!(err.kind, DatabaseErrorKind::Timeout);
    }

    #[test]
    fn test_pool_exhausted_convenience() {
        let err = DatabaseError::pool_exhausted("No connections available");
        assert_eq!(err.operation, DatabaseOperation::PoolAcquire);
        assert_eq!(err.kind, DatabaseErrorKind::PoolExhausted);
    }

    #[test]
    fn test_transaction_failed_convenience() {
        let err = DatabaseError::transaction_failed("Commit failed");
        assert_eq!(err.operation, DatabaseOperation::Transaction);
        assert_eq!(err.kind, DatabaseErrorKind::TransactionFailed);
    }

    #[test]
    fn test_sync_failed_convenience() {
        let err = DatabaseError::sync_failed("Sync with remote failed");
        assert_eq!(err.operation, DatabaseOperation::Sync);
        assert_eq!(err.kind, DatabaseErrorKind::SyncFailed);
    }

    #[test]
    fn test_is_retriable_transient_errors() {
        // Transient errors should be retriable
        assert!(DatabaseError::connection_failed("refused").is_retriable());
        assert!(DatabaseError::timeout(DatabaseOperation::Query, "timeout").is_retriable());
        assert!(DatabaseError::pool_exhausted("exhausted").is_retriable());
        assert!(DatabaseError::sync_failed("sync error").is_retriable());
    }

    #[test]
    fn test_is_retriable_permanent_errors() {
        // Permanent errors should not be retriable
        assert!(!DatabaseError::not_found(DatabaseOperation::Query, "not found").is_retriable());
        assert!(
            !DatabaseError::constraint_violation(DatabaseOperation::Insert, "unique")
                .is_retriable()
        );
        assert!(!DatabaseError::query_failed("syntax error").is_retriable());
        assert!(!DatabaseError::transaction_failed("rollback").is_retriable());
        assert!(!DatabaseError::new(
            DatabaseOperation::Query,
            DatabaseErrorKind::PermissionDenied,
            "denied"
        )
        .is_retriable());
        assert!(!DatabaseError::new(
            DatabaseOperation::Query,
            DatabaseErrorKind::TypeConversion,
            "type error"
        )
        .is_retriable());
        assert!(!DatabaseError::new(
            DatabaseOperation::Connect,
            DatabaseErrorKind::Configuration,
            "bad config"
        )
        .is_retriable());
    }

    #[test]
    fn test_add_context() {
        let err = DatabaseError::query_failed("Query failed").add_context("SELECT * FROM users");
        assert_eq!(err.context, Some("SELECT * FROM users".to_string()));
    }

    #[test]
    fn test_display_formatting() {
        let err = DatabaseError::new(
            DatabaseOperation::Query,
            DatabaseErrorKind::QueryFailed,
            "Syntax error near 'FROM'",
        );
        let display = format!("{}", err);
        assert!(display.contains("query_failed"));
        assert!(display.contains("query"));
        assert!(display.contains("Syntax error near 'FROM'"));
    }

    #[test]
    fn test_display_formatting_with_context() {
        let err = DatabaseError::with_context(
            DatabaseOperation::Insert,
            DatabaseErrorKind::ConstraintViolation,
            "Unique constraint violated",
            "users.email",
        );
        let display = format!("{}", err);
        assert!(display.contains("[context: users.email]"));
    }

    #[test]
    fn test_database_operation_display() {
        assert_eq!(format!("{}", DatabaseOperation::Connect), "connect");
        assert_eq!(format!("{}", DatabaseOperation::Query), "query");
        assert_eq!(format!("{}", DatabaseOperation::Insert), "insert");
        assert_eq!(format!("{}", DatabaseOperation::Update), "update");
        assert_eq!(format!("{}", DatabaseOperation::Delete), "delete");
        assert_eq!(format!("{}", DatabaseOperation::Transaction), "transaction");
        assert_eq!(format!("{}", DatabaseOperation::Sync), "sync");
        assert_eq!(format!("{}", DatabaseOperation::Migration), "migration");
        assert_eq!(
            format!("{}", DatabaseOperation::PoolAcquire),
            "pool_acquire"
        );
    }

    #[test]
    fn test_database_error_kind_display() {
        assert_eq!(
            format!("{}", DatabaseErrorKind::ConnectionFailed),
            "connection_failed"
        );
        assert_eq!(format!("{}", DatabaseErrorKind::NotFound), "not_found");
        assert_eq!(
            format!("{}", DatabaseErrorKind::ConstraintViolation),
            "constraint_violation"
        );
        assert_eq!(
            format!("{}", DatabaseErrorKind::QueryFailed),
            "query_failed"
        );
        assert_eq!(
            format!("{}", DatabaseErrorKind::TransactionFailed),
            "transaction_failed"
        );
        assert_eq!(
            format!("{}", DatabaseErrorKind::TypeConversion),
            "type_conversion"
        );
        assert_eq!(format!("{}", DatabaseErrorKind::SyncFailed), "sync_failed");
        assert_eq!(
            format!("{}", DatabaseErrorKind::Configuration),
            "configuration"
        );
        assert_eq!(format!("{}", DatabaseErrorKind::Timeout), "timeout");
        assert_eq!(
            format!("{}", DatabaseErrorKind::PermissionDenied),
            "permission_denied"
        );
        assert_eq!(
            format!("{}", DatabaseErrorKind::PoolExhausted),
            "pool_exhausted"
        );
        assert_eq!(format!("{}", DatabaseErrorKind::Other), "other");
    }

    #[test]
    fn test_sanitize_url_postgres() {
        let url = "postgres://admin:secret123@localhost:5432/mydb";
        let sanitized = sanitize_url(url);
        assert_eq!(sanitized, "postgres://<redacted>@localhost:5432/mydb");
        assert!(!sanitized.contains("admin"));
        assert!(!sanitized.contains("secret123"));
    }

    #[test]
    fn test_sanitize_url_turso() {
        let url = "libsql://my-db-org.turso.io?authToken=eyJ0eXAi";
        let sanitized = sanitize_url(url);
        assert_eq!(
            sanitized,
            "libsql://my-db-org.turso.io?<credentials redacted>"
        );
        assert!(!sanitized.contains("eyJ0eXAi"));
    }

    #[test]
    fn test_sanitize_url_no_credentials() {
        let url = "libsql://localhost:8080";
        let sanitized = sanitize_url(url);
        assert_eq!(sanitized, "libsql://localhost:8080");
    }

    #[test]
    fn test_database_error_equality() {
        let err1 = DatabaseError::new(
            DatabaseOperation::Query,
            DatabaseErrorKind::NotFound,
            "Not found",
        );
        let err2 = DatabaseError::new(
            DatabaseOperation::Query,
            DatabaseErrorKind::NotFound,
            "Not found",
        );
        assert_eq!(err1, err2);
    }

    #[test]
    fn test_database_error_clone() {
        let err = DatabaseError::with_context(
            DatabaseOperation::Insert,
            DatabaseErrorKind::ConstraintViolation,
            "Duplicate key",
            "users.id",
        );
        let cloned = err.clone();
        assert_eq!(err, cloned);
    }
}
