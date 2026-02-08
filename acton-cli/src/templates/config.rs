use super::ServiceTemplate;

pub fn generate(template: &ServiceTemplate) -> String {
    let mut content = format!(
        r#"[service]
name = "{}"
port = 8080
log_level = "info"

"#,
        template.name
    );

    // Add database configuration
    if let Some(ref db_type) = template.database {
        if db_type == "surrealdb" {
            content.push_str(
                r#"[surrealdb]
url = "ws://localhost:8000"
# For production, use environment variable: ACTON_SURREALDB_URL
namespace = "default"
database = "default"
# username = "root"     # Optional: omit for unauthenticated access
# password = "root"     # Optional: omit for unauthenticated access
optional = true       # Service can start without SurrealDB
lazy_init = true      # Connect in background
max_retries = 5       # Retry up to 5 times
retry_delay_secs = 2  # Base delay for exponential backoff

"#,
            );
        } else {
            content.push_str(
                r#"[database]
url = "postgres://localhost:5432/mydb"
# For production, use environment variable: ACTON_DATABASE_URL
optional = true       # Service can start without database
lazy_init = true      # Connect in background
max_retries = 5       # Retry up to 5 times
retry_delay_secs = 2  # Base delay for exponential backoff
pool_min_size = 5
pool_max_size = 20

"#,
            );
        }
    }

    // Add cache configuration
    if template.cache.is_some() {
        content.push_str(
            r#"[cache]
url = "redis://localhost:6379"
# For production, use environment variable: ACTON_CACHE_URL
optional = true
lazy_init = true
pool_size = 10

"#,
        );
    }

    // Add events configuration
    if template.events.is_some() {
        content.push_str(
            r#"[events]
url = "nats://localhost:4222"
# For production, use environment variable: ACTON_EVENTS_URL
optional = true
lazy_init = true

"#,
        );
    }

    // Add gRPC configuration
    if template.grpc {
        content.push_str(
            r#"[grpc]
enabled = true

# Port Configuration
#
# Single-port mode (default, recommended):
#   - HTTP and gRPC share the same port (8080)
#   - Automatic protocol detection via Content-Type header
#   - Simpler deployment (one port to expose)
#   - Perfect for most use cases
#
# Dual-port mode (advanced):
#   - HTTP runs on port 8080
#   - gRPC runs on separate port (9090)
#   - Useful for network policies requiring protocol separation
#   - Allows independent scaling of HTTP and gRPC traffic
#   - Requires exposing both ports in deployment
#
# To switch to dual-port mode:
#   1. Set use_separate_port = true
#   2. Optionally change the gRPC port below
#   3. Restart the service
use_separate_port = false  # false = single-port, true = dual-port
port = 9090                # gRPC port (only used when use_separate_port = true)

# gRPC Features
reflection_enabled = true
health_check_enabled = true
max_message_size_mb = 4
connection_timeout_secs = 10
timeout_secs = 30

"#,
        );
    }

    // Add observability configuration
    if template.observability {
        content.push_str(
            r#"[otlp]
endpoint = "http://localhost:4317"
# For production, use environment variable: ACTON_OTLP_ENDPOINT
enabled = true
# service_name defaults to service.name if not specified

"#,
        );
    }

    // Add audit configuration
    if template.audit {
        content.push_str(
            r#"[audit]
enabled = true
audit_all_requests = false
audit_auth_events = true
audited_routes = ["/api/v1/admin/*"]
excluded_routes = ["/health", "/ready", "/metrics"]

[audit.syslog]
transport = "udp"
address = "127.0.0.1:514"
facility = 13

"#,
        );
    }

    // Add middleware configuration
    content.push_str(
        r#"[middleware]
# Basic middleware settings
body_limit_mb = 10
catch_panic = true
compression = true
cors_mode = "permissive"  # Options: permissive, restrictive, disabled

# Request tracking
[middleware.request_tracking]
request_id_enabled = true
propagate_headers = true
mask_sensitive_headers = true

"#,
    );

    // Add resilience configuration
    if template.resilience {
        content.push_str(
            r#"# Resilience patterns
[middleware.resilience]
circuit_breaker_enabled = true
circuit_breaker_threshold = 0.5
circuit_breaker_min_requests = 10
circuit_breaker_wait_secs = 30

bulkhead_enabled = true
bulkhead_max_concurrent = 100

"#,
        );
    }

    // Add metrics configuration
    if template.observability {
        content.push_str(
            r#"# HTTP Metrics (OpenTelemetry)
[middleware.metrics]
enabled = true
export_interval_secs = 60
# Metrics exported to OTLP endpoint above

"#,
        );
    }

    // Add rate limiting configuration
    if template.rate_limit {
        content.push_str(
            r#"# Rate limiting
[middleware.rate_limit]
enabled = true
requests_per_minute = 100
burst_size = 20

"#,
        );
    }

    content
}
