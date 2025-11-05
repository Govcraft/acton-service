// Deployment template generation will be implemented here
pub fn generate_dockerfile(service_name: &str) -> String {
    format!(
r#"# Multi-stage build for {}

# Stage 1: Build
FROM rust:1.75-alpine AS builder

RUN apk add --no-cache musl-dev

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --release

# Stage 2: Runtime
FROM alpine:3.19

RUN apk add --no-cache ca-certificates && \
    addgroup -g 1000 appuser && \
    adduser -D -u 1000 -G appuser appuser

WORKDIR /app
COPY --from=builder /build/target/release/{} /app/

USER appuser
EXPOSE 8080 9090

CMD ["/app/{}"]
"#,
        service_name, service_name, service_name
    )
}

pub fn generate_dockerignore() -> String {
r#"target/
.git/
.gitignore
*.md
.env
.env.local
config.local.toml
"#.to_string()
}
