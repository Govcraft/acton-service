//! HTTP handlers for users API

use acton_service::prelude::*;
use uuid::Uuid;

use crate::models::{CreateUserRequest, ListUsersResponse, UpdateUserRequest, UserResponse};

/// List all users
pub async fn list_users(
    State(_state): State<AppState>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<ListUsersResponse>> {
    info!("Listing users for {}", claims.sub);

    // In a real service, you would query the database
    // For now, return empty list
    let response = ListUsersResponse {
        users: vec![],
        total: 0,
    };

    Ok(Json(response))
}

/// Get a user by ID
pub async fn get_user(
    State(_state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(user_id): Path<Uuid>,
) -> Result<Json<UserResponse>> {
    info!("Getting user {} for {}", user_id, claims.sub);

    // Authorization: users can only get their own profile unless they're admin
    if !claims.has_role("admin") {
        let requester_id = claims.user_id()
            .ok_or_else(|| Error::Unauthorized("Invalid user token".to_string()))?;

        if requester_id != user_id.to_string() {
            return Err(Error::Forbidden("Cannot access other users".to_string()));
        }
    }

    // In a real service, you would query the database:
    // let db = state.db().ok_or_else(|| Error::Internal("Database not configured".to_string()))?;
    // let user = sqlx::query_as!(User, "SELECT * FROM users WHERE id = $1", user_id)
    //     .fetch_optional(db)
    //     .await?
    //     .ok_or_else(|| Error::NotFound("User not found".to_string()))?;

    // For now, return a mock user
    Err(Error::NotFound("User not found".to_string()))
}

/// Create a new user
pub async fn create_user(
    State(_state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(request): Json<CreateUserRequest>,
) -> Result<Json<UserResponse>> {
    info!("Creating user for {}", claims.sub);

    // Authorization: only admins can create users
    if !claims.has_role("admin") {
        return Err(Error::Forbidden("Only admins can create users".to_string()));
    }

    // Validate request
    request.validate()
        .map_err(Error::BadRequest)?;

    // In a real service, you would insert into database:
    // let db = state.db().ok_or_else(|| Error::Internal("Database not configured".to_string()))?;
    // let user_id = Uuid::new_v4();
    // let roles = request.roles();
    //
    // let user = sqlx::query_as!(
    //     User,
    //     r#"
    //     INSERT INTO users (id, username, email, roles, created_at, updated_at)
    //     VALUES ($1, $2, $3, $4, NOW(), NOW())
    //     RETURNING id, username, email, roles, created_at, updated_at
    //     "#,
    //     user_id,
    //     request.username,
    //     request.email,
    //     &roles
    // )
    // .fetch_one(db)
    // .await?;

    info!(
        "Would create user: username={}, email={}, roles={:?}",
        request.username,
        request.email,
        request.roles()
    );

    Err(Error::Internal("Database not configured - cannot create user".to_string()))
}

/// Update a user
pub async fn update_user(
    State(_state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(user_id): Path<Uuid>,
    Json(request): Json<UpdateUserRequest>,
) -> Result<Json<UserResponse>> {
    info!("Updating user {} for {}", user_id, claims.sub);

    // Check if there are any updates
    if !request.has_updates() {
        return Err(Error::BadRequest("No updates provided".to_string()));
    }

    // Validate request
    request.validate()
        .map_err(Error::BadRequest)?;

    // Authorization: users can only update their own profile unless they're admin
    if !claims.has_role("admin") {
        let requester_id = claims.user_id()
            .ok_or_else(|| Error::Unauthorized("Invalid user token".to_string()))?;

        if requester_id != user_id.to_string() {
            return Err(Error::Forbidden("Cannot update other users".to_string()));
        }

        // Non-admins cannot update roles
        if request.roles.is_some() {
            return Err(Error::Forbidden("Only admins can update roles".to_string()));
        }
    }

    info!(
        "Would update user {}: username={:?}, email={:?}, roles={:?}",
        user_id,
        request.username,
        request.email,
        request.roles
    );

    // In a real service, you would update the database
    Err(Error::Internal("Database not configured - cannot update user".to_string()))
}

/// Delete a user
pub async fn delete_user(
    State(_state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(user_id): Path<Uuid>,
) -> Result<()> {
    info!("Deleting user {} for {}", user_id, claims.sub);

    // Authorization: only admins can delete users
    if !claims.has_role("admin") || !claims.has_permission("delete_user") {
        return Err(Error::Forbidden("Only admins with delete_user permission can delete users".to_string()));
    }

    // In a real service, you would delete from database:
    // let db = state.db().ok_or_else(|| Error::Internal("Database not configured".to_string()))?;
    // sqlx::query!("DELETE FROM users WHERE id = $1", user_id)
    //     .execute(db)
    //     .await?;

    // For now, return an error
    Err(Error::Internal("Not implemented yet".to_string()))
}
