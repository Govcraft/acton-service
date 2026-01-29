//! Handler traits for REST CRUD patterns
//!
//! This module provides handler-level abstractions for the standard REST
//! collection pattern (list, get, create, update, delete). It builds on
//! the repository traits to provide HTTP-aware error handling and response types.
//!
//! # Features
//!
//! - **CRUD Handlers**: [`CollectionHandler`] trait for standard REST operations
//! - **Soft Delete**: [`SoftDeleteHandler`] for GDPR compliance and audit trails
//! - **Pagination**: [`ListQuery`] and [`ListResponse`] for paginated list endpoints
//! - **Error Handling**: [`ApiError`] with automatic HTTP status code mapping
//!
//! # Example
//!
//! ```rust,ignore
//! use acton_service::handlers::{
//!     ApiError, CollectionHandler, ItemResponse, ListQuery, ListResponse, PaginationMeta,
//! };
//! use acton_service::repository::Repository;
//!
//! struct UserHandler {
//!     repository: UserRepository,
//! }
//!
//! impl CollectionHandler<UserId, User, CreateUser, UpdateUser> for UserHandler {
//!     async fn list(&self, query: ListQuery) -> Result<ListResponse<User>, ApiError> {
//!         let filters = vec![];
//!         let order = query.sort.as_ref().map(|s| (s.as_str(), query.sort_order().into()));
//!         let pagination = Some(Pagination::new(query.offset(), query.items_per_page().into()));
//!
//!         let users = self.repository.find_all(&filters, order, pagination).await?;
//!         let total = self.repository.count(&filters).await?;
//!
//!         Ok(ListResponse::new(users, PaginationMeta::new(
//!             query.page_number(),
//!             query.items_per_page(),
//!             total,
//!         )))
//!     }
//!
//!     async fn get(&self, id: UserId) -> Result<ItemResponse<User>, ApiError> {
//!         let user = self.repository.find_by_id(&id).await?
//!             .ok_or_else(|| ApiError::not_found("User", id.to_string()))?;
//!         Ok(ItemResponse::new(user))
//!     }
//!
//!     async fn create(&self, dto: CreateUser) -> Result<ItemResponse<User>, ApiError> {
//!         let user = self.repository.create(dto).await?;
//!         Ok(ItemResponse::new(user))
//!     }
//!
//!     async fn update(&self, id: UserId, dto: UpdateUser) -> Result<ItemResponse<User>, ApiError> {
//!         let user = self.repository.update(&id, dto).await?;
//!         Ok(ItemResponse::new(user))
//!     }
//!
//!     async fn delete(&self, id: UserId) -> Result<(), ApiError> {
//!         self.repository.delete(&id).await?;
//!         Ok(())
//!     }
//! }
//! ```
//!
//! # Integration with Axum
//!
//! The response types implement `IntoResponse`, so they can be returned directly
//! from Axum handlers:
//!
//! ```rust,ignore
//! use axum::{extract::{Path, Query, State}, Json};
//! use acton_service::handlers::{CollectionHandler, ListQuery};
//!
//! async fn list_users(
//!     State(handler): State<UserHandler>,
//!     Query(query): Query<ListQuery>,
//! ) -> Result<impl IntoResponse, ApiError> {
//!     handler.list(query).await
//! }
//!
//! async fn get_user(
//!     State(handler): State<UserHandler>,
//!     Path(id): Path<UserId>,
//! ) -> Result<impl IntoResponse, ApiError> {
//!     handler.get(id).await
//! }
//! ```

mod error;
mod query;
mod response;
mod traits;

// Re-export all public types
pub use error::{ApiError, ApiErrorKind, ApiOperation};
pub use query::{ListQuery, SortOrder, DEFAULT_PER_PAGE, MAX_PER_PAGE};
pub use response::{ItemResponse, ListResponse, PaginationMeta, ResponseMeta};
pub use traits::{CollectionHandler, SoftDeleteHandler};
