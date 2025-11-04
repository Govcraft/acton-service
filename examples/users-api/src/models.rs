//! Data models for users API

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// User model
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub email: String,
    pub roles: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Create user request
#[derive(Debug, Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
    pub email: String,
    pub roles: Option<Vec<String>>,
}

impl CreateUserRequest {
    /// Validate the create user request
    pub fn validate(&self) -> Result<(), String> {
        if self.username.is_empty() {
            return Err("Username cannot be empty".to_string());
        }
        if self.email.is_empty() || !self.email.contains('@') {
            return Err("Invalid email address".to_string());
        }
        Ok(())
    }

    /// Get roles or default to ["user"]
    pub fn roles(&self) -> Vec<String> {
        self.roles.clone().unwrap_or_else(|| vec!["user".to_string()])
    }
}

/// Update user request
#[derive(Debug, Deserialize)]
pub struct UpdateUserRequest {
    pub username: Option<String>,
    pub email: Option<String>,
    pub roles: Option<Vec<String>>,
}

impl UpdateUserRequest {
    /// Check if the request has any updates
    pub fn has_updates(&self) -> bool {
        self.username.is_some() || self.email.is_some() || self.roles.is_some()
    }

    /// Validate the update request
    pub fn validate(&self) -> Result<(), String> {
        if let Some(email) = &self.email {
            if email.is_empty() || !email.contains('@') {
                return Err("Invalid email address".to_string());
            }
        }
        if let Some(username) = &self.username {
            if username.is_empty() {
                return Err("Username cannot be empty".to_string());
            }
        }
        Ok(())
    }
}

/// User response
#[derive(Debug, Serialize)]
pub struct UserResponse {
    pub id: Uuid,
    pub username: String,
    pub email: String,
    pub roles: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<User> for UserResponse {
    fn from(user: User) -> Self {
        Self {
            id: user.id,
            username: user.username,
            email: user.email,
            roles: user.roles,
            created_at: user.created_at,
            updated_at: user.updated_at,
        }
    }
}

/// List users response
#[derive(Debug, Serialize)]
pub struct ListUsersResponse {
    pub users: Vec<UserResponse>,
    pub total: usize,
}
