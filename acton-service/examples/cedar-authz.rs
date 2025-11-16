//! Cedar Authorization Example - Fine-grained Policy-based Access Control
//!
//! This example demonstrates AWS Cedar authorization integration with acton-service.
//! It shows how to implement fine-grained, policy-based access control using Cedar.
//!
//! Features demonstrated:
//! - JWT authentication + Cedar authorization (layered security)
//! - Policy-based access control with Cedar policies
//! - Resource ownership patterns
//! - Role-based access (admin vs user)
//! - HTTP middleware integration
//! - Redis caching for policy decisions
//! - Custom path normalization for Cedar actions
//!
//! Prerequisites:
//! 1. Create a Cedar policy file at ~/.config/acton-service/cedar-authz-example/policies.cedar
//! 2. Create a JWT public key at ~/.config/acton-service/cedar-authz-example/jwt-public.pem
//! 3. Start Redis (optional, for caching): docker run -d -p 6379:6379 redis
//!
//! Run with: cargo run --example cedar-authz --features cedar-authz,cache
//!
//! Test with:
//!   # Generate a test JWT token (you'll need your own JWT for real testing)
//!   export TOKEN="your-jwt-token-here"
//!
//!   # Test with admin user (should succeed)
//!   curl -H "Authorization: Bearer $TOKEN" http://localhost:8080/api/v1/documents
//!
//!   # Test document ownership (user can only access their own documents)
//!   curl -H "Authorization: Bearer $TOKEN" http://localhost:8080/api/v1/documents/user123/doc1
//!
//!   # Test admin-only endpoint (requires admin role)
//!   curl -H "Authorization: Bearer $TOKEN" http://localhost:8080/api/v1/admin/users

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

// V1 Handlers

/// List all documents (admin only via Cedar policy)
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

/// Get a specific document (owner or admin only via Cedar policy)
async fn get_document(
    Path((user_id, doc_id)): Path<(String, String)>,
) -> Json<Document> {
    // In production, you would fetch this from a database
    // Cedar policies will ensure the user has access
    Json(Document {
        id: doc_id,
        owner_id: user_id,
        title: "Document Title".to_string(),
        content: "Document content here".to_string(),
    })
}

/// Create a new document (any authenticated user)
async fn create_document(Json(payload): Json<Document>) -> Json<Document> {
    // Cedar will verify the user is authenticated
    // In production, set owner_id from JWT claims
    Json(payload)
}

/// Update a document (owner only via Cedar policy)
async fn update_document(
    Path((user_id, doc_id)): Path<(String, String)>,
    Json(mut payload): Json<Document>,
) -> Json<Document> {
    payload.id = doc_id;
    payload.owner_id = user_id;
    Json(payload)
}

/// Delete a document (owner or admin only via Cedar policy)
async fn delete_document(
    Path((user_id, doc_id)): Path<(String, String)>,
) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "message": format!("Document {} deleted by user {}", doc_id, user_id),
    }))
}

/// List all users (admin only via Cedar policy)
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

/// Setup function to copy example files to config directory
fn setup_example_files() -> Result<()> {
    use std::path::Path;

    println!("🔧 Setting up example files...");

    // Get the config directory
    let config_dir = Path::new(&std::env::var("HOME").unwrap())
        .join(".config/acton-service/cedar-authz-example");

    // Create config directory if it doesn't exist
    std::fs::create_dir_all(&config_dir)?;

    // Copy policy file (always overwrite to get latest changes)
    let policy_dest = config_dir.join("policies.cedar");
    let policy_src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples/policies.cedar");
    std::fs::copy(&policy_src, &policy_dest)?;
    println!("   ✓ Copied policies.cedar to {:?}", policy_dest);

    // Copy JWT public key (idempotent)
    let jwt_dest = config_dir.join("jwt-public.pem");
    if !jwt_dest.exists() {
        let jwt_src = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("examples/jwt-public.pem");
        std::fs::copy(&jwt_src, &jwt_dest)?;
        println!("   ✓ Copied jwt-public.pem to {:?}", jwt_dest);
    } else {
        println!("   ✓ jwt-public.pem already exists");
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
        println!("   ✓ Created config.toml at {:?}", config_dest);
    } else {
        println!("   ✓ config.toml already exists");
    }

    println!();
    Ok(())
}

// ============================================================================
// Custom Path Normalization for Alphanumeric IDs
// ============================================================================
//
// IMPORTANT: This example uses alphanumeric IDs like "user123" and "doc1" in the URLs.
// The default path normalizer only handles UUIDs and numeric IDs, NOT alphanumeric strings.
//
// This example demonstrates using a custom path normalizer with ServiceBuilder
// to handle alphanumeric document IDs in the URLs.

/// Custom path normalizer for alphanumeric document IDs
/// Handles paths like "/api/v1/documents/user123/doc1" -> "/api/v1/documents/{user_id}/{doc_id}"
fn normalize_document_paths(path: &str) -> String {
    // Match the pattern: /api/v1/documents/{user_id}/{doc_id}
    // This regex handles alphanumeric IDs like "user123", "doc1", etc.
    let doc_pattern = regex::Regex::new(r"^(/api/v[0-9]+/documents/)([a-zA-Z0-9_-]+)/([a-zA-Z0-9_-]+)$").unwrap();

    if let Some(caps) = doc_pattern.captures(path) {
        return format!("{}{{user_id}}/{{doc_id}}", &caps[1]);
    }

    // Fallback to original path if no match
    path.to_string()
}
//
// When to use custom normalization:
// - Alphanumeric IDs (like "user123", "doc1") that aren't UUIDs or numeric
// - Slug-based routes (like "/articles/my-article-title")
// - Complex path patterns not handled by the default normalizer
// - Non-Axum frameworks where MatchedPath isn't available
// - API gateways or proxies that need explicit path normalization

#[tokio::main]
async fn main() -> Result<()> {
    // Setup example files first
    setup_example_files()?;

    println!("🚀 Cedar Authorization Example");
    println!("================================");
    println!();
    println!("Example files copied to: ~/.config/acton-service/cedar-authz-example/");
    println!();
    println!("Note: Redis is optional but recommended for caching.");
    println!("      Start with: docker run -d -p 6379:6379 redis");
    println!();
    println!("Path Normalization:");
    println!("  ✓  Using custom normalize_document_paths() function");
    println!("  ✓  Handles alphanumeric IDs (user123, doc1)");
    println!("  ✓  Normalizes to {{user_id}}/{{doc_id}} placeholders");
    println!("  ℹ️  Default normalizer only handles UUIDs and numeric IDs");
    println!();
    println!("Example policy file content:");
    println!("---");
    println!(r#"
// Admin can do everything
permit(
    principal,
    action,
    resource
)
when {{
    principal.roles.contains("admin")
}};

// Users can list documents
permit(
    principal,
    action == Action::"GET /api/v1/documents",
    resource
);

// Users can create their own documents
permit(
    principal,
    action == Action::"POST /api/v1/documents",
    resource
);

// Users with document permissions can access documents
permit(
    principal,
    action in [
        Action::"GET /api/v1/documents/{{user_id}}/{{doc_id}}",
        Action::"PUT /api/v1/documents/{{user_id}}/{{doc_id}}",
        Action::"DELETE /api/v1/documents/{{user_id}}/{{doc_id}}"
    ],
    resource
)
when {{
    principal.permissions.contains("write:documents")
}};
"#);
    println!("---");
    println!();
    println!("Starting server on http://localhost:8080");
    println!();

    // Build versioned API with Cedar authorization
    let routes = VersionedApiBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, |routes| {
            routes
                // Document endpoints (protected by Cedar)
                .route("/documents", get(list_documents).post(create_document))
                .route(
                    "/documents/{user_id}/{doc_id}",
                    get(get_document)
                        .put(update_document)
                        .delete(delete_document),
                )
                // Admin endpoints (protected by Cedar)
                .route("/admin/users", get(list_users))
        })
        .build_routes();

    // Load configuration for this specific service
    let config = Config::load_for_service("cedar-authz-example")?;

    // Build Cedar with custom path normalizer using the builder pattern
    let cedar = CedarAuthz::builder(config.cedar.clone().unwrap())
        .with_path_normalizer(normalize_document_paths)
        .build()
        .await?;

    // Build service with explicit Cedar instance
    let service = ServiceBuilder::new()
        .with_config(config)
        .with_routes(routes)
        .with_cedar(cedar)  // Explicit Cedar with custom normalizer
        .build();

    service.serve().await?;

    Ok(())
}
