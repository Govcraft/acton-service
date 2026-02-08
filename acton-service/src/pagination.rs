//! Pagination support for acton-service
//!
//! This module re-exports pagination types from the paginator-rs family of crates
//! when the appropriate feature flags are enabled. It provides a unified interface
//! for pagination across different contexts (collections, web frameworks, databases).
//!
//! ## Features
//!
//! - `pagination` - Core pagination types and traits from `paginator-rs`
//! - `pagination-axum` - Axum web framework integration from `paginator-axum`
//! - `pagination-sqlx` - SQLx database integration from `paginator-sqlx`
//! - `pagination-full` - All pagination features combined
//!
//! ## Quick Start
//!
//! Add the feature to your `Cargo.toml`:
//!
//! ```toml
//! [dependencies]
//! acton-service = { version = "0.12", features = ["pagination-axum"] }
//! ```
//!
//! ## Example: Paginated API Endpoint
//!
//! ```rust,ignore
//! use acton_service::prelude::*;
//!
//! #[derive(Serialize)]
//! struct User {
//!     id: u64,
//!     name: String,
//! }
//!
//! async fn list_users(
//!     Query(params): Query<PaginationQueryParams>,
//! ) -> PaginatedJson<Vec<User>> {
//!     let users = vec![
//!         User { id: 1, name: "Alice".into() },
//!         User { id: 2, name: "Bob".into() },
//!     ];
//!
//!     // Create paginated response with metadata
//!     let metadata = PaginatorResponse {
//!         data: users.clone(),
//!         page: params.page.unwrap_or(1),
//!         per_page: params.per_page.unwrap_or(10),
//!         total_items: 2,
//!         total_pages: 1,
//!         has_next: false,
//!         has_prev: false,
//!     };
//!
//!     PaginatedJson::new(users, metadata)
//! }
//! ```
//!
//! ## Core Types
//!
//! The pagination feature provides these core types:
//!
//! - [`Paginator`] - Main pagination handler for in-memory collections
//! - [`PaginationParams`] - Configuration for pagination operations
//! - [`PaginatorResponse`] - Structured response with pagination metadata
//! - [`Cursor`] and [`CursorBuilder`] - Cursor-based pagination support
//! - [`Filter`] and [`FilterBuilder`] - Data filtering capabilities
//! - [`SortBuilder`] and [`SortDirection`] - Sorting configuration
//!
//! ## Axum Integration
//!
//! With the `pagination-axum` feature:
//!
//! - [`PaginatedJson`] - Response wrapper for paginated JSON data
//! - [`PaginationQuery`] - Extractor for pagination parameters
//! - [`PaginationQueryParams`] - Query parameter handling
//! - [`create_link_header`] - Generate HTTP Link headers for pagination
//!
//! ## SQLx Integration
//!
//! With the `pagination-sqlx` feature:
//!
//! - [`PaginatedQuery`] - Paginated database query results
//! - [`PaginateQuery`] - Trait for pagination behavior
//! - [`QueryBuilderExt`] - Extends SQLx query builder with pagination
//! - [`validate_field_name`] - SQL injection prevention for field names

// ============================================================================
// Core pagination types from paginator-rs
// ============================================================================

// Main paginator types
pub use paginator_rs::Paginator;
pub use paginator_rs::PaginatorBuilder;
pub use paginator_rs::PaginatorResponse;
pub use paginator_rs::PaginatorResponseMeta;
pub use paginator_rs::PaginatorResult;
pub use paginator_rs::PaginatorTrait;

// Pagination parameters
pub use paginator_rs::IntoPaginationParams;
pub use paginator_rs::PaginationParams;

// Cursor-based pagination
pub use paginator_rs::Cursor;
pub use paginator_rs::CursorBuilder;
pub use paginator_rs::CursorDirection;
pub use paginator_rs::CursorValue;

// Filtering
pub use paginator_rs::Filter;
pub use paginator_rs::FilterBuilder;
pub use paginator_rs::FilterOperator;
pub use paginator_rs::FilterValue;

// Search
pub use paginator_rs::SearchBuilder;
pub use paginator_rs::SearchParams;

// Sorting
pub use paginator_rs::SortBuilder;
pub use paginator_rs::SortDirection;

// Error handling
pub use paginator_rs::PaginatorError;

// ============================================================================
// Axum integration from paginator-axum
// ============================================================================

#[cfg(feature = "pagination-axum")]
pub use paginator_axum::create_link_header;
#[cfg(feature = "pagination-axum")]
pub use paginator_axum::PaginatedJson;
#[cfg(feature = "pagination-axum")]
pub use paginator_axum::PaginationQuery;
#[cfg(feature = "pagination-axum")]
pub use paginator_axum::PaginationQueryParams;

// ============================================================================
// SQLx integration from paginator-sqlx
// ============================================================================

#[cfg(feature = "pagination-sqlx")]
pub use paginator_sqlx::validate_field_name;
#[cfg(feature = "pagination-sqlx")]
pub use paginator_sqlx::PaginateQuery;
#[cfg(feature = "pagination-sqlx")]
pub use paginator_sqlx::PaginatedQuery;
#[cfg(feature = "pagination-sqlx")]
pub use paginator_sqlx::QueryBuilderExt;

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_core_types_accessible() {
        // Verify core types compile and are accessible
        let _params = PaginationParams::default();
    }

    #[test]
    fn test_cursor_direction_variants() {
        // Verify cursor direction enum variants
        let _after = CursorDirection::After;
        let _before = CursorDirection::Before;
    }

    #[test]
    fn test_sort_direction_variants() {
        // Verify sort direction enum variants
        let _asc = SortDirection::Asc;
        let _desc = SortDirection::Desc;
    }

    #[test]
    fn test_filter_operators_available() {
        // Verify filter operators are accessible
        let _eq = FilterOperator::Eq;
        let _ne = FilterOperator::Ne;
        let _gt = FilterOperator::Gt;
        let _lt = FilterOperator::Lt;
    }

    #[test]
    #[cfg(feature = "pagination-axum")]
    fn test_axum_types_accessible() {
        // Verify Axum types compile when feature is enabled
        // PaginationQueryParams is a struct for deserializing query params
        fn _type_check(_: PaginationQueryParams) {}
    }

    #[test]
    #[cfg(feature = "pagination-sqlx")]
    fn test_field_validation() {
        // Verify field name validation works
        assert!(validate_field_name("valid_field").is_ok());
        assert!(validate_field_name("table.column").is_ok());
        // SQL injection attempts should fail
        assert!(validate_field_name("field; DROP TABLE users").is_err());
    }
}
