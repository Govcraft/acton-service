//! Response types for REST handlers
//!
//! This module provides standardized response wrappers for REST API endpoints,
//! including single item responses and paginated list responses.
//!
//! # Example
//!
//! ```rust
//! use acton_service::handlers::{ItemResponse, ListResponse, PaginationMeta};
//! use serde::Serialize;
//!
//! #[derive(Serialize)]
//! struct User {
//!     id: u64,
//!     name: String,
//! }
//!
//! // Single item response
//! let user = User { id: 1, name: "Alice".to_string() };
//! let response = ItemResponse::new(user);
//!
//! // List response with pagination
//! let users = vec![User { id: 1, name: "Alice".to_string() }];
//! let pagination = PaginationMeta::new(1, 20, 1);
//! let response = ListResponse::new(users, pagination);
//! ```

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};

/// Metadata attached to API responses
///
/// Optional metadata that can be included with any response, providing
/// diagnostic information useful for debugging and monitoring.
///
/// # Example
///
/// ```rust
/// use acton_service::handlers::ResponseMeta;
///
/// let meta = ResponseMeta::default()
///     .with_request_id("req_abc123".to_string())
///     .with_processing_time(42);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ResponseMeta {
    /// The unique identifier for this request
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    /// Time taken to process the request in milliseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub processing_time_ms: Option<u64>,
}

impl ResponseMeta {
    /// Set the request ID
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::handlers::ResponseMeta;
    ///
    /// let meta = ResponseMeta::default().with_request_id("req_123".to_string());
    /// assert_eq!(meta.request_id, Some("req_123".to_string()));
    /// ```
    #[must_use]
    pub fn with_request_id(mut self, request_id: String) -> Self {
        self.request_id = Some(request_id);
        self
    }

    /// Set the processing time in milliseconds
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::handlers::ResponseMeta;
    ///
    /// let meta = ResponseMeta::default().with_processing_time(42);
    /// assert_eq!(meta.processing_time_ms, Some(42));
    /// ```
    #[must_use]
    pub fn with_processing_time(mut self, ms: u64) -> Self {
        self.processing_time_ms = Some(ms);
        self
    }

    /// Check if the metadata has any values set
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::handlers::ResponseMeta;
    ///
    /// let empty = ResponseMeta::default();
    /// assert!(empty.is_empty());
    ///
    /// let with_data = ResponseMeta::default().with_request_id("req_123".to_string());
    /// assert!(!with_data.is_empty());
    /// ```
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.request_id.is_none() && self.processing_time_ms.is_none()
    }
}

/// Single item response wrapper
///
/// Wraps a single entity response with optional metadata.
///
/// # Example
///
/// ```rust
/// use acton_service::handlers::{ItemResponse, ResponseMeta};
/// use serde::Serialize;
///
/// #[derive(Serialize)]
/// struct User { id: u64, name: String }
///
/// let user = User { id: 1, name: "Alice".to_string() };
/// let response = ItemResponse::new(user)
///     .with_meta(ResponseMeta::default().with_request_id("req_123".to_string()));
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemResponse<T> {
    /// The response data
    pub data: T,
    /// Optional response metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<ResponseMeta>,
}

impl<T> ItemResponse<T> {
    /// Create a new item response
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::handlers::ItemResponse;
    ///
    /// let response = ItemResponse::new("Hello, World!");
    /// assert_eq!(response.data, "Hello, World!");
    /// assert!(response.meta.is_none());
    /// ```
    pub fn new(data: T) -> Self {
        Self { data, meta: None }
    }

    /// Add metadata to the response
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::handlers::{ItemResponse, ResponseMeta};
    ///
    /// let response = ItemResponse::new("data")
    ///     .with_meta(ResponseMeta::default().with_request_id("req_123".to_string()));
    /// assert!(response.meta.is_some());
    /// ```
    #[must_use]
    pub fn with_meta(mut self, meta: ResponseMeta) -> Self {
        self.meta = Some(meta);
        self
    }

    /// Map the inner data to a new type
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::handlers::ItemResponse;
    ///
    /// let response = ItemResponse::new(42);
    /// let mapped = response.map(|n| n.to_string());
    /// assert_eq!(mapped.data, "42");
    /// ```
    pub fn map<U, F>(self, f: F) -> ItemResponse<U>
    where
        F: FnOnce(T) -> U,
    {
        ItemResponse {
            data: f(self.data),
            meta: self.meta,
        }
    }
}

impl<T: Serialize> IntoResponse for ItemResponse<T> {
    fn into_response(self) -> Response {
        (StatusCode::OK, Json(self)).into_response()
    }
}

/// Pagination metadata for list responses
///
/// Provides information about the current page and total results
/// for paginated list responses.
///
/// # Example
///
/// ```rust
/// use acton_service::handlers::PaginationMeta;
///
/// let pagination = PaginationMeta::new(1, 20, 100);
/// assert_eq!(pagination.page, 1);
/// assert_eq!(pagination.per_page, 20);
/// assert_eq!(pagination.total, 100);
/// assert_eq!(pagination.total_pages, 5);
/// assert!(pagination.has_next);
/// assert!(!pagination.has_prev);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PaginationMeta {
    /// Current page number (1-indexed)
    pub page: u32,
    /// Number of items per page
    pub per_page: u32,
    /// Total number of items across all pages
    pub total: u64,
    /// Total number of pages
    pub total_pages: u32,
    /// Whether there is a next page
    pub has_next: bool,
    /// Whether there is a previous page
    pub has_prev: bool,
}

impl PaginationMeta {
    /// Create new pagination metadata
    ///
    /// Automatically calculates `total_pages`, `has_next`, and `has_prev`.
    ///
    /// # Arguments
    ///
    /// * `page` - Current page number (1-indexed)
    /// * `per_page` - Number of items per page
    /// * `total` - Total number of items across all pages
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::handlers::PaginationMeta;
    ///
    /// let pagination = PaginationMeta::new(2, 20, 50);
    /// assert_eq!(pagination.total_pages, 3);
    /// assert!(pagination.has_next);
    /// assert!(pagination.has_prev);
    /// ```
    #[must_use]
    pub fn new(page: u32, per_page: u32, total: u64) -> Self {
        let per_page = if per_page == 0 { 1 } else { per_page };
        let total_pages = calculate_total_pages(total, per_page);
        let has_next = page < total_pages;
        let has_prev = page > 1;

        Self {
            page,
            per_page,
            total,
            total_pages,
            has_next,
            has_prev,
        }
    }

    /// Create pagination for an empty result set
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::handlers::PaginationMeta;
    ///
    /// let pagination = PaginationMeta::empty(20);
    /// assert_eq!(pagination.page, 1);
    /// assert_eq!(pagination.total, 0);
    /// assert!(!pagination.has_next);
    /// assert!(!pagination.has_prev);
    /// ```
    #[must_use]
    pub fn empty(per_page: u32) -> Self {
        Self::new(1, per_page, 0)
    }

    /// Get the offset for database queries
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::handlers::PaginationMeta;
    ///
    /// let pagination = PaginationMeta::new(3, 20, 100);
    /// assert_eq!(pagination.offset(), 40); // Skip first 2 pages
    /// ```
    #[must_use]
    pub fn offset(&self) -> u64 {
        u64::from(self.page.saturating_sub(1)) * u64::from(self.per_page)
    }
}

/// Calculate total pages, rounding up
fn calculate_total_pages(total: u64, per_page: u32) -> u32 {
    let per_page = u64::from(per_page);
    // Ceiling division: (total + per_page - 1) / per_page
    let pages = total.saturating_add(per_page).saturating_sub(1) / per_page;
    // Clamp to u32 max, but realistically this won't exceed u32
    pages.min(u64::from(u32::MAX)) as u32
}

/// List response with pagination
///
/// Wraps a collection of entities with pagination metadata and optional
/// response metadata.
///
/// # Example
///
/// ```rust
/// use acton_service::handlers::{ListResponse, PaginationMeta, ResponseMeta};
/// use serde::Serialize;
///
/// #[derive(Serialize)]
/// struct User { id: u64, name: String }
///
/// let users = vec![
///     User { id: 1, name: "Alice".to_string() },
///     User { id: 2, name: "Bob".to_string() },
/// ];
/// let pagination = PaginationMeta::new(1, 20, 2);
/// let response = ListResponse::new(users, pagination);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListResponse<T> {
    /// The list of items
    pub data: Vec<T>,
    /// Pagination metadata
    pub pagination: PaginationMeta,
    /// Optional response metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<ResponseMeta>,
}

impl<T> ListResponse<T> {
    /// Create a new list response
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::handlers::{ListResponse, PaginationMeta};
    ///
    /// let items = vec![1, 2, 3];
    /// let pagination = PaginationMeta::new(1, 20, 3);
    /// let response = ListResponse::new(items, pagination);
    /// assert_eq!(response.data.len(), 3);
    /// ```
    pub fn new(data: Vec<T>, pagination: PaginationMeta) -> Self {
        Self {
            data,
            pagination,
            meta: None,
        }
    }

    /// Create an empty list response
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::handlers::ListResponse;
    ///
    /// let response: ListResponse<String> = ListResponse::empty(20);
    /// assert!(response.data.is_empty());
    /// assert_eq!(response.pagination.total, 0);
    /// ```
    pub fn empty(per_page: u32) -> Self {
        Self {
            data: Vec::new(),
            pagination: PaginationMeta::empty(per_page),
            meta: None,
        }
    }

    /// Add metadata to the response
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::handlers::{ListResponse, PaginationMeta, ResponseMeta};
    ///
    /// let response = ListResponse::new(vec![1, 2, 3], PaginationMeta::new(1, 20, 3))
    ///     .with_meta(ResponseMeta::default().with_request_id("req_123".to_string()));
    /// assert!(response.meta.is_some());
    /// ```
    #[must_use]
    pub fn with_meta(mut self, meta: ResponseMeta) -> Self {
        self.meta = Some(meta);
        self
    }

    /// Map each item in the list to a new type
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::handlers::{ListResponse, PaginationMeta};
    ///
    /// let response = ListResponse::new(vec![1, 2, 3], PaginationMeta::new(1, 20, 3));
    /// let mapped = response.map(|n| n.to_string());
    /// assert_eq!(mapped.data, vec!["1", "2", "3"]);
    /// ```
    pub fn map<U, F>(self, f: F) -> ListResponse<U>
    where
        F: FnMut(T) -> U,
    {
        ListResponse {
            data: self.data.into_iter().map(f).collect(),
            pagination: self.pagination,
            meta: self.meta,
        }
    }

    /// Get the number of items in the current page
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::handlers::{ListResponse, PaginationMeta};
    ///
    /// let response = ListResponse::new(vec![1, 2, 3], PaginationMeta::new(1, 20, 100));
    /// assert_eq!(response.len(), 3);
    /// ```
    #[must_use]
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Check if the current page is empty
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::handlers::ListResponse;
    ///
    /// let response: ListResponse<String> = ListResponse::empty(20);
    /// assert!(response.is_empty());
    /// ```
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

impl<T: Serialize> IntoResponse for ListResponse<T> {
    fn into_response(self) -> Response {
        (StatusCode::OK, Json(self)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_response_meta_default() {
        let meta = ResponseMeta::default();
        assert!(meta.request_id.is_none());
        assert!(meta.processing_time_ms.is_none());
        assert!(meta.is_empty());
    }

    #[test]
    fn test_response_meta_with_request_id() {
        let meta = ResponseMeta::default().with_request_id("req_123".to_string());
        assert_eq!(meta.request_id, Some("req_123".to_string()));
        assert!(!meta.is_empty());
    }

    #[test]
    fn test_response_meta_with_processing_time() {
        let meta = ResponseMeta::default().with_processing_time(42);
        assert_eq!(meta.processing_time_ms, Some(42));
        assert!(!meta.is_empty());
    }

    #[test]
    fn test_response_meta_chained() {
        let meta = ResponseMeta::default()
            .with_request_id("req_123".to_string())
            .with_processing_time(42);
        assert_eq!(meta.request_id, Some("req_123".to_string()));
        assert_eq!(meta.processing_time_ms, Some(42));
    }

    #[test]
    fn test_item_response_new() {
        let response = ItemResponse::new("data");
        assert_eq!(response.data, "data");
        assert!(response.meta.is_none());
    }

    #[test]
    fn test_item_response_with_meta() {
        let response = ItemResponse::new("data")
            .with_meta(ResponseMeta::default().with_request_id("req_123".to_string()));
        assert_eq!(response.meta.unwrap().request_id, Some("req_123".to_string()));
    }

    #[test]
    fn test_item_response_map() {
        let response = ItemResponse::new(42);
        let mapped = response.map(|n| n.to_string());
        assert_eq!(mapped.data, "42");
    }

    #[test]
    fn test_item_response_map_preserves_meta() {
        let response = ItemResponse::new(42)
            .with_meta(ResponseMeta::default().with_request_id("req_123".to_string()));
        let mapped = response.map(|n| n.to_string());
        assert!(mapped.meta.is_some());
        assert_eq!(
            mapped.meta.unwrap().request_id,
            Some("req_123".to_string())
        );
    }

    #[test]
    fn test_pagination_meta_new() {
        let pagination = PaginationMeta::new(1, 20, 100);
        assert_eq!(pagination.page, 1);
        assert_eq!(pagination.per_page, 20);
        assert_eq!(pagination.total, 100);
        assert_eq!(pagination.total_pages, 5);
        assert!(pagination.has_next);
        assert!(!pagination.has_prev);
    }

    #[test]
    fn test_pagination_meta_middle_page() {
        let pagination = PaginationMeta::new(3, 20, 100);
        assert_eq!(pagination.page, 3);
        assert_eq!(pagination.total_pages, 5);
        assert!(pagination.has_next);
        assert!(pagination.has_prev);
    }

    #[test]
    fn test_pagination_meta_last_page() {
        let pagination = PaginationMeta::new(5, 20, 100);
        assert_eq!(pagination.page, 5);
        assert!(!pagination.has_next);
        assert!(pagination.has_prev);
    }

    #[test]
    fn test_pagination_meta_empty() {
        let pagination = PaginationMeta::empty(20);
        assert_eq!(pagination.page, 1);
        assert_eq!(pagination.per_page, 20);
        assert_eq!(pagination.total, 0);
        assert_eq!(pagination.total_pages, 0);
        assert!(!pagination.has_next);
        assert!(!pagination.has_prev);
    }

    #[test]
    fn test_pagination_meta_offset() {
        assert_eq!(PaginationMeta::new(1, 20, 100).offset(), 0);
        assert_eq!(PaginationMeta::new(2, 20, 100).offset(), 20);
        assert_eq!(PaginationMeta::new(3, 20, 100).offset(), 40);
    }

    #[test]
    fn test_pagination_meta_zero_per_page_protected() {
        // Should not panic or divide by zero
        let pagination = PaginationMeta::new(1, 0, 100);
        assert_eq!(pagination.per_page, 1); // Should default to 1
    }

    #[test]
    fn test_pagination_meta_partial_last_page() {
        // 45 items with 20 per page = 3 pages (20 + 20 + 5)
        let pagination = PaginationMeta::new(1, 20, 45);
        assert_eq!(pagination.total_pages, 3);
    }

    #[test]
    fn test_calculate_total_pages() {
        assert_eq!(calculate_total_pages(0, 20), 0);
        assert_eq!(calculate_total_pages(1, 20), 1);
        assert_eq!(calculate_total_pages(20, 20), 1);
        assert_eq!(calculate_total_pages(21, 20), 2);
        assert_eq!(calculate_total_pages(100, 20), 5);
        assert_eq!(calculate_total_pages(101, 20), 6);
    }

    #[test]
    fn test_list_response_new() {
        let items = vec![1, 2, 3];
        let pagination = PaginationMeta::new(1, 20, 3);
        let response = ListResponse::new(items, pagination);
        assert_eq!(response.data.len(), 3);
        assert_eq!(response.pagination.total, 3);
        assert!(response.meta.is_none());
    }

    #[test]
    fn test_list_response_empty() {
        let response: ListResponse<String> = ListResponse::empty(20);
        assert!(response.data.is_empty());
        assert_eq!(response.pagination.total, 0);
        assert_eq!(response.pagination.per_page, 20);
    }

    #[test]
    fn test_list_response_with_meta() {
        let response = ListResponse::new(vec![1, 2, 3], PaginationMeta::new(1, 20, 3))
            .with_meta(ResponseMeta::default().with_request_id("req_123".to_string()));
        assert!(response.meta.is_some());
    }

    #[test]
    fn test_list_response_map() {
        let response = ListResponse::new(vec![1, 2, 3], PaginationMeta::new(1, 20, 3));
        let mapped = response.map(|n| n.to_string());
        assert_eq!(mapped.data, vec!["1", "2", "3"]);
    }

    #[test]
    fn test_list_response_map_preserves_pagination() {
        let response = ListResponse::new(vec![1, 2, 3], PaginationMeta::new(2, 20, 100));
        let mapped = response.map(|n| n.to_string());
        assert_eq!(mapped.pagination.page, 2);
        assert_eq!(mapped.pagination.total, 100);
    }

    #[test]
    fn test_list_response_len() {
        let response = ListResponse::new(vec![1, 2, 3], PaginationMeta::new(1, 20, 100));
        assert_eq!(response.len(), 3);
    }

    #[test]
    fn test_list_response_is_empty() {
        let response: ListResponse<String> = ListResponse::empty(20);
        assert!(response.is_empty());

        let non_empty = ListResponse::new(vec![1], PaginationMeta::new(1, 20, 1));
        assert!(!non_empty.is_empty());
    }

    #[test]
    fn test_response_meta_clone() {
        let meta = ResponseMeta::default()
            .with_request_id("req_123".to_string())
            .with_processing_time(42);
        let cloned = meta.clone();
        assert_eq!(meta, cloned);
    }

    #[test]
    fn test_pagination_meta_clone() {
        let pagination = PaginationMeta::new(2, 20, 100);
        let cloned = pagination.clone();
        assert_eq!(pagination, cloned);
    }

    #[test]
    fn test_item_response_clone() {
        let response = ItemResponse::new("data".to_string());
        let cloned = response.clone();
        assert_eq!(cloned.data, "data");
    }

    #[test]
    fn test_list_response_clone() {
        let response = ListResponse::new(vec![1, 2, 3], PaginationMeta::new(1, 20, 3));
        let cloned = response.clone();
        assert_eq!(cloned.data, vec![1, 2, 3]);
    }
}
