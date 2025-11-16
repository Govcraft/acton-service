---
title: Health Checks
nextjs:
  metadata:
    title: Automatic Health Checks
    description: Kubernetes-ready liveness and readiness probes with dependency monitoring
---

Health checks are essential for production microservices, enabling orchestrators like Kubernetes to monitor service health and make informed decisions about traffic routing and container lifecycle management. acton-service includes automatic health and readiness endpoints that follow Kubernetes best practices out of the box.

---

## Liveness vs Readiness

Understanding the difference between liveness and readiness probes is critical for reliable service deployment:

| Probe Type | Purpose | When to Fail | Kubernetes Action |
|------------|---------|--------------|-------------------|
| **Liveness** (`/health`) | Is the process alive and able to serve requests? | Never, unless the process is completely broken (panic, deadlock) | Restart the container |
| **Readiness** (`/ready`) | Is the service ready to accept traffic? | When dependencies are unavailable (database down, cache unreachable) | Remove from load balancer, stop routing traffic |

**Key Principle**: A liveness probe failure indicates the container should be killed and restarted. A readiness probe failure indicates the service should be temporarily removed from the load balancer but continue running.

---

## Automatic Endpoints

acton-service includes both health endpoints automatically when you build a service. No additional code is required:

```rust
use acton_service::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    let routes = VersionedApiBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, |router| {
            router.route("/hello", get(|| async { "Hello!" }))
        })
        .build_routes();

    // Health checks are automatic - no code needed
    ServiceBuilder::new()
        .with_routes(routes)
        .build()
        .serve()
        .await
}
```

**Available Endpoints:**

### `GET /health` - Liveness Probe

Returns HTTP 200 if the process is alive and can handle requests.

**Response (Success):**
```json
{
  "status": "healthy",
  "service": "my-service",
  "version": "0.5.2"
}
```

This endpoint almost always returns 200 OK. It's designed to detect catastrophic failures like:
- Process deadlock
- Out of memory errors
- Critical panics that prevent request handling

### `GET /ready` - Readiness Probe

Returns HTTP 200 if the service and all **required** dependencies are healthy. Returns HTTP 503 if any required dependency is unavailable.

**Response (Success):**
```json
{
  "ready": true,
  "service": "my-service",
  "dependencies": {
    "database": "healthy",
    "redis": "healthy"
  }
}
```

**Response (Dependency Failure):**
```json
{
  "ready": false,
  "service": "my-service",
  "dependencies": {
    "database": "unhealthy",
    "redis": "healthy"
  }
}
```

---

## Dependency Monitoring

The readiness endpoint automatically checks the health of configured dependencies. You can mark dependencies as **required** or **optional** to control readiness behavior:

### Configuration

Define dependencies in your `config.toml` with the `optional` flag:

```toml
# ~/.config/acton-service/my-service/config.toml

[database]
url = "postgres://localhost/mydb"
max_connections = 50
optional = false  # REQUIRED: Readiness fails if database is down

[redis]
url = "redis://localhost:6379"
pool_size = 20
optional = true   # OPTIONAL: Readiness succeeds even if Redis is down

[nats]
url = "nats://localhost:4222"
optional = false  # REQUIRED: Readiness fails if NATS is unavailable
```

### Dependency Check Behavior

| Dependency State | `optional = false` | `optional = true` |
|------------------|--------------------|--------------------|
| Healthy | `/ready` returns 200 | `/ready` returns 200 |
| Unhealthy | `/ready` returns 503 | `/ready` returns 200 (dependency reported as degraded) |

**Use Cases:**

- **Required dependencies** (`optional = false`): Primary database, message queue for critical workflows, authentication service
- **Optional dependencies** (`optional = true`): Cache layers (Redis), analytics systems, non-critical external APIs

When a dependency is marked as optional and fails, the service remains ready to serve traffic, but the degraded state is logged and reported in the readiness response.

---

## Kubernetes Integration

Health and readiness probes integrate seamlessly with Kubernetes deployments. Here's a complete example:

### Deployment Manifest

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: my-service
  namespace: production
spec:
  replicas: 3
  selector:
    matchLabels:
      app: my-service
  template:
    metadata:
      labels:
        app: my-service
        version: v1.0.0
    spec:
      containers:
      - name: my-service
        image: my-service:1.0.0
        ports:
        - name: http
          containerPort: 8080
          protocol: TCP
        env:
        - name: ACTON_SERVICE_PORT
          value: "8080"
        - name: ACTON_DATABASE_URL
          valueFrom:
            secretKeyRef:
              name: db-credentials
              key: url
        - name: ACTON_REDIS_URL
          valueFrom:
            secretKeyRef:
              name: cache-credentials
              key: url

        # Liveness probe: Restart container if process is broken
        livenessProbe:
          httpGet:
            path: /health
            port: http
          initialDelaySeconds: 30    # Wait 30s after startup before first check
          periodSeconds: 10           # Check every 10 seconds
          timeoutSeconds: 5           # Fail if no response in 5 seconds
          failureThreshold: 3         # Restart after 3 consecutive failures

        # Readiness probe: Remove from load balancer if dependencies are down
        readinessProbe:
          httpGet:
            path: /ready
            port: http
          initialDelaySeconds: 5      # Check readiness 5s after startup
          periodSeconds: 5            # Check every 5 seconds
          timeoutSeconds: 3           # Fail if no response in 3 seconds
          failureThreshold: 2         # Mark unready after 2 consecutive failures
          successThreshold: 1         # Mark ready after 1 successful check

        resources:
          requests:
            memory: "256Mi"
            cpu: "100m"
          limits:
            memory: "512Mi"
            cpu: "500m"

---
apiVersion: v1
kind: Service
metadata:
  name: my-service
  namespace: production
spec:
  type: ClusterIP
  selector:
    app: my-service
  ports:
  - name: http
    port: 80
    targetPort: http
    protocol: TCP
```

### Probe Configuration Best Practices

**Liveness Probe:**
- **initialDelaySeconds**: Set to allow service startup (database migrations, cache warming). Start with 30-60 seconds.
- **periodSeconds**: Check infrequently (10-30 seconds) to avoid overhead.
- **failureThreshold**: Use 3+ to avoid false positives from temporary issues.

**Readiness Probe:**
- **initialDelaySeconds**: Set low (5-10 seconds) to bring new pods into rotation quickly.
- **periodSeconds**: Check frequently (5-10 seconds) to detect dependency failures quickly.
- **failureThreshold**: Use 2-3 to balance responsiveness and stability.

---

## Testing Health Endpoints

You can test health endpoints directly using curl or any HTTP client:

### Check Liveness

```bash
curl http://localhost:8080/health

# Response
{
  "status": "healthy",
  "timestamp": "2025-11-16T10:30:00Z"
}
```

### Check Readiness

```bash
curl http://localhost:8080/ready

# Response (all dependencies healthy)
{
  "ready": true,
  "service": "my-service",
  "dependencies": {
    "database": "healthy",
    "redis": "healthy",
    "nats": "healthy"
  }
}
```

### Simulate Dependency Failure

To test readiness behavior when dependencies fail, temporarily stop a required dependency:

```bash
# Stop Redis
docker stop redis

# Check readiness
curl http://localhost:8080/ready

# Response (if Redis is required)
{
  "ready": false,
  "service": "my-service",
  "dependencies": {
    "database": "healthy",
    "redis": "unhealthy",
    "nats": "healthy"
  }
}

# HTTP status code: 503 Service Unavailable
```

### Kubernetes Pod Testing

Test probes from within a Kubernetes cluster:

```bash
# Get pod name
kubectl get pods -l app=my-service

# Check liveness
kubectl exec -it my-service-7d8f9c5b6-abc12 -- curl localhost:8080/health

# Check readiness
kubectl exec -it my-service-7d8f9c5b6-abc12 -- curl localhost:8080/ready

# View probe failures in events
kubectl describe pod my-service-7d8f9c5b6-abc12
```

---

## Custom Health Checks

While acton-service provides automatic dependency health checks, you can also implement custom health check logic for advanced scenarios:

### Adding Custom Health Checks

For application-specific health checks (e.g., "can we reach a critical external API?"), you can add custom endpoints or extend the default health check behavior. This is covered in the [Advanced Configuration](/docs/advanced-configuration) guide.

**Common Custom Health Checks:**
- External API availability
- Message queue consumer status
- Background worker health
- Cache hit rate thresholds
- Circuit breaker state monitoring

---

## Next Steps

- See [Configuration](/docs/configuration) for dependency setup and `optional` flag usage
- Learn about [Kubernetes Deployment](/docs/kubernetes) best practices
- Explore [Observability](/docs/observability) for metrics and distributed tracing
- Review [Resilience Patterns](/docs/resilience) for circuit breakers and retry logic

---

Health checks are a critical foundation for production microservices. With acton-service, they're included automatically and follow industry best practices, allowing you to focus on building features rather than infrastructure.
