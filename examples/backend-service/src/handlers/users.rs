use axum::{extract::{Path, State}, http::StatusCode, Json, response::IntoResponse};
use chrono::Utc;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::models::{User, CreateUserRequest, CreateUserResponse, GetUserResponse, ListUsersResponse};
use crate::AppState;

// In-memory user store (for demonstration)
#[derive(Clone)]
pub struct UserStore {
    users: Arc<Mutex<HashMap<String, User>>>,
}

impl Default for UserStore {
    fn default() -> Self {
        Self::new()
    }
}

impl UserStore {
    pub fn new() -> Self {
        Self {
            users: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn insert(&self, user: User) {
        let mut users = self.users.lock().await;
        users.insert(user.id.clone(), user);
    }

    pub async fn get(&self, id: &str) -> Option<User> {
        let users = self.users.lock().await;
        users.get(id).cloned()
    }

    pub async fn list(&self) -> Vec<User> {
        let users = self.users.lock().await;
        users.values().cloned().collect()
    }
}

// Global store (in a real app, this would be in AppState)
// Made public so it can be shared with gRPC service
lazy_static::lazy_static! {
    pub static ref USER_STORE: UserStore = UserStore::new();
}

/// Create a new user
pub async fn create_user(
    State(_state): State<AppState>,
    Json(payload): Json<CreateUserRequest>,
) -> impl IntoResponse {
    tracing::info!("Creating user: {} <{}>", payload.name, payload.email);

    let user = User {
        id: Uuid::new_v4().to_string(),
        name: payload.name,
        email: payload.email,
        created_at: Utc::now().to_rfc3339(),
    };

    USER_STORE.insert(user.clone()).await;

    tracing::info!("User created with ID: {}", user.id);
    (StatusCode::CREATED, Json(CreateUserResponse { user }))
}

/// Get a user by ID
pub async fn get_user(
    State(_state): State<AppState>,
    Path(user_id): Path<String>,
) -> Result<Json<GetUserResponse>, StatusCode> {
    tracing::info!("Getting user: {}", user_id);

    match USER_STORE.get(&user_id).await {
        Some(user) => {
            tracing::info!("User found: {}", user.id);
            Ok(Json(GetUserResponse { user }))
        }
        None => {
            tracing::warn!("User not found: {}", user_id);
            Err(StatusCode::NOT_FOUND)
        }
    }
}

/// List all users
pub async fn list_users(
    State(_state): State<AppState>,
) -> Json<ListUsersResponse> {
    tracing::info!("Listing all users");

    let users = USER_STORE.list().await;
    let total = users.len();

    tracing::info!("Returning {} users", total);
    Json(ListUsersResponse { users, total })
}
