//! Type-safe identifiers using the TypeID specification
//!
//! This module provides type-safe, prefix-enhanced identifiers for distributed systems.
//! Built on the [TypeID Specification](https://github.com/jetpack-io/typeid/blob/main/spec/SPEC.md),
//! these IDs combine the uniqueness of UUIDs with readability and type safety.
//!
//! # Request IDs
//!
//! Request IDs use UUIDv7 for time-sortability, making them ideal for distributed tracing:
//!
//! ```rust
//! use acton_service::ids::RequestId;
//!
//! let request_id = RequestId::new();
//! println!("Request ID: {}", request_id); // e.g., "req_01h455vb4pex5vsknk084sn02q"
//! ```
//!
//! # UUID Version Selection
//!
//! Different ID types use different UUID versions based on their use case:
//! - **V7** (time-sortable): Request IDs, Event IDs, Entity IDs - great for observability
//! - **V4** (random): Security-critical IDs where unpredictability is important

use mti::prelude::*;
use std::fmt;
use std::str::FromStr;
use tower_http::request_id::{MakeRequestId, RequestId as TowerRequestId};
use http::Request;

/// A type-safe request identifier for distributed tracing.
///
/// Uses UUIDv7 for time-sortability, making it ideal for:
/// - Distributed tracing across microservices
/// - Log correlation and analysis
/// - Request timing and ordering
///
/// # Format
///
/// Request IDs follow the TypeID format: `req_<base32-encoded-uuidv7>`
///
/// Example: `req_01h455vb4pex5vsknk084sn02q`
///
/// # Example
///
/// ```rust
/// use acton_service::ids::RequestId;
/// use std::str::FromStr;
///
/// // Create a new request ID
/// let id = RequestId::new();
/// assert!(id.as_str().starts_with("req_"));
///
/// // Parse an existing request ID
/// let parsed = RequestId::from_str("req_01h455vb4pex5vsknk084sn02q").unwrap();
/// assert_eq!(parsed.prefix(), "req");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RequestId(MagicTypeId);

impl RequestId {
    /// The prefix used for request IDs
    pub const PREFIX: &'static str = "req";

    /// Creates a new request ID with a UUIDv7 (time-sortable).
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::ids::RequestId;
    ///
    /// let id = RequestId::new();
    /// println!("New request: {}", id);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self(Self::PREFIX.create_type_id::<V7>())
    }

    /// Returns the request ID as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Returns the prefix portion of the ID.
    #[must_use]
    pub fn prefix(&self) -> &str {
        self.0.prefix().as_str()
    }

    /// Returns the underlying `MagicTypeId`.
    #[must_use]
    pub fn inner(&self) -> &MagicTypeId {
        &self.0
    }

    /// Converts the request ID into a `MagicTypeId`.
    #[must_use]
    pub fn into_inner(self) -> MagicTypeId {
        self.0
    }
}

impl Default for RequestId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for RequestId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for RequestId {
    type Err = RequestIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mti = MagicTypeId::from_str(s).map_err(RequestIdError::Parse)?;

        // Validate the prefix
        if mti.prefix().as_str() != Self::PREFIX {
            return Err(RequestIdError::InvalidPrefix {
                expected: Self::PREFIX.to_string(),
                actual: mti.prefix().as_str().to_string(),
            });
        }

        Ok(Self(mti))
    }
}

impl AsRef<str> for RequestId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl From<RequestId> for String {
    fn from(id: RequestId) -> Self {
        id.0.to_string()
    }
}

/// Error type for request ID parsing.
#[derive(Debug, thiserror::Error)]
pub enum RequestIdError {
    /// The ID could not be parsed as a valid TypeID.
    #[error("failed to parse request ID: {0}")]
    Parse(#[from] MagicTypeIdError),

    /// The prefix was not the expected value.
    #[error("invalid prefix: expected '{expected}', got '{actual}'")]
    InvalidPrefix {
        /// The expected prefix.
        expected: String,
        /// The actual prefix found.
        actual: String,
    },
}

/// A `MakeRequestId` implementation that generates `RequestId`s for tower-http.
///
/// This can be used with `tower_http::request_id::SetRequestIdLayer` to
/// automatically generate type-safe request IDs for incoming HTTP requests.
///
/// # Example
///
/// ```rust,ignore
/// use acton_service::ids::MakeTypedRequestId;
/// use tower_http::request_id::SetRequestIdLayer;
///
/// let layer = SetRequestIdLayer::new(MakeTypedRequestId::default());
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct MakeTypedRequestId;

impl MakeRequestId for MakeTypedRequestId {
    fn make_request_id<B>(&mut self, _request: &Request<B>) -> Option<TowerRequestId> {
        let id = RequestId::new();
        let header_value = http::HeaderValue::from_str(id.as_str()).ok()?;
        Some(TowerRequestId::new(header_value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_id_new() {
        let id = RequestId::new();
        assert!(id.as_str().starts_with("req_"));
        assert_eq!(id.prefix(), "req");
        // TypeID format: prefix (3) + underscore (1) + suffix (26) = 30
        assert_eq!(id.as_str().len(), 30);
    }

    #[test]
    fn test_request_id_parse() {
        let id_str = "req_01h455vb4pex5vsknk084sn02q";
        let id = RequestId::from_str(id_str).unwrap();
        assert_eq!(id.as_str(), id_str);
        assert_eq!(id.prefix(), "req");
    }

    #[test]
    fn test_request_id_invalid_prefix() {
        let id_str = "user_01h455vb4pex5vsknk084sn02q";
        let result = RequestId::from_str(id_str);
        assert!(result.is_err());

        match result.unwrap_err() {
            RequestIdError::InvalidPrefix { expected, actual } => {
                assert_eq!(expected, "req");
                assert_eq!(actual, "user");
            }
            _ => panic!("Expected InvalidPrefix error"),
        }
    }

    #[test]
    fn test_request_id_invalid_format() {
        let id_str = "req_invalid";
        let result = RequestId::from_str(id_str);
        assert!(result.is_err());
    }

    #[test]
    fn test_request_id_display() {
        let id = RequestId::new();
        let displayed = format!("{}", id);
        assert!(displayed.starts_with("req_"));
    }

    #[test]
    fn test_request_id_ordering() {
        let id1 = RequestId::new();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let id2 = RequestId::new();

        // UUIDv7 IDs should be time-ordered
        assert!(id1 < id2);
    }

    #[test]
    fn test_make_typed_request_id() {
        let mut maker = MakeTypedRequestId;
        let request = http::Request::builder()
            .body(())
            .unwrap();

        let id = maker.make_request_id(&request);
        assert!(id.is_some());

        let header_value = id.unwrap().into_header_value();
        let id_str = header_value.to_str().unwrap();
        assert!(id_str.starts_with("req_"));
    }
}
