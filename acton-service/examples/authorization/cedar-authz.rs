//! Cedar Authorization Example
//!
//! Demonstrates fine-grained, policy-based access control using AWS Cedar.
//!
//! ## Quick Start
//!
//! ```bash
//! cargo run --example cedar-authz --features cedar-authz,cache
//! ```
//!
//! The example automatically creates all necessary files in `~/.config/acton-service/cedar-authz-example/`.
//!
//! ## Features
//!
//! - JWT authentication + Cedar authorization
//! - Policy-based access control (admin vs user roles)
//! - Resource ownership patterns (users can only access their own documents)
//! - Custom path normalization for alphanumeric IDs
//! - Optional Redis caching (works fine without it)
//!
//! See `examples/CEDAR_EXAMPLE_README.md` for detailed testing instructions.

use acton_service::prelude::*;
use acton_service::middleware::cedar::CedarAuthz;
use axum::{extract::Path, Json};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct Document {
    id: String,
    owner_id: String,
    title: String,
    content: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct User {
    id: String,
    username: String,
    roles: Vec<String>,
}

// Document API handlers - authorization enforced by Cedar policies

async fn list_documents() -> Json<Vec<Document>> {
    Json(vec![
        Document {
            id: "doc1".to_string(),
            owner_id: "user123".to_string(),
            title: "My Document".to_string(),
            content: "Document content".to_string(),
        },
        Document {
            id: "doc2".to_string(),
            owner_id: "user456".to_string(),
            title: "Another Document".to_string(),
            content: "More content".to_string(),
        },
    ])
}

async fn get_document(Path((user_id, doc_id)): Path<(String, String)>) -> Json<Document> {
    // In production, fetch from database
    Json(Document {
        id: doc_id,
        owner_id: user_id,
        title: "Document Title".to_string(),
        content: "Document content here".to_string(),
    })
}

async fn create_document(Json(payload): Json<Document>) -> Json<Document> {
    // In production, set owner_id from JWT claims
    Json(payload)
}

async fn update_document(
    Path((user_id, doc_id)): Path<(String, String)>,
    Json(mut payload): Json<Document>,
) -> Json<Document> {
    payload.id = doc_id;
    payload.owner_id = user_id;
    Json(payload)
}

async fn delete_document(Path((user_id, doc_id)): Path<(String, String)>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "message": format!("Document {} deleted by user {}", doc_id, user_id),
    }))
}

async fn list_users() -> Json<Vec<User>> {
    Json(vec![
        User {
            id: "user123".to_string(),
            username: "alice".to_string(),
            roles: vec!["user".to_string()],
        },
        User {
            id: "user456".to_string(),
            username: "bob".to_string(),
            roles: vec!["user".to_string(), "admin".to_string()],
        },
    ])
}

fn setup_example_files() -> Result<()> {
    use std::path::Path;

    let config_dir = Path::new(&std::env::var("HOME").unwrap())
        .join(".config/acton-service/cedar-authz-example");

    std::fs::create_dir_all(&config_dir)?;

    // Copy policy file (always overwrite to get latest changes)
    let policy_dest = config_dir.join("policies.cedar");
    let policy_src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples/authorization/policies.cedar");
    std::fs::copy(&policy_src, &policy_dest)?;

    // Copy JWT public key (idempotent)
    let jwt_dest = config_dir.join("jwt-public.pem");
    if !jwt_dest.exists() {
        let jwt_src = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("examples/authorization/jwt-public.pem");
        std::fs::copy(&jwt_src, &jwt_dest)?;
    }

    // Create config file with absolute paths (idempotent)
    let config_dest = config_dir.join("config.toml");
    if !config_dest.exists() {
        let config_content = format!(r#"[service]
name = "cedar-authz-example"
port = 8080
host = "127.0.0.1"

[jwt]
public_key_path = "{}/jwt-public.pem"
algorithm = "RS256"

[cedar]
enabled = true
policy_path = "{}/policies.cedar"
hot_reload = false
hot_reload_interval_secs = 60
cache_enabled = false
cache_ttl_secs = 300
fail_open = false

[rate_limit]
enabled = false

[middleware]
timeout_secs = 30
cors_enabled = true
cors_allowed_origins = ["http://localhost:3000"]
"#, config_dir.display(), config_dir.display());

        std::fs::write(&config_dest, config_content)?;
    }

    Ok(())
}

// Custom path normalizer for alphanumeric IDs
//
// The default normalizer only handles UUIDs and numeric IDs.
// This example uses alphanumeric IDs like "user123" and "doc1",
// so we provide a custom normalizer to handle them.
fn normalize_document_paths(path: &str) -> String {
    let doc_pattern = regex::Regex::new(r"^(/api/v[0-9]+/documents/)([a-zA-Z0-9_-]+)/([a-zA-Z0-9_-]+)$").unwrap();

    if let Some(caps) = doc_pattern.captures(path) {
        return format!("{}{{user_id}}/{{doc_id}}", &caps[1]);
    }

    path.to_string()
}

#[tokio::main]
async fn main() -> Result<()> {
    setup_example_files()?;

    println!("🚀 Cedar Authorization Example");
    println!("================================");
    println!();
    println!("Files created: ~/.config/acton-service/cedar-authz-example/");
    println!("  • policies.cedar    - Cedar policy definitions");
    println!("  • jwt-public.pem    - JWT public key for token validation");
    println!("  • config.toml       - Service configuration");
    println!();
    println!("Custom path normalizer: Handles alphanumeric IDs (user123, doc1)");
    println!();
    println!("See examples/authorization/README.md for testing instructions.");
    println!();
    println!("Starting server on http://localhost:8080");
    println!();

    // Define API routes - authorization is enforced by Cedar middleware
    let routes = VersionedApiBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, |routes| {
            routes
                .route("/documents", get(list_documents).post(create_document))
                .route(
                    "/documents/{user_id}/{doc_id}",
                    get(get_document)
                        .put(update_document)
                        .delete(delete_document),
                )
                .route("/admin/users", get(list_users))
        })
        .build_routes();

    let config = Config::load_for_service("cedar-authz-example")?;

    // Build Cedar with custom path normalizer
    // This is necessary because the example uses alphanumeric IDs like "user123" and "doc1"
    // which the default normalizer doesn't handle
    let cedar = CedarAuthz::builder(config.cedar.clone().unwrap())
        .with_path_normalizer(normalize_document_paths)
        .build()
        .await?;

    // Build and start service with Cedar authorization
    let service = ServiceBuilder::new()
        .with_config(config)
        .with_routes(routes)
        .with_cedar(cedar)
        .build();

    service.serve().await?;

    Ok(())
}
