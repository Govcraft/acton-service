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

#[tokio::main]
async fn main() -> Result<()> {
    println!("🚀 Cedar Authorization Example");
    println!("================================");
    println!();
    println!("Prerequisites:");
    println!("1. Cedar policy file: ~/.config/acton-service/cedar-authz-example/policies.cedar");
    println!("2. JWT public key: ~/.config/acton-service/cedar-authz-example/jwt-public.pem");
    println!("3. Redis running on localhost:6379 (optional, for caching)");
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

// Users can access their own documents
permit(
    principal,
    action in [
        Action::"GET /api/v1/documents/{{user_id}}/{{doc_id}}",
        Action::"PUT /api/v1/documents/{{user_id}}/{{doc_id}}",
        Action::"DELETE /api/v1/documents/{{user_id}}/{{doc_id}}"
    ],
    resource
)
when {{{{
    principal.sub == resource.owner_id
}}}};
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
    // This will look for config at:
    // 1. ./config.toml
    // 2. ~/.config/acton-service/cedar-authz-example/config.toml (recommended)
    // 3. /etc/acton-service/cedar-authz-example/config.toml
    let config = Config::load_for_service("cedar-authz-example")?;

    // Build and serve with Cedar authorization
    // ServiceBuilder will:
    // 1. Use the loaded config
    // 2. Initialize JWT authentication middleware
    // 3. Initialize Cedar authorization middleware (if enabled in config)
    // 4. Setup Redis caching for policy decisions (if cache feature enabled)
    ServiceBuilder::new()
        .with_config(config)
        .with_routes(routes)
        .build()
        .serve()
        .await?;

    Ok(())
}
