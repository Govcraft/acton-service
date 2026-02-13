//! Pre-built REST handlers for account management
//!
//! Requires feature: `account-handlers`
//!
//! These handlers use `AccountService` from `Extension` or `State`.

use axum::{
    extract::{Path, Query},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Extension, Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::{AccountError, AccountService, AccountStatus, CreateAccount, UpdateAccount};

/// Build the account management routes
///
/// Mounts at whatever prefix you choose:
/// ```rust,ignore
/// let app = Router::new()
///     .nest("/api/v1", account_routes())
///     .layer(Extension(account_service));
/// ```
pub fn account_routes() -> Router {
    Router::new()
        .route("/accounts", post(create_account).get(list_accounts))
        .route(
            "/accounts/{id}",
            get(get_account)
                .patch(update_account)
                .delete(delete_account),
        )
        .route("/accounts/{id}/disable", post(disable_account))
        .route("/accounts/{id}/enable", post(enable_account))
        .route("/accounts/{id}/lock", post(lock_account))
        .route("/accounts/{id}/unlock", post(unlock_account))
        .route("/accounts/{id}/verify-email", post(verify_email))
        .route("/accounts/{id}/change-password", post(change_password))
}

// ============================================================================
// Request/Response types
// ============================================================================

/// Query parameters for listing accounts
#[derive(Debug, Deserialize)]
pub struct ListAccountsQuery {
    /// Filter by status
    pub status: Option<String>,
    /// Maximum results (default: 50)
    pub limit: Option<usize>,
    /// Offset for pagination (default: 0)
    pub offset: Option<usize>,
}

/// Request body for disabling/locking/suspending
#[derive(Debug, Deserialize)]
pub struct ReasonRequest {
    pub reason: String,
}

/// Request body for changing password
#[derive(Debug, Deserialize)]
pub struct ChangePasswordRequest {
    pub new_password: String,
}

/// Account list response
#[derive(Debug, Serialize)]
pub struct AccountListResponse {
    pub accounts: Vec<serde_json::Value>,
    pub total: u64,
    pub limit: usize,
    pub offset: usize,
}

// ============================================================================
// Handlers
// ============================================================================

async fn create_account(
    Extension(svc): Extension<Arc<AccountService>>,
    Json(data): Json<CreateAccount>,
) -> Result<impl IntoResponse, crate::error::Error> {
    let account = svc.create_account(data).await?;
    Ok((StatusCode::CREATED, Json(account)))
}

async fn get_account(
    Extension(svc): Extension<Arc<AccountService>>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, crate::error::Error> {
    let account = svc
        .get_account(&id)
        .await?
        .ok_or_else(|| AccountError::NotFound(id))?;
    Ok(Json(account))
}

async fn update_account(
    Extension(svc): Extension<Arc<AccountService>>,
    Path(id): Path<String>,
    Json(data): Json<UpdateAccount>,
) -> Result<impl IntoResponse, crate::error::Error> {
    let account = svc.update_account(&id, data).await?;
    Ok(Json(account))
}

async fn delete_account(
    Extension(svc): Extension<Arc<AccountService>>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, crate::error::Error> {
    svc.delete_account(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_accounts(
    Extension(svc): Extension<Arc<AccountService>>,
    Query(query): Query<ListAccountsQuery>,
) -> Result<impl IntoResponse, crate::error::Error> {
    let limit = query.limit.unwrap_or(50).min(200);
    let offset = query.offset.unwrap_or(0);
    let status_filter = query
        .status
        .as_deref()
        .and_then(|s| s.parse::<AccountStatus>().ok());

    let accounts = svc.list_accounts(status_filter, limit, offset).await?;
    let total = svc.count_accounts(status_filter).await?;

    Ok(Json(AccountListResponse {
        accounts: accounts
            .into_iter()
            .map(|a| serde_json::to_value(a).unwrap_or_default())
            .collect(),
        total,
        limit,
        offset,
    }))
}

async fn disable_account(
    Extension(svc): Extension<Arc<AccountService>>,
    Path(id): Path<String>,
    Json(body): Json<ReasonRequest>,
) -> Result<impl IntoResponse, crate::error::Error> {
    svc.disable_account(&id, &body.reason).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn enable_account(
    Extension(svc): Extension<Arc<AccountService>>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, crate::error::Error> {
    svc.enable_account(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn lock_account(
    Extension(svc): Extension<Arc<AccountService>>,
    Path(id): Path<String>,
    Json(body): Json<ReasonRequest>,
) -> Result<impl IntoResponse, crate::error::Error> {
    svc.lock_account(&id, &body.reason).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn unlock_account(
    Extension(svc): Extension<Arc<AccountService>>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, crate::error::Error> {
    svc.unlock_account(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn verify_email(
    Extension(svc): Extension<Arc<AccountService>>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, crate::error::Error> {
    svc.verify_email(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn change_password(
    Extension(svc): Extension<Arc<AccountService>>,
    Path(id): Path<String>,
    Json(body): Json<ChangePasswordRequest>,
) -> Result<impl IntoResponse, crate::error::Error> {
    svc.change_password(&id, &body.new_password).await?;
    Ok(StatusCode::NO_CONTENT)
}
