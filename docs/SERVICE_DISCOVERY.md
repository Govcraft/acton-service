# Service Discovery Patterns

This document describes service discovery patterns for microservices built with `acton-service`.

## Table of Contents

- [Overview](#overview)
- [Kubernetes Service Discovery](#kubernetes-service-discovery)
- [Service Mesh Integration](#service-mesh-integration)
- [DNS-Based Discovery](#dns-based-discovery)
- [Client-Side Load Balancing](#client-side-load-balancing)

---

## Overview

Service discovery allows microservices to dynamically find and communicate with each other without hardcoding network locations. The `acton-service` framework supports multiple discovery patterns depending on your deployment environment.

### Key Concepts

- **Service Registry**: Central repository of service locations
- **Health Checks**: Automated endpoint testing to ensure service availability
- **Load Balancing**: Distribute requests across multiple service instances
- **Fail-over**: Automatic routing to healthy instances

---

## Kubernetes Service Discovery

Kubernetes provides built-in service discovery through DNS and environment variables.

### DNS-Based Discovery (Recommended)

Kubernetes automatically creates DNS records for Services:

```
<service-name>.<namespace>.svc.cluster.local
```

#### Example: gRPC Service Communication

```rust
use acton_service::prelude::*;
use tonic::transport::Channel;

#[tokio::main]
async fn main() -> Result<()> {
    // Connect to another service via Kubernetes DNS
    let channel = Channel::from_static("http://auth-service:8081")
        .connect()
        .await?;

    // Use the channel for gRPC calls
    Ok(())
}
```

#### Example: HTTP Service Communication

```rust
use reqwest::Client;

async fn call_user_service(client: &Client) -> Result<String> {
    // Kubernetes DNS automatically resolves to available pods
    let response = client
        .get("http://users-api:8080/v1/users/123")
        .send()
        .await?;

    Ok(response.text().await?)
}
```

### Kubernetes Service Configuration

**Service Manifest** (`k8s/service.yaml`):

```yaml
apiVersion: v1
kind: Service
metadata:
  name: auth-api
  namespace: default
  labels:
    app: auth-api
spec:
  type: ClusterIP
  ports:
    - name: http
      port: 8080
      targetPort: 8080
      protocol: TCP
    - name: grpc
      port: 8081
      targetPort: 8081
      protocol: TCP
  selector:
    app: auth-api
```

### Headless Service for Direct Pod Access

For direct pod-to-pod communication (bypassing kube-proxy):

```yaml
apiVersion: v1
kind: Service
metadata:
  name: auth-api-headless
spec:
  clusterIP: None  # Headless service
  ports:
    - port: 8081
  selector:
    app: auth-api
```

DNS will resolve to individual pod IPs:

```
auth-api-headless.default.svc.cluster.local
-> 10.244.1.5, 10.244.1.6, 10.244.1.7
```

### Environment Variables

Kubernetes injects service endpoints as environment variables:

```bash
AUTH_API_SERVICE_HOST=10.96.0.10
AUTH_API_SERVICE_PORT=8080
```

**Usage in Rust:**

```rust
use std::env;

let auth_host = env::var("AUTH_API_SERVICE_HOST")
    .unwrap_or_else(|_| "auth-api".to_string());
let auth_port = env::var("AUTH_API_SERVICE_PORT")
    .unwrap_or_else(|_| "8080".to_string());

let url = format!("http://{}:{}", auth_host, auth_port);
```

---

## Service Mesh Integration

Service meshes provide advanced service discovery, traffic management, and observability.

### Istio

Istio provides automatic service discovery with advanced routing and load balancing.

#### Installation

```bash
# Install Istio
istioctl install --set profile=default

# Enable sidecar injection for namespace
kubectl label namespace default istio-injection=enabled
```

#### Virtual Service Example

```yaml
apiVersion: networking.istio.io/v1beta1
kind: VirtualService
metadata:
  name: auth-api
spec:
  hosts:
    - auth-api
  http:
    - match:
        - headers:
            version:
              exact: "v2"
      route:
        - destination:
            host: auth-api
            subset: v2
    - route:
        - destination:
            host: auth-api
            subset: v1
```

#### Destination Rule (Load Balancing)

```yaml
apiVersion: networking.istio.io/v1beta1
kind: DestinationRule
metadata:
  name: auth-api
spec:
  host: auth-api
  trafficPolicy:
    loadBalancer:
      consistentHash:
        httpHeaderName: x-user-id  # Consistent hashing
  subsets:
    - name: v1
      labels:
        version: v1
    - name: v2
      labels:
        version: v2
```

### Linkerd

Linkerd is a lightweight service mesh focused on simplicity.

#### Installation

```bash
# Install Linkerd
linkerd install | kubectl apply -f -

# Inject sidecar into deployment
kubectl get deploy auth-api -o yaml | linkerd inject - | kubectl apply -f -
```

Linkerd automatically discovers services and provides:
- Automatic retries
- Circuit breaking
- Load balancing
- Mutual TLS

---

## DNS-Based Discovery

### Configuration in acton-service

Store service URLs in configuration:

**config.toml**:

```toml
[service]
name = "order-api"

[external_services]
auth_service = "http://auth-api:8080"
users_service = "http://users-api:8080"
payments_service = "http://payments-api:8080"
```

**Rust Configuration**:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalServices {
    pub auth_service: String,
    pub users_service: String,
    pub payments_service: String,
}

// In your config struct
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub service: ServiceConfig,
    pub external_services: ExternalServices,
}
```

### Service Client Pattern

Create reusable service clients:

```rust
use reqwest::Client;
use std::sync::Arc;

#[derive(Clone)]
pub struct AuthServiceClient {
    base_url: String,
    client: Arc<Client>,
}

impl AuthServiceClient {
    pub fn new(base_url: String) -> Self {
        Self {
            base_url,
            client: Arc::new(Client::new()),
        }
    }

    pub async fn validate_token(&self, token: &str) -> Result<bool> {
        let url = format!("{}/v1/tokens/validate", self.base_url);
        let response = self.client
            .post(url)
            .bearer_auth(token)
            .send()
            .await?;

        Ok(response.status().is_success())
    }
}
```

---

## Client-Side Load Balancing

For services outside Kubernetes or when you need custom load balancing logic.

### Round-Robin Load Balancer

```rust
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

pub struct LoadBalancer {
    endpoints: Vec<String>,
    counter: Arc<AtomicUsize>,
}

impl LoadBalancer {
    pub fn new(endpoints: Vec<String>) -> Self {
        Self {
            endpoints,
            counter: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn next_endpoint(&self) -> &str {
        let index = self.counter.fetch_add(1, Ordering::Relaxed);
        &self.endpoints[index % self.endpoints.len()]
    }
}

// Usage
let lb = LoadBalancer::new(vec![
    "http://service-1:8080".to_string(),
    "http://service-2:8080".to_string(),
    "http://service-3:8080".to_string(),
]);

let endpoint = lb.next_endpoint();
```

### Health-Aware Load Balancing

```rust
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct HealthAwareLoadBalancer {
    endpoints: Vec<String>,
    health_status: Arc<RwLock<HashMap<String, bool>>>,
}

impl HealthAwareLoadBalancer {
    pub fn new(endpoints: Vec<String>) -> Self {
        let health_status = endpoints
            .iter()
            .map(|e| (e.clone(), true))
            .collect();

        Self {
            endpoints,
            health_status: Arc::new(RwLock::new(health_status)),
        }
    }

    pub async fn next_healthy_endpoint(&self) -> Option<String> {
        let health = self.health_status.read().await;

        self.endpoints
            .iter()
            .find(|e| *health.get(*e).unwrap_or(&false))
            .cloned()
    }

    pub async fn mark_unhealthy(&self, endpoint: &str) {
        let mut health = self.health_status.write().await;
        health.insert(endpoint.to_string(), false);
    }

    pub async fn mark_healthy(&self, endpoint: &str) {
        let mut health = self.health_status.write().await;
        health.insert(endpoint.to_string(), true);
    }
}
```

---

## Best Practices

### 1. Use Kubernetes DNS for Internal Services

```rust
// ✅ Good: Use Kubernetes service DNS
let url = "http://auth-api:8080";

// ❌ Bad: Hardcode pod IPs
let url = "http://10.244.1.5:8080";
```

### 2. Configure Timeouts

```rust
use std::time::Duration;
use reqwest::Client;

let client = Client::builder()
    .timeout(Duration::from_secs(10))
    .connect_timeout(Duration::from_secs(5))
    .build()?;
```

### 3. Implement Retries with Exponential Backoff

```rust
use tryhard::RetryFutureConfig;
use std::time::Duration;

async fn call_with_retry() -> Result<Response> {
    RetryFutureConfig::new(3)
        .exponential_backoff(Duration::from_millis(100))
        .retry_if(|e: &reqwest::Error| e.is_timeout() || e.is_connect())
        .invoke(|| async {
            client.get("http://users-api:8080/health").send().await
        })
        .await
}
```

### 4. Use Circuit Breakers

The `acton-service` framework includes circuit breaker middleware:

```rust
use acton_service::prelude::*;

let resilience = ResilienceConfig {
    circuit_breaker_enabled: true,
    circuit_breaker_threshold: 5,
    circuit_breaker_timeout_secs: 30,
    retry_enabled: true,
    retry_max_attempts: 3,
    ..Default::default()
};
```

### 5. Monitor Service Health

Use the built-in pool metrics:

```rust
// Add to your router
Router::new()
    .route("/health", get(health))
    .route("/ready", get(readiness))
    .route("/metrics/pools", get(pool_metrics))
    .with_state(state)
```

---

## Summary

| Pattern | Use Case | Complexity | Kubernetes Native |
|---------|----------|------------|-------------------|
| **Kubernetes DNS** | Internal service communication | Low | ✅ Yes |
| **Headless Service** | Direct pod access | Medium | ✅ Yes |
| **Istio Service Mesh** | Advanced routing, security | High | ⚠️ Requires Istio |
| **Linkerd Service Mesh** | Simple mesh, mTLS | Medium | ⚠️ Requires Linkerd |
| **Client-Side LB** | External services, custom logic | Medium | ❌ No |

**Recommendation**: Start with Kubernetes DNS for simplicity. Add a service mesh (Linkerd or Istio) when you need advanced features like traffic splitting, mutual TLS, or sophisticated observability.

---

## Further Reading

- [Kubernetes Service Documentation](https://kubernetes.io/docs/concepts/services-networking/service/)
- [Istio Traffic Management](https://istio.io/latest/docs/concepts/traffic-management/)
- [Linkerd Documentation](https://linkerd.io/2/overview/)
- [acton-service Resilience Patterns](./RESILIENCE.md)
