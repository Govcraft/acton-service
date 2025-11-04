use axum::{extract::{Path, State}, http::StatusCode, Json, response::IntoResponse};
use reqwest::Client;

use crate::models::{CreateUserRequest, CreateUserResponse, GetUserResponse, ListUsersResponse};
use crate::AppState;

// Backend service URL - in production, this should come from config
const BACKEND_URL: &str = "http://localhost:8080";

/// Proxy: Create user via backend service
pub async fn create_user_proxy(
    State(_state): State<AppState>,
    Json(payload): Json<CreateUserRequest>,
) -> impl IntoResponse {
    tracing::info!("API Gateway: Proxying create user request to backend");

    let client = Client::new();
    let url = format!("{}/users", BACKEND_URL);

    match client.post(&url)
        .json(&payload)
        .send()
        .await
    {
        Ok(response) => {
            if response.status().is_success() {
                match response.json::<CreateUserResponse>().await {
                    Ok(user_response) => {
                        tracing::info!("API Gateway: User created via backend: {}", user_response.user.id);
                        Ok((StatusCode::CREATED, Json(user_response)))
                    }
                    Err(e) => {
                        tracing::error!("API Gateway: Failed to parse backend response: {}", e);
                        Err(StatusCode::INTERNAL_SERVER_ERROR)
                    }
                }
            } else {
                tracing::error!("API Gateway: Backend returned error: {}", response.status());
                Err(StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR))
            }
        }
        Err(e) => {
            tracing::error!("API Gateway: Failed to connect to backend: {}", e);
            Err(StatusCode::SERVICE_UNAVAILABLE)
        }
    }
}

/// Proxy: Get user by ID via backend service
pub async fn get_user_proxy(
    State(_state): State<AppState>,
    Path(user_id): Path<String>,
) -> impl IntoResponse {
    tracing::info!("API Gateway: Proxying get user request to backend: {}", user_id);

    let client = Client::new();
    let url = format!("{}/users/{}", BACKEND_URL, user_id);

    match client.get(&url).send().await {
        Ok(response) => {
            if response.status().is_success() {
                match response.json::<GetUserResponse>().await {
                    Ok(user_response) => {
                        tracing::info!("API Gateway: User retrieved from backend: {}", user_response.user.id);
                        Ok(Json(user_response))
                    }
                    Err(e) => {
                        tracing::error!("API Gateway: Failed to parse backend response: {}", e);
                        Err(StatusCode::INTERNAL_SERVER_ERROR)
                    }
                }
            } else {
                tracing::warn!("API Gateway: User not found in backend: {}", user_id);
                Err(StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::NOT_FOUND))
            }
        }
        Err(e) => {
            tracing::error!("API Gateway: Failed to connect to backend: {}", e);
            Err(StatusCode::SERVICE_UNAVAILABLE)
        }
    }
}

/// Proxy: List users via backend service
pub async fn list_users_proxy(
    State(_state): State<AppState>,
) -> impl IntoResponse {
    tracing::info!("API Gateway: Proxying list users request to backend");

    let client = Client::new();
    let url = format!("{}/users", BACKEND_URL);

    match client.get(&url).send().await {
        Ok(response) => {
            if response.status().is_success() {
                match response.json::<ListUsersResponse>().await {
                    Ok(users_response) => {
                        tracing::info!("API Gateway: Retrieved {} users from backend", users_response.total);
                        Ok(Json(users_response))
                    }
                    Err(e) => {
                        tracing::error!("API Gateway: Failed to parse backend response: {}", e);
                        Err(StatusCode::INTERNAL_SERVER_ERROR)
                    }
                }
            } else {
                tracing::error!("API Gateway: Backend returned error: {}", response.status());
                Err(StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR))
            }
        }
        Err(e) => {
            tracing::error!("API Gateway: Failed to connect to backend: {}", e);
            Err(StatusCode::SERVICE_UNAVAILABLE)
        }
    }
}
