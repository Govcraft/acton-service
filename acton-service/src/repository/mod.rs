//! Repository traits for database CRUD abstractions
//!
//! This module provides generic repository traits for common CRUD operations,
//! enabling a consistent interface for database access across different backends.
//!
//! # Features
//!
//! - **Generic CRUD**: [`Repository`] trait for create, read, update, delete operations
//! - **Soft Delete**: [`SoftDeleteRepository`] for GDPR compliance and audit trails
//! - **Relation Loading**: [`RelationLoader`] for eager loading (N+1 prevention)
//! - **Filtering**: [`FilterCondition`] for building WHERE clauses
//! - **Pagination**: [`Pagination`] for limiting query results
//!
//! # Example
//!
//! ```rust,ignore
//! use acton_service::repository::{
//!     FilterCondition, OrderDirection, Pagination, Repository, RepositoryResult,
//! };
//!
//! struct UserRepository {
//!     pool: PgPool,
//! }
//!
//! impl Repository<UserId, User, CreateUser, UpdateUser> for UserRepository {
//!     async fn find_by_id(&self, id: &UserId) -> RepositoryResult<Option<User>> {
//!         sqlx::query_as!(User, "SELECT * FROM users WHERE id = $1", id.as_str())
//!             .fetch_optional(&self.pool)
//!             .await
//!             .map_err(Into::into)
//!     }
//!
//!     async fn find_all(
//!         &self,
//!         filters: &[FilterCondition],
//!         order_by: Option<(&str, OrderDirection)>,
//!         pagination: Option<Pagination>,
//!     ) -> RepositoryResult<Vec<User>> {
//!         // Build dynamic query with filters
//!         todo!()
//!     }
//!
//!     async fn count(&self, filters: &[FilterCondition]) -> RepositoryResult<u64> {
//!         todo!()
//!     }
//!
//!     async fn exists(&self, id: &UserId) -> RepositoryResult<bool> {
//!         todo!()
//!     }
//!
//!     async fn create(&self, data: CreateUser) -> RepositoryResult<User> {
//!         todo!()
//!     }
//!
//!     async fn update(&self, id: &UserId, data: UpdateUser) -> RepositoryResult<User> {
//!         todo!()
//!     }
//!
//!     async fn delete(&self, id: &UserId) -> RepositoryResult<bool> {
//!         todo!()
//!     }
//! }
//! ```

mod error;
mod pagination;
mod traits;

// Re-export all public types
pub use error::{RepositoryError, RepositoryErrorKind, RepositoryOperation};
pub use pagination::{FilterCondition, FilterOperator, FilterValue, OrderDirection, Pagination};
pub use traits::{RelationLoader, Repository, RepositoryResult, SoftDeleteRepository};
