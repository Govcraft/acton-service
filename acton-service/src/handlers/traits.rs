//! Handler trait definitions for REST CRUD patterns
//!
//! This module provides generic traits for REST collection handlers using RPITIT
//! (Return Position Impl Trait In Traits), available since Rust 1.75.
//!
//! # Overview
//!
//! - [`CollectionHandler`]: Standard CRUD operations (list, get, create, update, delete)
//! - [`SoftDeleteHandler`]: Extended operations for soft delete support
//!
//! # Example
//!
//! ```rust,ignore
//! use acton_service::handlers::{
//!     ApiError, CollectionHandler, ItemResponse, ListQuery, ListResponse,
//! };
//!
//! struct UserHandler {
//!     repository: UserRepository,
//! }
//!
//! impl CollectionHandler<UserId, User, CreateUser, UpdateUser> for UserHandler {
//!     async fn list(&self, query: ListQuery) -> Result<ListResponse<User>, ApiError> {
//!         let users = self.repository.find_all(&[], None, Some(query.into())).await?;
//!         let total = self.repository.count(&[]).await?;
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
//!     // ... other methods
//! }
//! ```

use std::future::Future;

use super::error::ApiError;
use super::query::ListQuery;
use super::response::{ItemResponse, ListResponse};

/// Standard REST CRUD handler trait
///
/// This trait defines the standard operations for REST collection endpoints.
/// It uses Rust 1.75+ RPITIT (Return Position Impl Trait In Traits) for ergonomic
/// async trait methods without requiring `async_trait`.
///
/// # Type Parameters
///
/// - `Id`: The identifier type for the entity (e.g., `UserId`, `Uuid`, `i64`)
/// - `Entity`: The full entity type returned from queries
/// - `CreateDto`: The data transfer object for creating new entities
/// - `UpdateDto`: The data transfer object for updating existing entities
///
/// # Example
///
/// ```rust,ignore
/// use acton_service::handlers::{ApiError, CollectionHandler, ItemResponse, ListQuery, ListResponse};
///
/// struct ProductHandler { /* ... */ }
///
/// impl CollectionHandler<ProductId, Product, CreateProduct, UpdateProduct> for ProductHandler {
///     async fn list(&self, query: ListQuery) -> Result<ListResponse<Product>, ApiError> {
///         // Implementation
///         todo!()
///     }
///
///     async fn get(&self, id: ProductId) -> Result<ItemResponse<Product>, ApiError> {
///         // Implementation
///         todo!()
///     }
///
///     async fn create(&self, dto: CreateProduct) -> Result<ItemResponse<Product>, ApiError> {
///         // Implementation
///         todo!()
///     }
///
///     async fn update(&self, id: ProductId, dto: UpdateProduct) -> Result<ItemResponse<Product>, ApiError> {
///         // Implementation
///         todo!()
///     }
///
///     async fn delete(&self, id: ProductId) -> Result<(), ApiError> {
///         // Implementation
///         todo!()
///     }
/// }
/// ```
pub trait CollectionHandler<Id, Entity, CreateDto, UpdateDto>: Send + Sync {
    /// List entities with pagination, filtering, and sorting
    ///
    /// Returns a paginated list of entities matching the query parameters.
    ///
    /// # Arguments
    ///
    /// * `query` - Query parameters for pagination, sorting, and filtering
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let query = ListQuery::new()
    ///     .with_page(1)
    ///     .with_per_page(20)
    ///     .with_sort("created_at".to_string())
    ///     .with_order(SortOrder::Desc);
    ///
    /// let response = handler.list(query).await?;
    /// println!("Found {} items", response.pagination.total);
    /// ```
    fn list(
        &self,
        query: ListQuery,
    ) -> impl Future<Output = Result<ListResponse<Entity>, ApiError>> + Send;

    /// Get a single entity by its identifier
    ///
    /// Returns the entity if found, or a NotFound error if it doesn't exist.
    ///
    /// # Arguments
    ///
    /// * `id` - The unique identifier of the entity
    ///
    /// # Errors
    ///
    /// Returns `ApiError` with `NotFound` kind if the entity doesn't exist.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let user = handler.get(user_id).await?;
    /// println!("Found user: {}", user.data.name);
    /// ```
    fn get(&self, id: Id) -> impl Future<Output = Result<ItemResponse<Entity>, ApiError>> + Send;

    /// Create a new entity
    ///
    /// Returns the created entity with any generated fields (e.g., ID, timestamps).
    ///
    /// # Arguments
    ///
    /// * `dto` - The data for creating the new entity
    ///
    /// # Errors
    ///
    /// Returns `ApiError` with:
    /// - `ValidationFailed` if the input data is invalid
    /// - `AlreadyExists` if a duplicate entity would be created
    /// - `Conflict` if the operation violates constraints
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let dto = CreateUser { name: "Alice".to_string(), email: "alice@example.com".to_string() };
    /// let response = handler.create(dto).await?;
    /// println!("Created user with ID: {}", response.data.id);
    /// ```
    fn create(
        &self,
        dto: CreateDto,
    ) -> impl Future<Output = Result<ItemResponse<Entity>, ApiError>> + Send;

    /// Update an existing entity
    ///
    /// Returns the updated entity.
    ///
    /// # Arguments
    ///
    /// * `id` - The unique identifier of the entity to update
    /// * `dto` - The data for updating the entity
    ///
    /// # Errors
    ///
    /// Returns `ApiError` with:
    /// - `NotFound` if the entity doesn't exist
    /// - `ValidationFailed` if the input data is invalid
    /// - `Conflict` if the operation violates constraints
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let dto = UpdateUser { name: Some("Alice Smith".to_string()), email: None };
    /// let response = handler.update(user_id, dto).await?;
    /// println!("Updated user: {}", response.data.name);
    /// ```
    fn update(
        &self,
        id: Id,
        dto: UpdateDto,
    ) -> impl Future<Output = Result<ItemResponse<Entity>, ApiError>> + Send;

    /// Delete an entity by its identifier (hard delete)
    ///
    /// Permanently removes the entity from the system.
    ///
    /// # Arguments
    ///
    /// * `id` - The unique identifier of the entity to delete
    ///
    /// # Errors
    ///
    /// Returns `ApiError` with:
    /// - `NotFound` if the entity doesn't exist
    /// - `Conflict` if the entity cannot be deleted due to constraints
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// handler.delete(user_id).await?;
    /// println!("User deleted");
    /// ```
    fn delete(&self, id: Id) -> impl Future<Output = Result<(), ApiError>> + Send;
}

/// Extended handler trait for soft delete support
///
/// Soft delete is useful for GDPR compliance, audit trails, and data recovery.
/// Entities are marked as deleted rather than being removed from the database.
///
/// # Type Parameters
///
/// Same as [`CollectionHandler`].
///
/// # Example
///
/// ```rust,ignore
/// use acton_service::handlers::{SoftDeleteHandler, CollectionHandler};
///
/// impl SoftDeleteHandler<UserId, User, CreateUser, UpdateUser> for UserHandler {
///     async fn soft_delete(&self, id: UserId) -> Result<(), ApiError> {
///         // Mark user as deleted
///         self.repository.soft_delete(&id).await?;
///         Ok(())
///     }
///
///     async fn restore(&self, id: UserId) -> Result<ItemResponse<User>, ApiError> {
///         // Restore soft-deleted user
///         self.repository.restore(&id).await?;
///         let user = self.repository.find_by_id(&id).await?
///             .ok_or_else(|| ApiError::not_found("User", id.to_string()))?;
///         Ok(ItemResponse::new(user))
///     }
///
///     async fn list_with_deleted(&self, query: ListQuery) -> Result<ListResponse<User>, ApiError> {
///         // List all users including soft-deleted ones
///         todo!()
///     }
/// }
/// ```
pub trait SoftDeleteHandler<Id, Entity, CreateDto, UpdateDto>:
    CollectionHandler<Id, Entity, CreateDto, UpdateDto>
{
    /// Mark an entity as deleted without removing it from the database
    ///
    /// # Arguments
    ///
    /// * `id` - The unique identifier of the entity to soft delete
    ///
    /// # Errors
    ///
    /// Returns `ApiError` with `NotFound` if the entity doesn't exist.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// handler.soft_delete(user_id).await?;
    /// // User is now marked as deleted but still in the database
    /// ```
    fn soft_delete(&self, id: Id) -> impl Future<Output = Result<(), ApiError>> + Send;

    /// Restore a soft-deleted entity
    ///
    /// # Arguments
    ///
    /// * `id` - The unique identifier of the entity to restore
    ///
    /// # Errors
    ///
    /// Returns `ApiError` with `NotFound` if the entity doesn't exist or isn't deleted.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let response = handler.restore(user_id).await?;
    /// println!("Restored user: {}", response.data.name);
    /// ```
    fn restore(&self, id: Id) -> impl Future<Output = Result<ItemResponse<Entity>, ApiError>> + Send;

    /// List all entities including soft-deleted ones
    ///
    /// Useful for admin interfaces or audit views.
    ///
    /// # Arguments
    ///
    /// * `query` - Query parameters for pagination, sorting, and filtering
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let all_users = handler.list_with_deleted(ListQuery::default()).await?;
    /// println!("Total users (including deleted): {}", all_users.pagination.total);
    /// ```
    fn list_with_deleted(
        &self,
        query: ListQuery,
    ) -> impl Future<Output = Result<ListResponse<Entity>, ApiError>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    // Compile-time tests to ensure traits can be implemented
    // Actual implementations would be tested in integration tests.

    #[derive(Debug, Clone)]
    struct MockId(String);

    #[derive(Debug, Clone)]
    struct MockEntity {
        id: String,
        name: String,
    }

    #[derive(Debug, Clone)]
    struct MockCreate {
        name: String,
    }

    #[derive(Debug, Clone)]
    struct MockUpdate {
        name: Option<String>,
    }

    struct MockHandler;

    impl CollectionHandler<MockId, MockEntity, MockCreate, MockUpdate> for MockHandler {
        async fn list(&self, query: ListQuery) -> Result<ListResponse<MockEntity>, ApiError> {
            let data = vec![MockEntity {
                id: "1".to_string(),
                name: "Test".to_string(),
            }];
            let pagination = super::super::response::PaginationMeta::new(
                query.page_number(),
                query.items_per_page(),
                1,
            );
            Ok(ListResponse::new(data, pagination))
        }

        async fn get(&self, id: MockId) -> Result<ItemResponse<MockEntity>, ApiError> {
            Ok(ItemResponse::new(MockEntity {
                id: id.0,
                name: "Test".to_string(),
            }))
        }

        async fn create(&self, dto: MockCreate) -> Result<ItemResponse<MockEntity>, ApiError> {
            Ok(ItemResponse::new(MockEntity {
                id: "new".to_string(),
                name: dto.name,
            }))
        }

        async fn update(
            &self,
            id: MockId,
            dto: MockUpdate,
        ) -> Result<ItemResponse<MockEntity>, ApiError> {
            Ok(ItemResponse::new(MockEntity {
                id: id.0,
                name: dto.name.unwrap_or_else(|| "unchanged".to_string()),
            }))
        }

        async fn delete(&self, _id: MockId) -> Result<(), ApiError> {
            Ok(())
        }
    }

    impl SoftDeleteHandler<MockId, MockEntity, MockCreate, MockUpdate> for MockHandler {
        async fn soft_delete(&self, _id: MockId) -> Result<(), ApiError> {
            Ok(())
        }

        async fn restore(&self, id: MockId) -> Result<ItemResponse<MockEntity>, ApiError> {
            Ok(ItemResponse::new(MockEntity {
                id: id.0,
                name: "Restored".to_string(),
            }))
        }

        async fn list_with_deleted(
            &self,
            query: ListQuery,
        ) -> Result<ListResponse<MockEntity>, ApiError> {
            self.list(query).await
        }
    }

    #[tokio::test]
    async fn test_mock_handler_list() {
        let handler = MockHandler;
        let query = ListQuery::new().with_page(1).with_per_page(20);
        let result = handler.list(query).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.data.len(), 1);
        assert_eq!(response.pagination.total, 1);
    }

    #[tokio::test]
    async fn test_mock_handler_get() {
        let handler = MockHandler;
        let result = handler.get(MockId("123".to_string())).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().data.id, "123");
    }

    #[tokio::test]
    async fn test_mock_handler_create() {
        let handler = MockHandler;
        let dto = MockCreate {
            name: "New Entity".to_string(),
        };
        let result = handler.create(dto).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().data.name, "New Entity");
    }

    #[tokio::test]
    async fn test_mock_handler_update() {
        let handler = MockHandler;
        let dto = MockUpdate {
            name: Some("Updated".to_string()),
        };
        let result = handler.update(MockId("123".to_string()), dto).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().data.name, "Updated");
    }

    #[tokio::test]
    async fn test_mock_handler_delete() {
        let handler = MockHandler;
        let result = handler.delete(MockId("123".to_string())).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_mock_soft_delete_handler() {
        let handler = MockHandler;
        let result = handler.soft_delete(MockId("123".to_string())).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_mock_restore_handler() {
        let handler = MockHandler;
        let result = handler.restore(MockId("123".to_string())).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().data.name, "Restored");
    }

    #[tokio::test]
    async fn test_mock_list_with_deleted() {
        let handler = MockHandler;
        let query = ListQuery::default();
        let result = handler.list_with_deleted(query).await;
        assert!(result.is_ok());
    }
}
