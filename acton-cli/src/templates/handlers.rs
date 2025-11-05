pub fn generate_handlers_mod() -> String {
r#"// Add your handler modules here
// Example:
// pub mod users;
"#.to_string()
}

pub fn generate_example_handler() -> String {
r#"use acton_service::prelude::*;
use axum::{
    extract::State,
    Json,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    status: String,
    version: String,
}

/// Example health check handler
pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "healthy".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

// TODO: Add your handlers here
// Example:
//
// #[derive(Debug, Deserialize)]
// pub struct CreateUserRequest {
//     username: String,
//     email: String,
// }
//
// #[derive(Debug, Serialize)]
// pub struct User {
//     id: String,
//     username: String,
//     email: String,
// }
//
// pub async fn create_user(
//     State(state): State<AppState>,
//     Json(req): Json<CreateUserRequest>,
// ) -> Result<Json<User>, AppError> {
//     // TODO: Implement user creation
//     todo!("Implement create_user handler")
// }
"#.to_string()
}
