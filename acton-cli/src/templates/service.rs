use super::ServiceTemplate;

pub fn generate_main_rs(template: &ServiceTemplate) -> String {
    let mut content = String::from(
r#"use acton_service::prelude::*;
use anyhow::Result;

"#
    );

    // Add handler imports
    if template.http {
        content.push_str("mod handlers;\n");
    }

    if template.grpc {
        content.push_str("mod services;\n");
    }

    content.push_str("\n#[tokio::main]\nasync fn main() -> Result<()> {\n");

    // HTTP-only service
    if template.http && !template.grpc {
        content.push_str(
r#"    // Build versioned routes
    let routes = VersionedApiBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, |router| {
            // TODO: Add your routes here
            // Example: router.route("/users", get(handlers::list_users))
            router
        })
        .build_routes();

    // Build and serve
    ServiceBuilder::new()
        .with_routes(routes)
        .build()
        .serve()
        .await?;

    Ok(())
"#
        );
    }
    // gRPC-only service
    else if template.grpc && !template.http {
        content.push_str(
r#"    // TODO: Configure your gRPC services

    // Build and serve gRPC
    let addr = "0.0.0.0:9090".parse().unwrap();

    println!("gRPC server listening on {}", addr);

    // TODO: Add gRPC service implementation
    // let service = YourServiceImpl::default();
    // Server::builder()
    //     .add_service(YourServiceServer::new(service))
    //     .serve(addr)
    //     .await?;

    Ok(())
"#
        );
    }
    // Dual protocol (HTTP + gRPC)
    else if template.http && template.grpc {
        content.push_str(
r#"    // Build HTTP routes
    let routes = VersionedApiBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, |router| {
            // TODO: Add your HTTP routes here
            router
        })
        .build_routes();

    // Start HTTP server
    let http_handle = tokio::spawn(async move {
        ServiceBuilder::new()
            .with_routes(routes)
            .build()
            .serve()
            .await
    });

    // Start gRPC server
    let grpc_handle = tokio::spawn(async move {
        let addr = "0.0.0.0:9090".parse().unwrap();

        // TODO: Add your gRPC services here
        println!("gRPC server listening on {}", addr);

        Ok::<(), anyhow::Error>(())
    });

    // Wait for both servers
    tokio::select! {
        result = http_handle => {
            result??;
        }
        result = grpc_handle => {
            result??;
        }
    }

    Ok(())
"#
        );
    }

    content.push_str("}\n");

    content
}

pub fn generate_gitignore() -> String {
r#"# Rust
target/
Cargo.lock

# IDE
.idea/
.vscode/
*.swp
*.swo
*~

# OS
.DS_Store
Thumbs.db

# Environment
.env
.env.local

# Logs
*.log

# acton-service
config.local.toml
"#.to_string()
}

pub fn generate_readme(template: &ServiceTemplate) -> String {
    format!(
r#"# {}

Production-ready microservice built with acton-service.

## Features

{}

## Getting Started

### Prerequisites

- Rust 1.75 or later
- Cargo{}

### Configuration

Copy `config.toml` to `config.local.toml` and update with your settings:

```bash
cp config.toml config.local.toml
```

Configuration is loaded from (highest to lowest priority):
1. Environment variables (`ACTON_*`)
2. `./config.local.toml` (development, not in git)
3. `./config.toml`
4. `~/.config/acton-service/{}/config.toml`
5. `/etc/acton-service/{}/config.toml`
6. Default values

### Running

```bash
# Development
cargo run

# Production (optimized)
cargo run --release
```

### Testing

```bash
# Run all tests
cargo test

# Run with coverage
cargo tarpaulin
```

## API Documentation

{}

## License

MIT
"#,
        template.name,
        generate_features_list(template),
        if template.database.is_some() { "\n- PostgreSQL (for database)" } else { "" },
        template.name,
        template.name,
        if template.http {
            "- REST API available at `http://localhost:8080/api/v1`\n- Health check: `GET /health`\n- Readiness check: `GET /ready`"
        } else {
            "- gRPC service available at `localhost:9090`"
        }
    )
}

fn generate_features_list(template: &ServiceTemplate) -> String {
    let mut features = vec![];

    if template.http {
        features.push("- ✅ HTTP REST API with versioning");
    }

    if template.grpc {
        features.push("- ✅ gRPC service");
    }

    if template.database.is_some() {
        features.push("- ✅ PostgreSQL database with connection pooling");
    }

    if template.cache.is_some() {
        features.push("- ✅ Redis caching");
    }

    if template.events.is_some() {
        features.push("- ✅ NATS event streaming");
    }

    if template.auth.is_some() {
        features.push("- ✅ JWT authentication");
    }

    if template.observability {
        features.push("- ✅ OpenTelemetry tracing and metrics");
    }

    if template.resilience {
        features.push("- ✅ Circuit breaker and retry patterns");
    }

    if template.rate_limit {
        features.push("- ✅ Rate limiting");
    }

    features.push("- ✅ Health and readiness endpoints");
    features.push("- ✅ Graceful shutdown");
    features.push("- ✅ Structured logging");

    features.join("\n")
}

pub fn generate_build_rs(template: &ServiceTemplate) -> Option<String> {
    if !template.grpc {
        return None;
    }

    Some(
r#"fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "grpc")]
    {
        // Compile proto files
        // You can customize proto compilation here
        // Example:
        // tonic_build::configure()
        //     .build_server(true)
        //     .build_client(true)
        //     .compile(&["proto/service.proto"], &["proto"])?;
    }
    Ok(())
}
"#.to_string()
    )
}
