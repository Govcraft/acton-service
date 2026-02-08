//! Database API Example - PostgreSQL Integration with acton-service
//!
//! This example demonstrates:
//! - Database connection pooling with SQLx
//! - Executing queries against PostgreSQL
//! - CRUD operations with typed responses
//! - Error handling for database operations
//! - Integration with the versioned API builder
//!
//! ## Prerequisites
//!
//! Start the PostgreSQL database with Docker:
//!
//! ```bash
//! cd acton-service/examples/database
//! docker compose up -d
//! ```
//!
//! The database will be automatically initialized with tables and seed data.
//!
//! ## Running the Example
//!
//! ```bash
//! # Set the database URL (port 5433 to avoid conflicts with local PostgreSQL)
//! export ACTON_DATABASE_URL="postgres://acton:acton_secret@localhost:5433/acton_example"
//!
//! # Run the example
//! cargo run --example database-api --features database
//! ```
//!
//! ## Testing the API
//!
//! ```bash
//! # Health check
//! curl http://localhost:8080/health
//!
//! # List all products
//! curl http://localhost:8080/api/v1/products
//!
//! # Get a specific product
//! curl http://localhost:8080/api/v1/products/550e8400-e29b-41d4-a716-446655440001
//!
//! # List products by category
//! curl "http://localhost:8080/api/v1/products?category=Electronics"
//!
//! # Create a new product
//! curl -X POST http://localhost:8080/api/v1/products \
//!   -H "Content-Type: application/json" \
//!   -d '{"name": "New Gadget", "description": "A cool new gadget", "price_cents": 4999, "stock_quantity": 100, "category": "Electronics"}'
//!
//! # Update a product
//! curl -X PUT http://localhost:8080/api/v1/products/550e8400-e29b-41d4-a716-446655440001 \
//!   -H "Content-Type: application/json" \
//!   -d '{"name": "Updated Mouse", "price_cents": 3499}'
//!
//! # List all orders
//! curl http://localhost:8080/api/v1/orders
//!
//! # Get order with items
//! curl http://localhost:8080/api/v1/orders/660e8400-e29b-41d4-a716-446655440001
//!
//! # Get database statistics
//! curl http://localhost:8080/api/v1/stats
//! ```
//!
//! ## Cleanup
//!
//! ```bash
//! cd acton-service/examples/database
//! docker compose down -v
//! ```

use acton_service::prelude::*;
use axum::extract::Query as AxumQuery;
use sqlx::{FromRow, PgPool};

// ============================================================================
// Data Models
// ============================================================================

/// Product model matching the database schema
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
struct Product {
    id: Uuid,
    name: String,
    description: Option<String>,
    price_cents: i64,
    stock_quantity: i32,
    category: Option<String>,
    is_active: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

/// Request payload for creating a product
#[derive(Debug, Deserialize)]
struct CreateProductRequest {
    name: String,
    description: Option<String>,
    price_cents: i64,
    stock_quantity: i32,
    category: Option<String>,
}

/// Request payload for updating a product
#[derive(Debug, Deserialize)]
struct UpdateProductRequest {
    name: Option<String>,
    description: Option<String>,
    price_cents: Option<i64>,
    stock_quantity: Option<i32>,
    category: Option<String>,
    is_active: Option<bool>,
}

/// Query parameters for listing products
#[derive(Debug, Deserialize)]
struct ListProductsQuery {
    category: Option<String>,
    active_only: Option<bool>,
    limit: Option<i64>,
}

/// Order model matching the database schema
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
struct Order {
    id: Uuid,
    customer_email: String,
    customer_name: String,
    status: String,
    total_cents: i64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

/// Order item model (unused but kept for schema completeness)
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
struct OrderItem {
    id: Uuid,
    order_id: Uuid,
    product_id: Uuid,
    quantity: i32,
    unit_price_cents: i64,
    created_at: DateTime<Utc>,
}

/// Order with its items
#[derive(Debug, Serialize)]
struct OrderWithItems {
    #[serde(flatten)]
    order: Order,
    items: Vec<OrderItemWithProduct>,
}

/// Order item with product details
#[derive(Debug, Serialize, FromRow)]
struct OrderItemWithProduct {
    id: Uuid,
    quantity: i32,
    unit_price_cents: i64,
    product_id: Uuid,
    product_name: String,
}

/// Database statistics
#[derive(Debug, Serialize)]
struct DatabaseStats {
    total_products: i64,
    active_products: i64,
    total_orders: i64,
    orders_by_status: Vec<StatusCount>,
    total_revenue_cents: i64,
}

#[derive(Debug, Serialize, FromRow)]
struct StatusCount {
    status: String,
    count: i64,
}

// ============================================================================
// Error Handling
// ============================================================================

/// Application error type
#[derive(Debug, Error)]
enum AppError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Database not available")]
    DatabaseUnavailable,

    #[error("Invalid input: {0}")]
    InvalidInput(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AppError::Database(e) => {
                error!("Database error: {}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, "Database error occurred")
            }
            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.as_str()),
            AppError::DatabaseUnavailable => {
                (StatusCode::SERVICE_UNAVAILABLE, "Database is not available")
            }
            AppError::InvalidInput(msg) => (StatusCode::BAD_REQUEST, msg.as_str()),
        };

        (status, Json(serde_json::json!({ "error": message }))).into_response()
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

// Type alias for handler results using our custom error type
type HandlerResult<T> = std::result::Result<T, AppError>;

/// Get database pool from state or return error
async fn get_db(state: &AppState) -> HandlerResult<PgPool> {
    state.db().await.ok_or(AppError::DatabaseUnavailable)
}

// ============================================================================
// Product Handlers
// ============================================================================

/// List all products with optional filtering
#[instrument(skip(state))]
async fn list_products(
    State(state): State<AppState>,
    AxumQuery(query): AxumQuery<ListProductsQuery>,
) -> HandlerResult<Json<Vec<Product>>> {
    let pool = get_db(&state).await?;

    let limit = query.limit.unwrap_or(100).min(1000);
    let active_only = query.active_only.unwrap_or(false);

    let products = match (&query.category, active_only) {
        (Some(category), true) => {
            sqlx::query_as::<_, Product>(
                r#"
                SELECT id, name, description, price_cents, stock_quantity,
                       category, is_active, created_at, updated_at
                FROM products
                WHERE category = $1 AND is_active = true
                ORDER BY name
                LIMIT $2
                "#,
            )
            .bind(category)
            .bind(limit)
            .fetch_all(&pool)
            .await?
        }
        (Some(category), false) => {
            sqlx::query_as::<_, Product>(
                r#"
                SELECT id, name, description, price_cents, stock_quantity,
                       category, is_active, created_at, updated_at
                FROM products
                WHERE category = $1
                ORDER BY name
                LIMIT $2
                "#,
            )
            .bind(category)
            .bind(limit)
            .fetch_all(&pool)
            .await?
        }
        (None, true) => {
            sqlx::query_as::<_, Product>(
                r#"
                SELECT id, name, description, price_cents, stock_quantity,
                       category, is_active, created_at, updated_at
                FROM products
                WHERE is_active = true
                ORDER BY name
                LIMIT $1
                "#,
            )
            .bind(limit)
            .fetch_all(&pool)
            .await?
        }
        (None, false) => {
            sqlx::query_as::<_, Product>(
                r#"
                SELECT id, name, description, price_cents, stock_quantity,
                       category, is_active, created_at, updated_at
                FROM products
                ORDER BY name
                LIMIT $1
                "#,
            )
            .bind(limit)
            .fetch_all(&pool)
            .await?
        }
    };

    info!("Listed {} products", products.len());
    Ok(Json(products))
}

/// Get a specific product by ID
#[instrument(skip(state))]
async fn get_product(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> HandlerResult<Json<Product>> {
    let pool = get_db(&state).await?;

    let product = sqlx::query_as::<_, Product>(
        r#"
        SELECT id, name, description, price_cents, stock_quantity,
               category, is_active, created_at, updated_at
        FROM products
        WHERE id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(&pool)
    .await?
    .ok_or_else(|| AppError::NotFound(format!("Product {} not found", id)))?;

    Ok(Json(product))
}

/// Create a new product
#[instrument(skip(state))]
async fn create_product(
    State(state): State<AppState>,
    Json(req): Json<CreateProductRequest>,
) -> HandlerResult<(StatusCode, Json<Product>)> {
    let pool = get_db(&state).await?;

    if req.name.trim().is_empty() {
        return Err(AppError::InvalidInput(
            "Product name cannot be empty".into(),
        ));
    }

    if req.price_cents < 0 {
        return Err(AppError::InvalidInput("Price cannot be negative".into()));
    }

    let product = sqlx::query_as::<_, Product>(
        r#"
        INSERT INTO products (name, description, price_cents, stock_quantity, category)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id, name, description, price_cents, stock_quantity,
                  category, is_active, created_at, updated_at
        "#,
    )
    .bind(&req.name)
    .bind(&req.description)
    .bind(req.price_cents)
    .bind(req.stock_quantity)
    .bind(&req.category)
    .fetch_one(&pool)
    .await?;

    info!("Created product: {} ({})", product.name, product.id);
    Ok((StatusCode::CREATED, Json(product)))
}

/// Update an existing product
#[instrument(skip(state))]
async fn update_product(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateProductRequest>,
) -> HandlerResult<Json<Product>> {
    let pool = get_db(&state).await?;

    // First check if product exists
    let existing = sqlx::query_as::<_, Product>(
        "SELECT id, name, description, price_cents, stock_quantity, category, is_active, created_at, updated_at FROM products WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&pool)
    .await?
    .ok_or_else(|| AppError::NotFound(format!("Product {} not found", id)))?;

    // Update with provided values or keep existing
    let product = sqlx::query_as::<_, Product>(
        r#"
        UPDATE products
        SET name = $2,
            description = $3,
            price_cents = $4,
            stock_quantity = $5,
            category = $6,
            is_active = $7
        WHERE id = $1
        RETURNING id, name, description, price_cents, stock_quantity,
                  category, is_active, created_at, updated_at
        "#,
    )
    .bind(id)
    .bind(req.name.unwrap_or(existing.name))
    .bind(req.description.or(existing.description))
    .bind(req.price_cents.unwrap_or(existing.price_cents))
    .bind(req.stock_quantity.unwrap_or(existing.stock_quantity))
    .bind(req.category.or(existing.category))
    .bind(req.is_active.unwrap_or(existing.is_active))
    .fetch_one(&pool)
    .await?;

    info!("Updated product: {} ({})", product.name, product.id);
    Ok(Json(product))
}

/// Delete a product
#[instrument(skip(state))]
async fn delete_product(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> HandlerResult<StatusCode> {
    let pool = get_db(&state).await?;

    let result = sqlx::query("DELETE FROM products WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound(format!("Product {} not found", id)));
    }

    info!("Deleted product: {}", id);
    Ok(StatusCode::NO_CONTENT)
}

// ============================================================================
// Order Handlers
// ============================================================================

/// List all orders
#[instrument(skip(state))]
async fn list_orders(State(state): State<AppState>) -> HandlerResult<Json<Vec<Order>>> {
    let pool = get_db(&state).await?;

    let orders = sqlx::query_as::<_, Order>(
        r#"
        SELECT id, customer_email, customer_name, status, total_cents, created_at, updated_at
        FROM orders
        ORDER BY created_at DESC
        LIMIT 100
        "#,
    )
    .fetch_all(&pool)
    .await?;

    info!("Listed {} orders", orders.len());
    Ok(Json(orders))
}

/// Get a specific order with its items
#[instrument(skip(state))]
async fn get_order(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> HandlerResult<Json<OrderWithItems>> {
    let pool = get_db(&state).await?;

    let order = sqlx::query_as::<_, Order>(
        r#"
        SELECT id, customer_email, customer_name, status, total_cents, created_at, updated_at
        FROM orders
        WHERE id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(&pool)
    .await?
    .ok_or_else(|| AppError::NotFound(format!("Order {} not found", id)))?;

    let items = sqlx::query_as::<_, OrderItemWithProduct>(
        r#"
        SELECT oi.id, oi.quantity, oi.unit_price_cents, oi.product_id, p.name as product_name
        FROM order_items oi
        JOIN products p ON oi.product_id = p.id
        WHERE oi.order_id = $1
        ORDER BY oi.created_at
        "#,
    )
    .bind(id)
    .fetch_all(&pool)
    .await?;

    Ok(Json(OrderWithItems { order, items }))
}

// ============================================================================
// Statistics Handler
// ============================================================================

/// Get database statistics
#[instrument(skip(state))]
async fn get_stats(State(state): State<AppState>) -> HandlerResult<Json<DatabaseStats>> {
    let pool = get_db(&state).await?;

    // Get product counts
    let total_products: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM products")
        .fetch_one(&pool)
        .await?;

    let active_products: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM products WHERE is_active = true")
            .fetch_one(&pool)
            .await?;

    // Get order counts
    let total_orders: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM orders")
        .fetch_one(&pool)
        .await?;

    // Get orders by status
    let orders_by_status = sqlx::query_as::<_, StatusCount>(
        r#"
        SELECT status, COUNT(*) as count
        FROM orders
        GROUP BY status
        ORDER BY count DESC
        "#,
    )
    .fetch_all(&pool)
    .await?;

    // Get total revenue (cast to BIGINT since SUM returns NUMERIC)
    let total_revenue: (Option<i64>,) = sqlx::query_as(
        r#"
        SELECT CAST(SUM(total_cents) AS BIGINT)
        FROM orders
        WHERE status IN ('confirmed', 'shipped', 'delivered')
        "#,
    )
    .fetch_one(&pool)
    .await?;

    Ok(Json(DatabaseStats {
        total_products: total_products.0,
        active_products: active_products.0,
        total_orders: total_orders.0,
        orders_by_status,
        total_revenue_cents: total_revenue.0.unwrap_or(0),
    }))
}

// ============================================================================
// Main Entry Point
// ============================================================================

#[tokio::main]
async fn main() -> Result<()> {
    // Build versioned API routes
    let routes = VersionedApiBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, |router| {
            router
                // Product endpoints
                .route("/products", get(list_products).post(create_product))
                .route(
                    "/products/{id}",
                    get(get_product).put(update_product).delete(delete_product),
                )
                // Order endpoints
                .route("/orders", get(list_orders))
                .route("/orders/{id}", get(get_order))
                // Statistics
                .route("/stats", get(get_stats))
        })
        .build_routes();

    // Build and serve the application
    // ServiceBuilder automatically handles:
    // - Configuration loading (including ACTON_DATABASE_URL)
    // - Database connection pool creation
    // - Health and readiness endpoints
    // - Tracing initialization
    ServiceBuilder::new()
        .with_routes(routes)
        .build()
        .serve()
        .await?;

    Ok(())
}
