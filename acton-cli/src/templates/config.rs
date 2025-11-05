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
    if template.database.is_some() {
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

"#
        );
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

"#
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

"#
        );
    }

    // Add observability configuration
    if template.observability {
        content.push_str(
r#"[observability]
tracing_endpoint = "http://localhost:14268/api/traces"
metrics_enabled = true

"#
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

"#
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

retry_enabled = true
retry_max_attempts = 3
retry_base_delay_ms = 100

bulkhead_enabled = true
bulkhead_max_concurrent = 100

"#
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

"#
        );
    }

    content
}
