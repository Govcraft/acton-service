# Event-Driven Architecture Examples

Examples demonstrating event-driven patterns and asynchronous communication in microservices.

## Examples

### event-driven.rs

**HTTP REST API + gRPC Service with Event Bus**

Demonstrates the recommended architecture pattern for acton-service:
- **HTTP REST API** (port 8080): External interface that publishes events
- **gRPC Service** (port 9090): Internal service that consumes events and handles RPC
- **Event Bus**: Decouples HTTP endpoints from business logic

Key concepts:
- Event publishing from HTTP endpoints
- Event consumption in gRPC handlers
- Decoupled microservice communication
- Async event processing

Run with:
```bash
cargo run --manifest-path=../../Cargo.toml --example event-driven --features grpc
```

## Architecture Pattern

```
HTTP Client → REST API (8080) → Event Bus → gRPC Service (9090)
                    ↓                              ↓
              Publish Event                  Consume Event
                                              Process Logic
```

This pattern provides:
- **Decoupling**: HTTP layer doesn't know about business logic
- **Scalability**: Events can be processed asynchronously
- **Testability**: Components can be tested independently
- **Flexibility**: Easy to add new event consumers

## Prerequisites

Requires the `grpc` feature flag:
```bash
cargo run --manifest-path=../../Cargo.toml --example event-driven --features grpc
```

## Testing

### Trigger an event via HTTP

```bash
curl -X POST http://localhost:8080/api/v1/events \
  -H "Content-Type: application/json" \
  -d '{"name": "test-event", "payload": "some data"}'
```

The HTTP API publishes the event to the bus, and the gRPC service consumes and processes it.

### Check processing via gRPC

```bash
grpcurl -plaintext localhost:9090 list
grpcurl -plaintext -d '{"event_id": "123"}' \
  localhost:9090 events.EventService/GetEventStatus
```

## Use Cases

Event-driven architecture is ideal for:
- Microservices that need to communicate asynchronously
- Systems with multiple consumers for the same events
- Applications requiring eventual consistency
- Services that need to scale independently

## Next Steps

- Explore different event bus implementations (NATS, Kafka, RabbitMQ)
- Add event persistence and replay capabilities
- Implement event sourcing patterns
- See the main acton-service documentation for advanced event patterns
