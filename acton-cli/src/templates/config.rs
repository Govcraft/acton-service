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
min_connections = 5
max_connections = 20

"#,
            );
        }
    }

    // Add cache configuration
    if template.cache.is_some() {
        content.push_str(
            r#"[redis]
url = "redis://localhost:6379"
# For production, use environment variable: ACTON_REDIS_URL
optional = true
lazy_init = true
max_connections = 10

"#,
        );
    }

    // Add events configuration
    if template.events.is_some() {
        content.push_str(
            r#"[nats]
url = "nats://localhost:4222"
# For production, use environment variable: ACTON_NATS_URL
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
# This table rejects keys it cannot honour, so every key here reaches the
# instrumentation. Metrics are pushed to the OTLP endpoint above and served
# for scraping on the exporter listener below.
[middleware.metrics]
enabled = true

# A dedicated, plaintext listener serving only GET /metrics, separate from the
# application listener. Managed collectors -- Fly.io's [[metrics]] block, GKE
# managed collection, most PodMonitor/ServiceMonitor defaults -- speak plain
# HTTP to a declared port and offer no TLS knobs, so they cannot scrape an
# application listener that terminates TLS. This socket carries no TLS and no
# authentication by design: bind it to a private scrape network, never an
# internet-facing interface. The generated Kubernetes manifests declare a
# container port, a Service port and a ServiceMonitor matching this table.
[middleware.metrics.exporter]
bind = "::"
port = 9090

"#,
        );
    }

    // Add rate limiting configuration
    if template.rate_limit {
        content.push_str(
            r#"# Rate limiting
[rate_limit]
auto_apply = true
per_user_rpm = 100
per_client_rpm = 100
anonymous_rpm = 100
anonymous_burst = 20

"#,
        );
    }

    content
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
    struct UnclaimedSections {
        #[serde(flatten)]
        unknown: BTreeMap<String, serde_json::Value>,
    }

    fn template(name: &str) -> ServiceTemplate {
        ServiceTemplate {
            name: name.into(),
            pascal_name: "GeneratedConfig".into(),
            snake_name: name.into(),
            http: true,
            grpc: false,
            database: None,
            cache: None,
            events: None,
            auth: None,
            observability: false,
            resilience: false,
            rate_limit: false,
            openapi: false,
            audit: false,
            graphql: false,
        }
    }

    fn load_generated(
        template: &ServiceTemplate,
    ) -> acton_service::config::Config<UnclaimedSections> {
        let path = std::env::temp_dir().join(format!(
            "acton-cli-generated-config-{}-{}.toml",
            std::process::id(),
            template.name,
        ));
        std::fs::write(&path, generate(template)).expect("generated config is written");
        let loaded = acton_service::config::Config::<UnclaimedSections>::load_from(
            path.to_str().expect("temporary path is UTF-8"),
        );
        std::fs::remove_file(path).expect("temporary config is removed");
        loaded.expect("generated framework configuration deserializes")
    }

    #[test]
    fn generated_config_uses_actual_framework_sections_and_values() {
        let minimal = template("minimal-config");
        assert!(load_generated(&minimal).custom.unknown.is_empty());

        let mut complete = template("complete-config");
        complete.database = Some("postgres".into());
        complete.cache = Some("redis".into());
        complete.events = Some("nats".into());
        complete.grpc = true;
        complete.observability = true;
        complete.resilience = true;
        complete.rate_limit = true;
        complete.audit = true;
        let config = load_generated(&complete);
        // The audit table needs the selected service's audit feature. The CLI's
        // own acton-service dependency may not compile that optional type.
        assert!(config.custom.unknown.keys().all(|key| key == "audit"));
        let database = config.database.expect("database table is recognized");
        assert_eq!(database.min_connections, 5);
        assert_eq!(database.max_connections, 20);
        let redis = config.redis.expect("Redis table is recognized");
        assert_eq!(redis.max_connections, 10);
        assert_eq!(redis.url, "redis://localhost:6379");
        assert_eq!(
            config.nats.expect("NATS table is recognized").url,
            "nats://localhost:4222"
        );
        assert!(config.grpc.expect("gRPC table is recognized").enabled);
        assert!(config.otlp.expect("OTLP table is recognized").enabled);
        assert!(
            config
                .middleware
                .resilience
                .expect("resilience table is recognized")
                .bulkhead_enabled
        );
        assert!(config
            .middleware
            .metrics
            .expect("metrics table is recognized")
            .exporter
            .is_some());
        assert!(config.rate_limit.auto_apply);
        assert_eq!(config.rate_limit.per_user_rpm, 100);
        assert_eq!(config.rate_limit.per_client_rpm, 100);
        assert_eq!(config.rate_limit.anonymous_quota(), (100, 20));
        assert!(complete
            .features()
            .iter()
            .any(|feature| feature == "governor"));
        assert!(!complete
            .features()
            .iter()
            .any(|feature| feature == "rate-limit"));
    }

    #[test]
    fn surrealdb_scaffold_selects_its_own_feature_and_configuration() {
        let mut surreal = template("surreal-config");
        surreal.database = Some("surrealdb".into());
        let config = load_generated(&surreal);
        // The generated service enables SurrealDB. When the CLI itself does
        // not, this table remains an extension rather than a framework type.
        assert!(config.custom.unknown.keys().all(|key| key == "surrealdb"));
        assert!(
            config.database.is_none(),
            "SurrealDB does not configure a SQL pool"
        );
        assert!(surreal
            .features()
            .iter()
            .any(|feature| feature == "surrealdb"));
        assert!(!surreal
            .features()
            .iter()
            .any(|feature| feature == "database"));
        if let Some(table) = config.custom.unknown.get("surrealdb") {
            assert_eq!(table["url"], "ws://localhost:8000");
            assert_eq!(table["namespace"], "default");
            assert_eq!(table["database"], "default");
        }
    }
}
