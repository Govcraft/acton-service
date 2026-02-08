//! Repository trait definitions
//!
//! This module provides generic traits for database CRUD operations using RPITIT
//! (Return Position Impl Trait In Traits), available since Rust 1.75.
//!
//! # Overview
//!
//! - [`Repository`]: Base trait for standard CRUD operations
//! - [`SoftDeleteRepository`]: Extended trait for soft delete support
//! - [`RelationLoader`]: Trait for eager loading relationships (N+1 prevention)
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
//!             .map_err(|e| e.into())
//!     }
//!     // ... other methods
//! }
//! ```

use std::collections::HashMap;
use std::future::Future;
use std::hash::Hash;

use super::error::RepositoryError;
use super::pagination::{FilterCondition, OrderDirection, Pagination};

/// Result type for repository operations
pub type RepositoryResult<T> = std::result::Result<T, RepositoryError>;

/// Base repository trait for CRUD operations
///
/// This trait defines the standard operations for working with entities in a database.
/// It uses Rust 1.75+ RPITIT (Return Position Impl Trait In Traits) for ergonomic async
/// trait methods without requiring `async_trait`.
///
/// # Type Parameters
///
/// - `Id`: The identifier type for the entity (e.g., `UserId`, `Uuid`, `i64`)
/// - `Entity`: The full entity type returned from queries
/// - `Create`: The data transfer object for creating new entities
/// - `Update`: The data transfer object for updating existing entities
///
/// # Example
///
/// ```rust,ignore
/// use acton_service::repository::{Repository, RepositoryResult};
///
/// struct UserRepository { /* ... */ }
///
/// impl Repository<UserId, User, CreateUser, UpdateUser> for UserRepository {
///     async fn find_by_id(&self, id: &UserId) -> RepositoryResult<Option<User>> {
///         // Implementation
///         todo!()
///     }
///
///     async fn find_all(
///         &self,
///         filters: &[FilterCondition],
///         order_by: Option<(&str, OrderDirection)>,
///         pagination: Option<Pagination>,
///     ) -> RepositoryResult<Vec<User>> {
///         // Implementation
///         todo!()
///     }
///
///     // ... other required methods
/// }
/// ```
pub trait Repository<Id, Entity, Create, Update>: Send + Sync {
    /// Find an entity by its unique identifier
    ///
    /// Returns `Ok(Some(entity))` if found, `Ok(None)` if not found.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let user = repo.find_by_id(&user_id).await?;
    /// match user {
    ///     Some(u) => println!("Found user: {}", u.name),
    ///     None => println!("User not found"),
    /// }
    /// ```
    fn find_by_id(&self, id: &Id) -> impl Future<Output = RepositoryResult<Option<Entity>>> + Send;

    /// Find all entities matching the given filters
    ///
    /// Supports filtering, ordering, and pagination.
    ///
    /// # Arguments
    ///
    /// - `filters`: Zero or more filter conditions to apply
    /// - `order_by`: Optional tuple of (field_name, direction)
    /// - `pagination`: Optional pagination parameters
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let filters = vec![
    ///     FilterCondition::eq("status", "active"),
    ///     FilterCondition::gte("age", 18),
    /// ];
    /// let users = repo.find_all(
    ///     &filters,
    ///     Some(("created_at", OrderDirection::Descending)),
    ///     Some(Pagination::first_page(20)),
    /// ).await?;
    /// ```
    fn find_all(
        &self,
        filters: &[FilterCondition],
        order_by: Option<(&str, OrderDirection)>,
        pagination: Option<Pagination>,
    ) -> impl Future<Output = RepositoryResult<Vec<Entity>>> + Send;

    /// Count entities matching the given filters
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let active_count = repo.count(&[FilterCondition::eq("status", "active")]).await?;
    /// println!("Active users: {}", active_count);
    /// ```
    fn count(
        &self,
        filters: &[FilterCondition],
    ) -> impl Future<Output = RepositoryResult<u64>> + Send;

    /// Check if an entity exists by its identifier
    ///
    /// More efficient than `find_by_id` when you only need to check existence.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if repo.exists(&user_id).await? {
    ///     println!("User exists");
    /// }
    /// ```
    fn exists(&self, id: &Id) -> impl Future<Output = RepositoryResult<bool>> + Send;

    /// Create a new entity
    ///
    /// Returns the created entity with any generated fields (e.g., ID, timestamps).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let new_user = CreateUser {
    ///     name: "Alice".to_string(),
    ///     email: "alice@example.com".to_string(),
    /// };
    /// let created = repo.create(new_user).await?;
    /// println!("Created user with ID: {}", created.id);
    /// ```
    fn create(&self, data: Create) -> impl Future<Output = RepositoryResult<Entity>> + Send;

    /// Update an existing entity
    ///
    /// Returns the updated entity.
    ///
    /// # Errors
    ///
    /// Returns `RepositoryError` with `NotFound` kind if the entity doesn't exist.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let update = UpdateUser {
    ///     name: Some("Alice Smith".to_string()),
    ///     email: None, // Don't update email
    /// };
    /// let updated = repo.update(&user_id, update).await?;
    /// ```
    fn update(
        &self,
        id: &Id,
        data: Update,
    ) -> impl Future<Output = RepositoryResult<Entity>> + Send;

    /// Delete an entity by its identifier (hard delete)
    ///
    /// Returns `true` if the entity was deleted, `false` if it didn't exist.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let was_deleted = repo.delete(&user_id).await?;
    /// if was_deleted {
    ///     println!("User deleted");
    /// }
    /// ```
    fn delete(&self, id: &Id) -> impl Future<Output = RepositoryResult<bool>> + Send;
}

/// Extended repository trait for soft delete support
///
/// Soft delete is useful for GDPR compliance, audit trails, and data recovery.
/// Entities are marked as deleted rather than being removed from the database.
///
/// # Type Parameters
///
/// Same as [`Repository`].
///
/// # Example
///
/// ```rust,ignore
/// use acton_service::repository::{SoftDeleteRepository, Repository};
///
/// impl SoftDeleteRepository<UserId, User, CreateUser, UpdateUser> for UserRepository {
///     async fn soft_delete(&self, id: &UserId) -> RepositoryResult<bool> {
///         let affected = sqlx::query!(
///             "UPDATE users SET deleted_at = NOW() WHERE id = $1 AND deleted_at IS NULL",
///             id.as_str()
///         )
///         .execute(&self.pool)
///         .await?
///         .rows_affected();
///         Ok(affected > 0)
///     }
///     // ... other methods
/// }
/// ```
pub trait SoftDeleteRepository<Id, Entity, Create, Update>:
    Repository<Id, Entity, Create, Update>
{
    /// Mark an entity as deleted without removing it from the database
    ///
    /// Returns `true` if the entity was soft-deleted, `false` if not found.
    fn soft_delete(&self, id: &Id) -> impl Future<Output = RepositoryResult<bool>> + Send;

    /// Restore a soft-deleted entity
    ///
    /// Returns `true` if the entity was restored, `false` if not found or not deleted.
    fn restore(&self, id: &Id) -> impl Future<Output = RepositoryResult<bool>> + Send;

    /// Find all entities including soft-deleted ones
    ///
    /// Useful for admin interfaces or audit views.
    fn find_with_deleted(
        &self,
        filters: &[FilterCondition],
        order_by: Option<(&str, OrderDirection)>,
        pagination: Option<Pagination>,
    ) -> impl Future<Output = RepositoryResult<Vec<Entity>>> + Send;

    /// Find only soft-deleted entities
    ///
    /// Useful for "trash" or "recycle bin" views.
    fn find_deleted(
        &self,
        filters: &[FilterCondition],
        order_by: Option<(&str, OrderDirection)>,
        pagination: Option<Pagination>,
    ) -> impl Future<Output = RepositoryResult<Vec<Entity>>> + Send;

    /// Permanently delete a soft-deleted entity
    ///
    /// This is a hard delete that removes the entity from the database entirely.
    /// Returns `true` if the entity was deleted, `false` if not found.
    fn force_delete(&self, id: &Id) -> impl Future<Output = RepositoryResult<bool>> + Send;
}

/// Trait for eager loading relationships (N+1 prevention)
///
/// This trait enables efficient batch loading of related entities,
/// preventing the N+1 query problem common in ORM usage.
///
/// # Type Parameters
///
/// - `Entity`: The parent entity type
/// - `RelatedId`: The identifier type for the related entity
/// - `Related`: The related entity type
///
/// # Example
///
/// ```rust,ignore
/// use acton_service::repository::{RelationLoader, RepositoryResult};
///
/// struct UserOrdersLoader {
///     pool: PgPool,
/// }
///
/// impl RelationLoader<User, OrderId, Order> for UserOrdersLoader {
///     async fn load_one(&self, entity: &User) -> RepositoryResult<Option<Order>> {
///         // Load the most recent order for a user
///         sqlx::query_as!(Order,
///             "SELECT * FROM orders WHERE user_id = $1 ORDER BY created_at DESC LIMIT 1",
///             entity.id.as_str()
///         )
///         .fetch_optional(&self.pool)
///         .await
///         .map_err(Into::into)
///     }
///
///     async fn load_many(&self, entity: &User) -> RepositoryResult<Vec<Order>> {
///         // Load all orders for a user
///         sqlx::query_as!(Order,
///             "SELECT * FROM orders WHERE user_id = $1",
///             entity.id.as_str()
///         )
///         .fetch_all(&self.pool)
///         .await
///         .map_err(Into::into)
///     }
///
///     async fn batch_load(
///         &self,
///         ids: &[OrderId],
///     ) -> RepositoryResult<HashMap<OrderId, Order>>
///     where
///         Order: Clone,
///         OrderId: Clone,
///     {
///         // Batch load orders by ID
///         let orders = sqlx::query_as!(Order,
///             "SELECT * FROM orders WHERE id = ANY($1)",
///             &ids.iter().map(|id| id.as_str()).collect::<Vec<_>>()
///         )
///         .fetch_all(&self.pool)
///         .await?;
///
///         Ok(orders.into_iter().map(|o| (o.id.clone(), o)).collect())
///     }
/// }
/// ```
pub trait RelationLoader<Entity, RelatedId, Related>: Send + Sync
where
    RelatedId: Eq + Hash,
{
    /// Load a single related entity for the given parent
    ///
    /// Returns `None` if no related entity exists.
    fn load_one(
        &self,
        entity: &Entity,
    ) -> impl Future<Output = RepositoryResult<Option<Related>>> + Send;

    /// Load multiple related entities for the given parent
    ///
    /// Returns an empty vector if no related entities exist.
    fn load_many(
        &self,
        entity: &Entity,
    ) -> impl Future<Output = RepositoryResult<Vec<Related>>> + Send;

    /// Batch load related entities by their IDs
    ///
    /// This is the key method for preventing N+1 queries. Instead of loading
    /// related entities one at a time, collect all needed IDs and load them
    /// in a single query.
    ///
    /// Returns a map from ID to entity for efficient lookup.
    fn batch_load(
        &self,
        ids: &[RelatedId],
    ) -> impl Future<Output = RepositoryResult<HashMap<RelatedId, Related>>> + Send
    where
        Related: Clone,
        RelatedId: Clone;
}

#[cfg(test)]
mod tests {
    use super::*;

    // Compile-time tests to ensure traits can be object-safe and implemented
    // Note: We can't make the traits object-safe due to RPITIT, but we can
    // verify they compile correctly.

    #[test]
    fn test_repository_result_type() {
        // Verify RepositoryResult type alias works correctly
        let ok_result: RepositoryResult<i32> = Ok(42);
        assert!(ok_result.is_ok());

        let err_result: RepositoryResult<i32> = Err(
            super::super::error::RepositoryError::not_found("Test", "123"),
        );
        assert!(err_result.is_err());
    }

    // The following tests verify that the trait bounds compile correctly.
    // Actual implementations would be tested in integration tests.

    struct MockId(String);
    struct MockEntity {
        id: String,
    }
    struct MockCreate {
        name: String,
    }
    struct MockUpdate {
        name: Option<String>,
    }

    // This test verifies the trait can be implemented
    struct MockRepository;

    impl Repository<MockId, MockEntity, MockCreate, MockUpdate> for MockRepository {
        async fn find_by_id(&self, _id: &MockId) -> RepositoryResult<Option<MockEntity>> {
            Ok(None)
        }

        async fn find_all(
            &self,
            _filters: &[FilterCondition],
            _order_by: Option<(&str, OrderDirection)>,
            _pagination: Option<Pagination>,
        ) -> RepositoryResult<Vec<MockEntity>> {
            Ok(vec![])
        }

        async fn count(&self, _filters: &[FilterCondition]) -> RepositoryResult<u64> {
            Ok(0)
        }

        async fn exists(&self, _id: &MockId) -> RepositoryResult<bool> {
            Ok(false)
        }

        async fn create(&self, data: MockCreate) -> RepositoryResult<MockEntity> {
            Ok(MockEntity { id: data.name })
        }

        async fn update(&self, _id: &MockId, _data: MockUpdate) -> RepositoryResult<MockEntity> {
            Ok(MockEntity {
                id: "updated".to_string(),
            })
        }

        async fn delete(&self, _id: &MockId) -> RepositoryResult<bool> {
            Ok(true)
        }
    }

    impl SoftDeleteRepository<MockId, MockEntity, MockCreate, MockUpdate> for MockRepository {
        async fn soft_delete(&self, _id: &MockId) -> RepositoryResult<bool> {
            Ok(true)
        }

        async fn restore(&self, _id: &MockId) -> RepositoryResult<bool> {
            Ok(true)
        }

        async fn find_with_deleted(
            &self,
            _filters: &[FilterCondition],
            _order_by: Option<(&str, OrderDirection)>,
            _pagination: Option<Pagination>,
        ) -> RepositoryResult<Vec<MockEntity>> {
            Ok(vec![])
        }

        async fn find_deleted(
            &self,
            _filters: &[FilterCondition],
            _order_by: Option<(&str, OrderDirection)>,
            _pagination: Option<Pagination>,
        ) -> RepositoryResult<Vec<MockEntity>> {
            Ok(vec![])
        }

        async fn force_delete(&self, _id: &MockId) -> RepositoryResult<bool> {
            Ok(true)
        }
    }

    #[derive(Clone, PartialEq, Eq, Hash)]
    struct RelatedId(String);

    #[derive(Clone)]
    struct RelatedEntity {
        id: RelatedId,
    }

    struct MockRelationLoader;

    impl RelationLoader<MockEntity, RelatedId, RelatedEntity> for MockRelationLoader {
        async fn load_one(&self, _entity: &MockEntity) -> RepositoryResult<Option<RelatedEntity>> {
            Ok(None)
        }

        async fn load_many(&self, _entity: &MockEntity) -> RepositoryResult<Vec<RelatedEntity>> {
            Ok(vec![])
        }

        async fn batch_load(
            &self,
            ids: &[RelatedId],
        ) -> RepositoryResult<HashMap<RelatedId, RelatedEntity>>
        where
            RelatedEntity: Clone,
            RelatedId: Clone,
        {
            Ok(ids
                .iter()
                .map(|id| (id.clone(), RelatedEntity { id: id.clone() }))
                .collect())
        }
    }

    #[tokio::test]
    async fn test_mock_repository_find_by_id() {
        let repo = MockRepository;
        let result = repo.find_by_id(&MockId("test".to_string())).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_mock_repository_create() {
        let repo = MockRepository;
        let result = repo
            .create(MockCreate {
                name: "test".to_string(),
            })
            .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().id, "test");
    }

    #[tokio::test]
    async fn test_mock_soft_delete_repository() {
        let repo = MockRepository;
        let result = repo.soft_delete(&MockId("test".to_string())).await;
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[tokio::test]
    async fn test_mock_relation_loader_batch() {
        let loader = MockRelationLoader;
        let ids = vec![RelatedId("1".to_string()), RelatedId("2".to_string())];
        let result = loader.batch_load(&ids).await;
        assert!(result.is_ok());
        let map = result.unwrap();
        assert_eq!(map.len(), 2);
        assert!(map.contains_key(&RelatedId("1".to_string())));
        assert!(map.contains_key(&RelatedId("2".to_string())));
    }
}
