# Examples

This folder contains example microservices demonstrating different patterns and communication protocols.

## Available Examples

### 1. backend-service + api-gateway (gRPC + REST Communication)

**Purpose**: Demonstrates microservices communicating via both REST and gRPC

**Services**:
- `backend-service/` - Backend with REST API (port 8080) and gRPC server (port 8081)
- `api-gateway/` - Gateway with REST endpoints and gRPC client capabilities

**Features**:
- ✅ Dual-protocol server (HTTP REST + gRPC on separate ports)
- ✅ Shared data store between protocols
- ✅ Proto-based service contracts
- ✅ Production-ready middleware (tracing, timeout, compression, CORS)
- ✅ Graceful shutdown
- ✅ Health checks

**Quick Start**:
```bash
# Start the backend (runs both HTTP and gRPC servers)
cd examples/backend-service
cargo run

# In another terminal, test REST
curl -X POST http://localhost:8080/users \
  -H "Content-Type: application/json" \
  -d '{"name":"Alice","email":"alice@example.com"}'

curl http://localhost:8080/users

# Test gRPC (if you have grpcurl)
grpcurl -plaintext localhost:8081 user.UserService/ListUsers
```

**Key Files**:
- `backend-service/src/grpc_service.rs` - gRPC server implementation
- `backend-service/src/handlers/users.rs` - REST handlers
- `api-gateway/src/grpc_client.rs` - gRPC client helpers
- `../proto/user_service.proto` - Shared service contract

### 2. users-api (Simple REST API)

**Purpose**: Basic REST API example

**To run**:
```bash
cd examples/users-api
cargo run
```

## Architecture Diagram

```
┌─────────────────────────┐          ┌──────────────────────────┐
│     API Gateway         │          │   Backend Service        │
│                         │          │                          │
│  REST Endpoints         │  REST    │  HTTP REST: Port 8080    │
│  (Port 8081)            │◀────────▶│    POST /users           │
│                         │          │    GET  /users           │
│  gRPC Client            │  gRPC    │  gRPC Server: Port 8081  │
│  Helper Functions       │◀────────▶│    CreateUser RPC        │
│                         │          │    GetUser RPC           │
└─────────────────────────┘          │    ListUsers RPC         │
                                     └──────────────────────────┘
                                              ↓
                                     Shared In-Memory Store
```

## Technology Stack

- **Rust** - Systems programming
- **Tokio** - Async runtime
- **Axum 0.6** - HTTP framework
- **Tonic 0.11** - gRPC framework
- **Prost 0.12** - Protobuf serialization
- **Tower** - Middleware composition
- **Tracing** - Structured logging

## Proto Definitions

Shared proto files are in the root `proto/` directory:
- `proto/user_service.proto` - User service RPC definitions

## Next Steps

1. Add database integration (PostgreSQL with sqlx)
2. Add Redis caching layer
3. Add NATS messaging for event-driven communication
4. Add JWT authentication
5. Add Docker and Kubernetes deployment manifests

## Learning Resources

- **REST Communication**: See how `backend-service/src/handlers/users.rs` implements CRUD
- **gRPC Server**: See how `backend-service/src/grpc_service.rs` implements the proto service
- **gRPC Client**: See how `api-gateway/src/grpc_client.rs` connects and calls RPCs
- **Data Sharing**: Both protocols use the same `USER_STORE` - create via REST, read via gRPC!

## Testing

```bash
# Build all examples
cargo build

# Run tests
cargo test

# Run specific example
cargo run --manifest-path examples/backend-service/Cargo.toml
```
