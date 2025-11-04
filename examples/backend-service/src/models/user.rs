use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub name: String,
    pub email: String,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateUserRequest {
    pub name: String,
    pub email: String,
}

#[derive(Debug, Serialize)]
pub struct CreateUserResponse {
    pub user: User,
}

#[derive(Debug, Serialize)]
pub struct GetUserResponse {
    pub user: User,
}

#[derive(Debug, Serialize)]
pub struct ListUsersResponse {
    pub users: Vec<User>,
    pub total: usize,
}
