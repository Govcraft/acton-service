//! Event-Driven Microservice Example
//!
//! This example demonstrates the recommended architecture pattern for acton-service:
//! - **HTTP REST API** (port 8080): External interface that publishes events
//! - **gRPC Service** (port 9090): Internal service that consumes events and handles RPC
//! - **Event Bus**: Decouples HTTP endpoints from business logic
//!
//! ## Protocol Buffer Setup
//!
//! This example uses the acton-service build_utils for proto compilation.
//!
//! **In your own project's `build.rs`:**
//! ```rust
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     #[cfg(feature = "grpc")]
//!     {
//!         // Automatically compiles all .proto files in proto/ directory
//!         acton_service::build_utils::compile_service_protos()?;
//!     }
//!     Ok(())
//! }
//! ```
//!
//! **Configure proto location (optional):**
//! ```bash
//! # Override default proto/ directory
//! ACTON_PROTO_DIR=../shared/protos cargo build
//! ```
//!
//! ## Architecture Pattern
//!
//! ```text
//! External Client
//!      │
//!      │ HTTP POST
//!      ▼
//! ┌─────────────────────┐
//! │ HTTP REST Endpoint  │  (Port 8080)
//! │ - Validates request │
//! │ - Publishes event   │
//! │ - Returns 202       │
//! └──────────┬──────────┘
//!            │
//!            │ Publish Event
//!            ▼
//!      Event Bus
//!       │       │
//!       │       └─────────────┐
//!       │                     │
//!       │ Subscribe           │ Subscribe
//!       ▼                     ▼
//! ┌──────────────┐      ┌──────────────┐
//! │ gRPC Service │◄────►│ Other gRPC   │
//! │ (Port 9090)  │ RPC  │ Services     │
//! │ - Processes  │      │              │
//! │   events     │      │              │
//! │ - Exposes    │      │              │
//! │   RPC API    │      │              │
//! └──────────────┘      └──────────────┘
//! ```
//!
//! ## Production Note
//!
//! This example uses **`tokio::sync::broadcast`** for in-memory pub/sub (no external dependencies).
//!
//! **In production microservices**, replace this with a message broker:
//! - **NATS JetStream**: Stream-based messaging, replay, exactly-once (framework default via `events` feature)
//! - **Apache Kafka**: High-throughput event streaming, durable log-based storage
//! - **Redis Streams**: Lightweight, lower latency, good for moderate throughput
//! - **RabbitMQ**: Mature AMQP broker, flexible routing patterns
//! - **Apache Pulsar**: Multi-tenancy, geo-replication
//! - **Cloud providers**: AWS SQS/SNS, Google Pub/Sub, Azure Service Bus
//!
//! For NATS JetStream (framework default): `cargo run --example event-driven --features grpc,events`
//!
//! ## Running This Example
//!
//! ```bash
//! cargo run --example event-driven --features grpc
//! ```
//!
//! ## Testing
//!
//! ```bash
//! # Create an order via HTTP REST API
//! curl -X POST http://localhost:8080/api/v1/orders \
//!   -H "Content-Type: application/json" \
//!   -d '{"item":"laptop","quantity":1}'
//!
//! # Check gRPC service status
//! grpcurl -plaintext localhost:9090 grpc.health.v1.Health/Check
//!
//! # Call gRPC service directly (inter-service communication)
//! grpcurl -plaintext -d '{"order_id":"123"}' \
//!   localhost:9090 orders.v1.OrderService/GetOrderStatus
//! ```

use acton_service::prelude::*;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tonic::{Request, Response, Status};

// ============================================================================
// Event Bus (In-Memory for Example - Use NATS JetStream in Production)
// ============================================================================

/// In-memory event bus using tokio::sync::broadcast
///
/// **PRODUCTION NOTE**: Replace this with a message broker such as:
/// - **NATS JetStream** (framework default): Persistent streams, replay, exactly-once delivery
/// - **Apache Kafka**: High-throughput event streaming, durable log storage
/// - **Redis Streams**: Lightweight, lower latency for moderate workloads
/// - **RabbitMQ**: Mature AMQP broker with flexible routing
///
/// Each offers persistence, cross-service communication, and failure recovery.
#[derive(Clone)]
struct EventBus {
    channels: Arc<RwLock<HashMap<String, broadcast::Sender<Vec<u8>>>>>,
}

impl EventBus {
    fn new() -> Self {
        Self {
            channels: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Publish an event to a topic
    async fn publish(&self, topic: &str, payload: Vec<u8>) -> Result<()> {
        let mut channels = self.channels.write().await;
        let tx = channels
            .entry(topic.to_string())
            .or_insert_with(|| broadcast::channel(1000).0);

        // Send to all subscribers (ignore error if no subscribers)
        let _ = tx.send(payload);

        tracing::info!(topic = topic, "Event published");
        Ok(())
    }

    /// Subscribe to a topic
    async fn subscribe(&self, topic: &str) -> broadcast::Receiver<Vec<u8>> {
        let mut channels = self.channels.write().await;
        let tx = channels
            .entry(topic.to_string())
            .or_insert_with(|| broadcast::channel(1000).0);

        tx.subscribe()
    }
}

// ============================================================================
// Domain Events
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OrderCreatedEvent {
    order_id: String,
    item: String,
    quantity: u32,
    timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OrderProcessedEvent {
    order_id: String,
    status: String,
    timestamp: i64,
}

// ============================================================================
// HTTP REST API (External Interface - Port 8080)
// ============================================================================

#[derive(Debug, Deserialize)]
struct CreateOrderRequest {
    item: String,
    quantity: u32,
}

#[derive(Debug, Serialize)]
struct CreateOrderResponse {
    order_id: String,
    status: String,
    message: String,
}

/// HTTP endpoint that publishes event and returns immediately
///
/// This demonstrates the event-driven pattern:
/// 1. Validate request
/// 2. Publish event to event bus
/// 3. Return 202 Accepted (async processing)
async fn create_order_handler(
    axum::extract::State(event_bus): axum::extract::State<EventBus>,
    Json(req): Json<CreateOrderRequest>,
) -> std::result::Result<
    (axum::http::StatusCode, Json<CreateOrderResponse>),
    (axum::http::StatusCode, String),
> {
    let order_id = uuid::Uuid::new_v4().to_string();

    // Create event
    let event = OrderCreatedEvent {
        order_id: order_id.clone(),
        item: req.item,
        quantity: req.quantity,
        timestamp: chrono::Utc::now().timestamp(),
    };

    // Publish to event bus (in production: NATS JetStream)
    let payload = serde_json::to_vec(&event)
        .map_err(|e| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    event_bus
        .publish("orders.created", payload)
        .await
        .map_err(|e| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    tracing::info!(order_id = %order_id, "Order created via HTTP, event published");

    // Return immediately - processing happens asynchronously
    Ok((
        axum::http::StatusCode::ACCEPTED,
        Json(CreateOrderResponse {
            order_id,
            status: "accepted".to_string(),
            message: "Order accepted for processing".to_string(),
        }),
    ))
}

// ============================================================================
// gRPC Service (Internal API - Port 9090)
// ============================================================================

// Generate protobuf types (you would define orders.proto)
pub mod orders {
    tonic::include_proto!("orders.v1");

    pub const FILE_DESCRIPTOR_SET: &[u8] =
        tonic::include_file_descriptor_set!("orders_descriptor");
}

use orders::{
    order_service_server::{OrderService, OrderServiceServer},
    GetOrderStatusRequest, GetOrderStatusResponse,
};

/// gRPC service implementation
///
/// This service:
/// 1. Consumes events from the event bus (background worker)
/// 2. Exposes gRPC RPC endpoints for inter-service communication
#[derive(Clone)]
struct OrderServiceImpl {
    event_bus: EventBus,
    // In production: This would have database access, etc.
    processed_orders: Arc<RwLock<HashMap<String, String>>>,
}

impl OrderServiceImpl {
    fn new(event_bus: EventBus) -> Self {
        Self {
            event_bus,
            processed_orders: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Background worker that consumes events
    ///
    /// In production with message brokers:
    /// - **NATS JetStream/Kafka**: Subscribe to persistent streams, consumer groups
    /// - **Redis Streams**: Consumer groups with XREADGROUP, pending messages
    /// - **RabbitMQ**: Durable queues with ack/nack, dead-letter exchanges
    /// - All support: Event replay, retries, and failure handling
    async fn start_event_consumer(self) {
        tracing::info!("Starting event consumer for orders.created");

        let mut rx = self.event_bus.subscribe("orders.created").await;

        while let Ok(payload) = rx.recv().await {
            match serde_json::from_slice::<OrderCreatedEvent>(&payload) {
                Ok(event) => {
                    tracing::info!(
                        order_id = %event.order_id,
                        item = %event.item,
                        quantity = event.quantity,
                        "Processing order event"
                    );

                    // Simulate processing
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

                    // Store result
                    self.processed_orders
                        .write()
                        .await
                        .insert(event.order_id.clone(), "processed".to_string());

                    // Publish processed event
                    let processed_event = OrderProcessedEvent {
                        order_id: event.order_id,
                        status: "processed".to_string(),
                        timestamp: chrono::Utc::now().timestamp(),
                    };

                    if let Ok(payload) = serde_json::to_vec(&processed_event) {
                        let _ = self.event_bus.publish("orders.processed", payload).await;
                    }

                    tracing::info!("Order processed successfully");
                }
                Err(e) => {
                    tracing::error!(error = %e, "Failed to deserialize event");
                }
            }
        }
    }
}

#[tonic::async_trait]
impl OrderService for OrderServiceImpl {
    /// gRPC endpoint for inter-service communication
    ///
    /// This is called by OTHER services, not by the HTTP REST API
    async fn get_order_status(
        &self,
        request: Request<GetOrderStatusRequest>,
    ) -> std::result::Result<Response<GetOrderStatusResponse>, Status> {
        let req = request.into_inner();

        tracing::info!(order_id = %req.order_id, "gRPC: GetOrderStatus called");

        let orders = self.processed_orders.read().await;
        let status = orders
            .get(&req.order_id)
            .cloned()
            .unwrap_or_else(|| "unknown".to_string());

        Ok(Response::new(GetOrderStatusResponse {
            order_id: req.order_id,
            status,
        }))
    }
}

// ============================================================================
// Main
// ============================================================================

#[tokio::main]
async fn main() -> Result<()> {
    // Create event bus (in production: NATS JetStream client)
    let event_bus = EventBus::new();

    tracing::info!("🚀 Starting Event-Driven Microservice Example");
    tracing::info!("");
    tracing::info!("Architecture:");
    tracing::info!("  HTTP REST API: http://0.0.0.0:8080 (external interface)");
    tracing::info!("  gRPC Service:  0.0.0.0:9090 (internal service communication)");
    tracing::info!("  Event Bus:     In-memory (use message broker in production)");
    tracing::info!("");
    tracing::info!("Production message brokers:");
    tracing::info!("  • NATS JetStream (framework default)");
    tracing::info!("  • Apache Kafka, Redis Streams, RabbitMQ, etc.");
    tracing::info!("");

    // ========================================================================
    // Start gRPC service with event consumer
    // ========================================================================

    let grpc_addr: std::net::SocketAddr = "0.0.0.0:9090".parse().unwrap();
    let event_bus_clone = event_bus.clone();

    let grpc_task = tokio::spawn(async move {
        tracing::info!("✓ gRPC service starting on {}", grpc_addr);

        let order_service = OrderServiceImpl::new(event_bus_clone.clone());

        // Start background event consumer
        let consumer = order_service.clone();
        tokio::spawn(async move {
            consumer.start_event_consumer().await;
        });

        // Build gRPC server with health and reflection
        let router = acton_service::grpc::server::GrpcServicesBuilder::new()
            .with_health()
            .with_reflection()
            .add_file_descriptor_set(orders::FILE_DESCRIPTOR_SET)
            .add_service(OrderServiceServer::new(order_service))
            .build(None);

        if let Some(router) = router {
            tracing::info!("✓ gRPC service listening on {}", grpc_addr);
            router.serve(grpc_addr).await.expect("gRPC server failed");
        }
    });

    // Wait for gRPC to start
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // ========================================================================
    // Start HTTP REST API
    // ========================================================================

    let http_task = tokio::spawn(async move {
        tracing::info!("✓ HTTP REST API starting on http://0.0.0.0:8080");

        // Wrap event bus for use in handlers via closure
        let event_bus_clone = event_bus.clone();

        // Build HTTP routes
        let routes = VersionedApiBuilder::new()
            .with_base_path("/api")
            .add_version(ApiVersion::V1, move |router| {
                let event_bus_for_handler = event_bus_clone.clone();
                router.route(
                    "/orders",
                    axum::routing::post(move |body| {
                        let eb = event_bus_for_handler.clone();
                        async move { create_order_handler(axum::extract::State(eb), body).await }
                    }),
                )
            })
            .build_routes();

        tracing::info!("✓ HTTP REST API listening on http://0.0.0.0:8080");

        ServiceBuilder::new()
            .with_routes(routes)
            .build()
            .serve()
            .await
            .expect("HTTP server failed");
    });

    // ========================================================================
    // Show usage instructions
    // ========================================================================

    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    tracing::info!("");
    tracing::info!("✨ Services are running!");
    tracing::info!("");
    tracing::info!("📝 Try these commands:");
    tracing::info!("");
    tracing::info!("  # Create an order (publishes event, returns immediately):");
    tracing::info!(
        r#"  curl -X POST http://localhost:8080/api/v1/orders \"#
    );
    tracing::info!(r#"    -H "Content-Type: application/json" \"#);
    tracing::info!(r#"    -d '{{"item":"laptop","quantity":2}}'"#);
    tracing::info!("");
    tracing::info!("  # Check gRPC health:");
    tracing::info!("  grpcurl -plaintext localhost:9090 grpc.health.v1.Health/Check");
    tracing::info!("");
    tracing::info!("  # Call gRPC directly (inter-service communication):");
    tracing::info!(
        r#"  grpcurl -plaintext -d '{{"order_id":"<order-id>"}}' \"#
    );
    tracing::info!("    localhost:9090 orders.v1.OrderService/GetOrderStatus");
    tracing::info!("");
    tracing::info!("  # List available gRPC services:");
    tracing::info!("  grpcurl -plaintext localhost:9090 list");
    tracing::info!("");
    tracing::info!("💡 Production Note:");
    tracing::info!("   This example uses in-memory pub/sub for simplicity.");
    tracing::info!("   In production, use a message broker:");
    tracing::info!("   • NATS JetStream (enable with 'events' feature)");
    tracing::info!("   • Apache Kafka, Redis Streams, RabbitMQ");
    tracing::info!("   • Cloud providers: AWS SQS/SNS, Google Pub/Sub, Azure Service Bus");
    tracing::info!("");

    // Wait for both servers
    tokio::select! {
        _ = grpc_task => tracing::error!("gRPC server stopped"),
        _ = http_task => tracing::error!("HTTP server stopped"),
    }

    Ok(())
}
