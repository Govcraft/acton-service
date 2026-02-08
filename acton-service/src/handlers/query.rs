//! Query types for list operations
//!
//! This module provides types for controlling list operation parameters,
//! including pagination, sorting, searching, and filtering.
//!
//! # Example
//!
//! ```rust
//! use acton_service::handlers::{ListQuery, SortOrder};
//!
//! let query = ListQuery::default()
//!     .with_page(2)
//!     .with_per_page(50)
//!     .with_sort("created_at".to_string())
//!     .with_order(SortOrder::Desc);
//!
//! assert_eq!(query.page_number(), 2);
//! assert_eq!(query.items_per_page(), 50);
//! assert_eq!(query.offset(), 50);
//! ```

use std::fmt;

use serde::{Deserialize, Serialize};

/// Default number of items per page
pub const DEFAULT_PER_PAGE: u32 = 20;

/// Maximum allowed items per page
pub const MAX_PER_PAGE: u32 = 100;

/// Sort direction for list queries
///
/// # Example
///
/// ```rust
/// use acton_service::handlers::SortOrder;
///
/// let asc = SortOrder::Asc;
/// let desc = SortOrder::Desc;
///
/// assert_eq!(format!("{}", asc), "asc");
/// assert_eq!(format!("{}", desc), "desc");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortOrder {
    /// Sort in ascending order (A-Z, 0-9, oldest first)
    #[default]
    Asc,
    /// Sort in descending order (Z-A, 9-0, newest first)
    Desc,
}

impl fmt::Display for SortOrder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Asc => write!(f, "asc"),
            Self::Desc => write!(f, "desc"),
        }
    }
}

impl SortOrder {
    /// Convert to SQL ORDER BY clause fragment
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::handlers::SortOrder;
    ///
    /// assert_eq!(SortOrder::Asc.as_sql(), "ASC");
    /// assert_eq!(SortOrder::Desc.as_sql(), "DESC");
    /// ```
    #[must_use]
    pub const fn as_sql(&self) -> &'static str {
        match self {
            Self::Asc => "ASC",
            Self::Desc => "DESC",
        }
    }
}

/// Query parameters for list operations
///
/// Provides pagination, sorting, searching, and filtering capabilities
/// for list endpoints.
///
/// # Example
///
/// ```rust
/// use acton_service::handlers::{ListQuery, SortOrder};
///
/// // Using builder pattern
/// let query = ListQuery::default()
///     .with_page(1)
///     .with_per_page(20)
///     .with_sort("name".to_string())
///     .with_order(SortOrder::Asc)
///     .with_search("alice".to_string())
///     .with_filter("status=active".to_string());
///
/// // Access computed values
/// assert_eq!(query.page_number(), 1);
/// assert_eq!(query.items_per_page(), 20);
/// assert_eq!(query.offset(), 0);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ListQuery {
    /// Page number (1-indexed). None defaults to 1.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<u32>,

    /// Number of items per page. None defaults to DEFAULT_PER_PAGE.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub per_page: Option<u32>,

    /// Field name to sort by
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort: Option<String>,

    /// Sort direction (asc or desc)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<SortOrder>,

    /// Search query string
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search: Option<String>,

    /// Filter expressions (e.g., "status=active", "age__gte=18")
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filter: Vec<String>,
}

impl ListQuery {
    /// Create a new empty query
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::handlers::ListQuery;
    ///
    /// let query = ListQuery::new();
    /// assert_eq!(query.page_number(), 1);
    /// assert_eq!(query.items_per_page(), 20);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the page number
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::handlers::ListQuery;
    ///
    /// let query = ListQuery::new().with_page(3);
    /// assert_eq!(query.page_number(), 3);
    /// ```
    #[must_use]
    pub fn with_page(mut self, page: u32) -> Self {
        self.page = Some(page);
        self
    }

    /// Set the number of items per page
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::handlers::ListQuery;
    ///
    /// let query = ListQuery::new().with_per_page(50);
    /// assert_eq!(query.items_per_page(), 50);
    /// ```
    #[must_use]
    pub fn with_per_page(mut self, per_page: u32) -> Self {
        self.per_page = Some(per_page);
        self
    }

    /// Set the sort field
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::handlers::ListQuery;
    ///
    /// let query = ListQuery::new().with_sort("created_at".to_string());
    /// assert_eq!(query.sort, Some("created_at".to_string()));
    /// ```
    #[must_use]
    pub fn with_sort(mut self, sort: String) -> Self {
        self.sort = Some(sort);
        self
    }

    /// Set the sort order
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::handlers::{ListQuery, SortOrder};
    ///
    /// let query = ListQuery::new().with_order(SortOrder::Desc);
    /// assert_eq!(query.order, Some(SortOrder::Desc));
    /// ```
    #[must_use]
    pub fn with_order(mut self, order: SortOrder) -> Self {
        self.order = Some(order);
        self
    }

    /// Set the search query
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::handlers::ListQuery;
    ///
    /// let query = ListQuery::new().with_search("alice".to_string());
    /// assert_eq!(query.search, Some("alice".to_string()));
    /// ```
    #[must_use]
    pub fn with_search(mut self, search: String) -> Self {
        self.search = Some(search);
        self
    }

    /// Add a filter expression
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::handlers::ListQuery;
    ///
    /// let query = ListQuery::new()
    ///     .with_filter("status=active".to_string())
    ///     .with_filter("role=admin".to_string());
    /// assert_eq!(query.filter.len(), 2);
    /// ```
    #[must_use]
    pub fn with_filter(mut self, filter: String) -> Self {
        self.filter.push(filter);
        self
    }

    /// Set multiple filters at once
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::handlers::ListQuery;
    ///
    /// let query = ListQuery::new().with_filters(vec![
    ///     "status=active".to_string(),
    ///     "role=admin".to_string(),
    /// ]);
    /// assert_eq!(query.filter.len(), 2);
    /// ```
    #[must_use]
    pub fn with_filters(mut self, filters: Vec<String>) -> Self {
        self.filter = filters;
        self
    }

    /// Get the 1-indexed page number, defaulting to 1
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::handlers::ListQuery;
    ///
    /// let query = ListQuery::new();
    /// assert_eq!(query.page_number(), 1);
    ///
    /// let query = ListQuery::new().with_page(5);
    /// assert_eq!(query.page_number(), 5);
    ///
    /// // Page 0 is treated as page 1
    /// let query = ListQuery::new().with_page(0);
    /// assert_eq!(query.page_number(), 1);
    /// ```
    #[must_use]
    pub fn page_number(&self) -> u32 {
        self.page.unwrap_or(1).max(1)
    }

    /// Get the number of items per page, with defaults and limits applied
    ///
    /// Returns a value between 1 and MAX_PER_PAGE, defaulting to DEFAULT_PER_PAGE.
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::handlers::ListQuery;
    ///
    /// let query = ListQuery::new();
    /// assert_eq!(query.items_per_page(), 20); // default
    ///
    /// let query = ListQuery::new().with_per_page(50);
    /// assert_eq!(query.items_per_page(), 50);
    ///
    /// // Capped at MAX_PER_PAGE (100)
    /// let query = ListQuery::new().with_per_page(500);
    /// assert_eq!(query.items_per_page(), 100);
    ///
    /// // Minimum of 1
    /// let query = ListQuery::new().with_per_page(0);
    /// assert_eq!(query.items_per_page(), 1);
    /// ```
    #[must_use]
    pub fn items_per_page(&self) -> u32 {
        self.per_page
            .unwrap_or(DEFAULT_PER_PAGE)
            .clamp(1, MAX_PER_PAGE)
    }

    /// Get the offset for database queries
    ///
    /// Calculates (page - 1) * per_page to skip the appropriate number of items.
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::handlers::ListQuery;
    ///
    /// let query = ListQuery::new(); // page 1, 20 per page
    /// assert_eq!(query.offset(), 0);
    ///
    /// let query = ListQuery::new().with_page(2).with_per_page(20);
    /// assert_eq!(query.offset(), 20);
    ///
    /// let query = ListQuery::new().with_page(3).with_per_page(50);
    /// assert_eq!(query.offset(), 100);
    /// ```
    #[must_use]
    pub fn offset(&self) -> u64 {
        u64::from(self.page_number().saturating_sub(1)) * u64::from(self.items_per_page())
    }

    /// Get the sort order, defaulting to ascending if not specified
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::handlers::{ListQuery, SortOrder};
    ///
    /// let query = ListQuery::new();
    /// assert_eq!(query.sort_order(), SortOrder::Asc);
    ///
    /// let query = ListQuery::new().with_order(SortOrder::Desc);
    /// assert_eq!(query.sort_order(), SortOrder::Desc);
    /// ```
    #[must_use]
    pub fn sort_order(&self) -> SortOrder {
        self.order.unwrap_or_default()
    }

    /// Check if a search query is present
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::handlers::ListQuery;
    ///
    /// let query = ListQuery::new();
    /// assert!(!query.has_search());
    ///
    /// let query = ListQuery::new().with_search("alice".to_string());
    /// assert!(query.has_search());
    ///
    /// // Empty search is not considered a search
    /// let query = ListQuery::new().with_search("".to_string());
    /// assert!(!query.has_search());
    /// ```
    #[must_use]
    pub fn has_search(&self) -> bool {
        self.search.as_ref().is_some_and(|s| !s.is_empty())
    }

    /// Check if any filters are present
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::handlers::ListQuery;
    ///
    /// let query = ListQuery::new();
    /// assert!(!query.has_filters());
    ///
    /// let query = ListQuery::new().with_filter("status=active".to_string());
    /// assert!(query.has_filters());
    /// ```
    #[must_use]
    pub fn has_filters(&self) -> bool {
        !self.filter.is_empty()
    }

    /// Check if sorting is specified
    ///
    /// # Example
    ///
    /// ```rust
    /// use acton_service::handlers::ListQuery;
    ///
    /// let query = ListQuery::new();
    /// assert!(!query.has_sort());
    ///
    /// let query = ListQuery::new().with_sort("name".to_string());
    /// assert!(query.has_sort());
    /// ```
    #[must_use]
    pub fn has_sort(&self) -> bool {
        self.sort.as_ref().is_some_and(|s| !s.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sort_order_display() {
        assert_eq!(format!("{}", SortOrder::Asc), "asc");
        assert_eq!(format!("{}", SortOrder::Desc), "desc");
    }

    #[test]
    fn test_sort_order_default() {
        assert_eq!(SortOrder::default(), SortOrder::Asc);
    }

    #[test]
    fn test_sort_order_as_sql() {
        assert_eq!(SortOrder::Asc.as_sql(), "ASC");
        assert_eq!(SortOrder::Desc.as_sql(), "DESC");
    }

    #[test]
    fn test_list_query_default() {
        let query = ListQuery::default();
        assert!(query.page.is_none());
        assert!(query.per_page.is_none());
        assert!(query.sort.is_none());
        assert!(query.order.is_none());
        assert!(query.search.is_none());
        assert!(query.filter.is_empty());
    }

    #[test]
    fn test_list_query_new() {
        let query = ListQuery::new();
        assert_eq!(query, ListQuery::default());
    }

    #[test]
    fn test_list_query_with_page() {
        let query = ListQuery::new().with_page(5);
        assert_eq!(query.page, Some(5));
        assert_eq!(query.page_number(), 5);
    }

    #[test]
    fn test_list_query_with_per_page() {
        let query = ListQuery::new().with_per_page(50);
        assert_eq!(query.per_page, Some(50));
        assert_eq!(query.items_per_page(), 50);
    }

    #[test]
    fn test_list_query_with_sort() {
        let query = ListQuery::new().with_sort("name".to_string());
        assert_eq!(query.sort, Some("name".to_string()));
        assert!(query.has_sort());
    }

    #[test]
    fn test_list_query_with_order() {
        let query = ListQuery::new().with_order(SortOrder::Desc);
        assert_eq!(query.order, Some(SortOrder::Desc));
        assert_eq!(query.sort_order(), SortOrder::Desc);
    }

    #[test]
    fn test_list_query_with_search() {
        let query = ListQuery::new().with_search("alice".to_string());
        assert_eq!(query.search, Some("alice".to_string()));
        assert!(query.has_search());
    }

    #[test]
    fn test_list_query_with_filter() {
        let query = ListQuery::new()
            .with_filter("status=active".to_string())
            .with_filter("role=admin".to_string());
        assert_eq!(query.filter.len(), 2);
        assert!(query.has_filters());
    }

    #[test]
    fn test_list_query_with_filters() {
        let query = ListQuery::new()
            .with_filters(vec!["status=active".to_string(), "role=admin".to_string()]);
        assert_eq!(query.filter.len(), 2);
    }

    #[test]
    fn test_page_number_defaults() {
        let query = ListQuery::new();
        assert_eq!(query.page_number(), 1);
    }

    #[test]
    fn test_page_number_zero_protection() {
        let query = ListQuery::new().with_page(0);
        assert_eq!(query.page_number(), 1);
    }

    #[test]
    fn test_items_per_page_defaults() {
        let query = ListQuery::new();
        assert_eq!(query.items_per_page(), DEFAULT_PER_PAGE);
    }

    #[test]
    fn test_items_per_page_zero_protection() {
        let query = ListQuery::new().with_per_page(0);
        assert_eq!(query.items_per_page(), 1);
    }

    #[test]
    fn test_items_per_page_max_limit() {
        let query = ListQuery::new().with_per_page(500);
        assert_eq!(query.items_per_page(), MAX_PER_PAGE);
    }

    #[test]
    fn test_offset_calculation() {
        // Page 1 -> offset 0
        let query = ListQuery::new().with_page(1).with_per_page(20);
        assert_eq!(query.offset(), 0);

        // Page 2 -> offset 20
        let query = ListQuery::new().with_page(2).with_per_page(20);
        assert_eq!(query.offset(), 20);

        // Page 3, 50 per page -> offset 100
        let query = ListQuery::new().with_page(3).with_per_page(50);
        assert_eq!(query.offset(), 100);
    }

    #[test]
    fn test_sort_order_defaults() {
        let query = ListQuery::new();
        assert_eq!(query.sort_order(), SortOrder::Asc);
    }

    #[test]
    fn test_has_search_empty() {
        let query = ListQuery::new();
        assert!(!query.has_search());
    }

    #[test]
    fn test_has_search_empty_string() {
        let query = ListQuery::new().with_search(String::new());
        assert!(!query.has_search());
    }

    #[test]
    fn test_has_search_with_value() {
        let query = ListQuery::new().with_search("test".to_string());
        assert!(query.has_search());
    }

    #[test]
    fn test_has_filters_empty() {
        let query = ListQuery::new();
        assert!(!query.has_filters());
    }

    #[test]
    fn test_has_filters_with_value() {
        let query = ListQuery::new().with_filter("status=active".to_string());
        assert!(query.has_filters());
    }

    #[test]
    fn test_has_sort_empty() {
        let query = ListQuery::new();
        assert!(!query.has_sort());
    }

    #[test]
    fn test_has_sort_empty_string() {
        let query = ListQuery::new().with_sort(String::new());
        assert!(!query.has_sort());
    }

    #[test]
    fn test_has_sort_with_value() {
        let query = ListQuery::new().with_sort("name".to_string());
        assert!(query.has_sort());
    }

    #[test]
    fn test_list_query_clone() {
        let query = ListQuery::new()
            .with_page(2)
            .with_per_page(50)
            .with_sort("name".to_string())
            .with_order(SortOrder::Desc)
            .with_search("test".to_string())
            .with_filter("status=active".to_string());

        let cloned = query.clone();
        assert_eq!(query, cloned);
    }

    #[test]
    fn test_list_query_chained_builder() {
        let query = ListQuery::new()
            .with_page(2)
            .with_per_page(50)
            .with_sort("created_at".to_string())
            .with_order(SortOrder::Desc)
            .with_search("alice".to_string())
            .with_filter("status=active".to_string())
            .with_filter("role=admin".to_string());

        assert_eq!(query.page_number(), 2);
        assert_eq!(query.items_per_page(), 50);
        assert_eq!(query.offset(), 50);
        assert_eq!(query.sort, Some("created_at".to_string()));
        assert_eq!(query.sort_order(), SortOrder::Desc);
        assert_eq!(query.search, Some("alice".to_string()));
        assert_eq!(query.filter.len(), 2);
    }

    #[test]
    fn test_sort_order_serde() {
        let asc: SortOrder = serde_json::from_str("\"asc\"").unwrap();
        assert_eq!(asc, SortOrder::Asc);

        let desc: SortOrder = serde_json::from_str("\"desc\"").unwrap();
        assert_eq!(desc, SortOrder::Desc);

        assert_eq!(serde_json::to_string(&SortOrder::Asc).unwrap(), "\"asc\"");
        assert_eq!(serde_json::to_string(&SortOrder::Desc).unwrap(), "\"desc\"");
    }

    #[test]
    fn test_list_query_serde() {
        let query = ListQuery::new()
            .with_page(2)
            .with_per_page(50)
            .with_sort("name".to_string())
            .with_order(SortOrder::Desc);

        let json = serde_json::to_string(&query).unwrap();
        let deserialized: ListQuery = serde_json::from_str(&json).unwrap();
        assert_eq!(query, deserialized);
    }
}
