---
title: Docker Deployment
nextjs:
  metadata:
    title: Docker Deployment
    description: Build production-ready Docker images for acton-service applications with multi-stage builds and best practices.
---

Deploy acton-service applications using Docker with optimized multi-stage builds for minimal image sizes and maximum security.

## Multi-Stage Dockerfile

The recommended Dockerfile uses a multi-stage build to separate compilation from runtime, reducing image size and attack surface:

```dockerfile
FROM rust:1.84-slim as builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/my-service /usr/local/bin/
EXPOSE 8080
CMD ["my-service"]
```

## Build and Run

Build your Docker image:

```bash
docker build -t my-service:latest .
```

Run the container:

```bash
docker run -p 8080:8080 my-service:latest
```

Access your service:

```bash
# Check health endpoint
curl http://localhost:8080/health

# Access API
curl http://localhost:8080/api/v1/hello
```

## Production Best Practices

### Use Specific Tags

Always use specific version tags instead of `latest` for production deployments:

```dockerfile
FROM rust:1.84-slim as builder
# ...
FROM debian:bookworm-slim
```

### Minimal Runtime Image

Use minimal base images to reduce attack surface and image size. The `debian:bookworm-slim` base provides essential libraries while staying compact.

### Install Only Required Dependencies

```dockerfile
# Install only CA certificates for HTTPS
RUN apt-get update && \
    apt-get install -y ca-certificates && \
    rm -rf /var/lib/apt/lists/*
```

### Run as Non-Root User

Add a dedicated user for running the service:

```dockerfile
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
RUN useradd -m -u 1001 appuser
COPY --from=builder /app/target/release/my-service /usr/local/bin/
USER appuser
EXPOSE 8080
CMD ["my-service"]
```

### Set Environment Variables

Configure your service using environment variables:

```bash
docker run -p 8080:8080 \
  -e ACTON_SERVICE_PORT=8080 \
  -e ACTON_DATABASE_URL=postgres://user:pass@db:5432/mydb \
  -e RUST_LOG=info \
  my-service:latest
```

## Configuration Files

Mount configuration files into the container:

```bash
docker run -p 8080:8080 \
  -v $(pwd)/config:/etc/acton-service \
  -e ACTON_CONFIG_DIR=/etc/acton-service \
  my-service:latest
```

## Health Checks

Add Docker health checks to verify service availability:

```dockerfile
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates curl && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/my-service /usr/local/bin/
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
  CMD curl -f http://localhost:8080/health || exit 1
CMD ["my-service"]
```

## Multi-Architecture Builds

Build for multiple architectures using Docker buildx:

```bash
docker buildx build \
  --platform linux/amd64,linux/arm64 \
  -t my-service:latest \
  --push .
```

## Optimize Build Cache

Use layer caching to speed up builds:

```dockerfile
FROM rust:1.84-slim as builder
WORKDIR /app

# Copy only Cargo files first
COPY Cargo.toml Cargo.lock ./

# Create a dummy main.rs to cache dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release
RUN rm -rf src

# Now copy actual source and build
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
# ... rest of Dockerfile
```

## Docker Compose

For local development with dependencies:

```yaml
version: '3.8'

services:
  my-service:
    build: .
    ports:
      - "8080:8080"
    environment:
      - ACTON_DATABASE_URL=postgres://user:pass@postgres:5432/mydb
      - ACTON_REDIS_URL=redis://redis:6379
      - ACTON_NATS_URL=nats://nats:4222
    depends_on:
      - postgres
      - redis
      - nats

  postgres:
    image: postgres:16-alpine
    environment:
      - POSTGRES_PASSWORD=pass
      - POSTGRES_USER=user
      - POSTGRES_DB=mydb
    volumes:
      - postgres_data:/var/lib/postgresql/data

  redis:
    image: redis:7-alpine

  nats:
    image: nats:2-alpine

volumes:
  postgres_data:
```

Run the complete stack:

```bash
docker-compose up
```

## Security Scanning

Scan images for vulnerabilities before deployment:

```bash
# Using Docker Scout
docker scout cves my-service:latest

# Using Trivy
trivy image my-service:latest
```

## Next Steps

- [Deploy to Kubernetes](/docs/kubernetes) for orchestration and scaling
- [Production Checklist](/docs/production) for deployment best practices
- [Configuration](/docs/configuration) for advanced settings
