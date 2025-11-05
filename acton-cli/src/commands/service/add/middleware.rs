use anyhow::{bail, Result};
use colored::Colorize;

pub async fn execute(middleware_type: String, dry_run: bool) -> Result<()> {
    let middleware_lower = middleware_type.to_lowercase();

    if dry_run {
        show_dry_run(&middleware_lower);
        return Ok(());
    }

    match middleware_lower.as_str() {
        "jwt" | "auth" | "authentication" => show_jwt_middleware(),
        "resilience" | "circuit-breaker" | "retry" => show_resilience_middleware(),
        "metrics" | "otel" | "opentelemetry" => show_metrics_middleware(),
        "governor" | "rate-limit" | "ratelimit" => show_governor_middleware(),
        "cors" => show_cors_middleware(),
        "compression" => show_compression_middleware(),
        "panic" | "catch-panic" => show_panic_middleware(),
        "request-tracking" | "request-id" => show_request_tracking_middleware(),
        "timeout" => show_timeout_middleware(),
        "all" | "list" => show_all_middleware(),
        _ => {
            eprintln!("{} Unknown middleware type: {}", "Error:".red().bold(), middleware_type);
            eprintln!();
            eprintln!("Available middleware types:");
            eprintln!("  - jwt, auth, authentication");
            eprintln!("  - resilience, circuit-breaker, retry");
            eprintln!("  - metrics, otel, opentelemetry");
            eprintln!("  - governor, rate-limit");
            eprintln!("  - cors");
            eprintln!("  - compression");
            eprintln!("  - panic, catch-panic");
            eprintln!("  - request-tracking, request-id");
            eprintln!("  - timeout");
            eprintln!("  - all, list (show all available middleware)");
            bail!("Unknown middleware type");
        }
    }

    Ok(())
}

fn show_dry_run(middleware_type: &str) {
    println!("\n{}", "Dry run - would show:".bold());
    println!();
    println!("Middleware: {}", middleware_type.cyan());
    println!();
    println!("Instructions for adding {} middleware to your service", middleware_type);
}

fn show_all_middleware() {
    println!("{}", "acton-service Available Middleware".bold().cyan());
    println!();
    println!("{}", "All middleware is configured via config.toml and enabled via Cargo features.".bold());
    println!();

    println!("{}", "1. JWT Authentication".green().bold());
    println!("   {}: jwt, auth, authentication", "Aliases".yellow());
    println!("   Validates JWT tokens for protected endpoints");
    println!();

    println!("{}", "2. Resilience Patterns".green().bold());
    println!("   {}: resilience, circuit-breaker, retry", "Aliases".yellow());
    println!("   Circuit breaker, retry, and bulkhead patterns");
    println!("   {}: resilience", "Feature".cyan());
    println!();

    println!("{}", "3. Metrics (OpenTelemetry)".green().bold());
    println!("   {}: metrics, otel, opentelemetry", "Aliases".yellow());
    println!("   HTTP request metrics and tracing");
    println!("   {}: otel-metrics", "Feature".cyan());
    println!();

    println!("{}", "4. Rate Limiting (Governor)".green().bold());
    println!("   {}: governor, rate-limit", "Aliases".yellow());
    println!("   Token bucket rate limiting");
    println!("   {}: governor", "Feature".cyan());
    println!();

    println!("{}", "5. CORS".green().bold());
    println!("   Cross-Origin Resource Sharing");
    println!();

    println!("{}", "6. Compression".green().bold());
    println!("   gzip/deflate response compression");
    println!();

    println!("{}", "7. Panic Recovery".green().bold());
    println!("   {}: panic, catch-panic", "Aliases".yellow());
    println!("   Gracefully handle panics in handlers");
    println!();

    println!("{}", "8. Request Tracking".green().bold());
    println!("   {}: request-tracking, request-id", "Aliases".yellow());
    println!("   Request ID generation and header propagation");
    println!();

    println!("{}", "9. Timeout".green().bold());
    println!("   Request timeout middleware");
    println!();

    println!("{}", "Usage:".cyan().bold());
    println!("  acton service add middleware <type>");
    println!("  acton service add middleware all    # Show this overview");
}

fn show_jwt_middleware() {
    println!("{}", "Adding JWT Authentication Middleware".bold());
    println!();
    println!("{}", "JWT authentication is built-in and configured via config.toml.".bold());
    println!();

    println!("{}", "1. Add to config.toml:".green().bold());
    println!();
    println!("   [jwt]");
    println!("   public_key_path = \"keys/public.pem\"");
    println!("   algorithm = \"RS256\"  # or ES256, HS256");
    println!("   issuer = \"your-issuer\"  # optional");
    println!("   audience = \"your-audience\"  # optional");
    println!();

    println!("{}", "2. Use in your handlers:".green().bold());
    println!();
    println!("   use acton_service::prelude::*;");
    println!("   use acton_service::middleware::{{Claims, JwtAuth}};");
    println!();
    println!("   // Protect an endpoint");
    println!("   async fn protected_handler(");
    println!("       JwtAuth(claims): JwtAuth,");
    println!("   ) -> impl IntoResponse {{");
    println!(r#"       Json(json!({{ "user_id": claims.sub }}))"#);
    println!("   }}");
    println!();
    println!("   // In your routes");
    println!("   .route(\"/protected\", get(protected_handler))");
    println!();

    println!("{}", "3. Optional - Custom Claims:".green().bold());
    println!();
    println!("   #[derive(Debug, Serialize, Deserialize)]");
    println!("   struct MyClaims {{");
    println!("       sub: String,");
    println!("       role: String,");
    println!("       // ... your fields");
    println!("   }}");
    println!();
    println!("   async fn handler(JwtAuth(claims): JwtAuth<MyClaims>) -> impl IntoResponse {{");
    println!(r#"       // Access claims.role, etc."#);
    println!("   }}");
    println!();

    println!("{}", "Learn more:".yellow().bold());
    println!("  See acton-service documentation for JWT configuration");
}

fn show_resilience_middleware() {
    println!("{}", "Adding Resilience Middleware".bold());
    println!();
    println!("{}", "Resilience patterns are enabled via the 'resilience' feature.".bold());
    println!();

    println!("{}", "1. Enable in Cargo.toml:".green().bold());
    println!();
    println!("   [dependencies]");
    println!(r#"   acton-service = {{ version = "0.2", features = ["resilience"] }}"#);
    println!();

    println!("{}", "2. Configure in config.toml:".green().bold());
    println!();
    println!("   [middleware.resilience]");
    println!("   # Circuit Breaker");
    println!("   circuit_breaker_enabled = true");
    println!("   circuit_breaker_threshold = 0.5  # 50% failure rate");
    println!("   circuit_breaker_min_requests = 10");
    println!("   circuit_breaker_wait_secs = 30");
    println!();
    println!("   # Retry");
    println!("   retry_enabled = true");
    println!("   retry_max_attempts = 3");
    println!("   retry_backoff_ms = 100");
    println!();
    println!("   # Bulkhead");
    println!("   bulkhead_enabled = true");
    println!("   bulkhead_max_concurrent = 100");
    println!();

    println!("{}", "Automatic integration:".cyan().bold());
    println!("  Resilience middleware is automatically added to all routes when configured.");
    println!("  No code changes required!");
    println!();

    println!("{}", "Learn more:".yellow().bold());
    println!("  See acton-service/src/middleware/resilience.rs");
}

fn show_metrics_middleware() {
    println!("{}", "Adding Metrics Middleware (OpenTelemetry)".bold());
    println!();
    println!("{}", "HTTP metrics are enabled via the 'otel-metrics' feature.".bold());
    println!();

    println!("{}", "1. Enable in Cargo.toml:".green().bold());
    println!();
    println!("   [dependencies]");
    println!(r#"   acton-service = {{ version = "0.2", features = ["otel-metrics"] }}"#);
    println!();

    println!("{}", "2. Configure in config.toml:".green().bold());
    println!();
    println!("   [middleware.metrics]");
    println!("   enabled = true");
    println!("   endpoint_path = \"/metrics\"  # Prometheus endpoint");
    println!();
    println!("   # Optional OTLP export");
    println!("   [otlp]");
    println!(r#"   endpoint = "http://localhost:4317""#);
    println!("   service_name = \"my-service\"");
    println!();

    println!("{}", "Automatic integration:".cyan().bold());
    println!("  Metrics middleware is automatically added when configured.");
    println!("  Exposes /metrics endpoint for Prometheus scraping.");
    println!();

    println!("{}", "Available metrics:".cyan().bold());
    println!("  - http_requests_total");
    println!("  - http_request_duration_seconds");
    println!("  - http_requests_in_flight");
    println!();

    println!("{}", "Learn more:".yellow().bold());
    println!("  See acton-service/src/middleware/metrics.rs");
}

fn show_governor_middleware() {
    println!("{}", "Adding Rate Limiting Middleware (Governor)".bold());
    println!();
    println!("{}", "Token bucket rate limiting via the 'governor' feature.".bold());
    println!();

    println!("{}", "1. Enable in Cargo.toml:".green().bold());
    println!();
    println!("   [dependencies]");
    println!(r#"   acton-service = {{ version = "0.2", features = ["governor"] }}"#);
    println!();

    println!("{}", "2. Configure in config.toml:".green().bold());
    println!();
    println!("   [middleware.governor]");
    println!("   requests_per_second = 10");
    println!("   burst_size = 20");
    println!("   # Optional: configure key extraction");
    println!(r#"   key_extractor = "ip"  # or "header:X-API-Key""#);
    println!();

    println!("{}", "Automatic integration:".cyan().bold());
    println!("  Rate limiting is automatically enforced on all routes when configured.");
    println!("  Returns 429 Too Many Requests when limit exceeded.");
    println!();

    println!("{}", "Learn more:".yellow().bold());
    println!("  See acton-service/src/middleware/governor.rs");
}

fn show_cors_middleware() {
    println!("{}", "Adding CORS Middleware".bold());
    println!();
    println!("{}", "CORS is built-in and configured via config.toml.".bold());
    println!();

    println!("{}", "Configure in config.toml:".green().bold());
    println!();
    println!("   [middleware]");
    println!(r#"   cors_mode = "permissive"  # or "restrictive" or "disabled""#);
    println!();

    println!("{}", "CORS modes:".cyan().bold());
    println!("  - {}: Allow all origins (development)", "permissive".green());
    println!("  - {}: Strict origin checking (production)", "restrictive".yellow());
    println!("  - {}: No CORS headers", "disabled".red());
    println!();

    println!("{}", "Automatic integration:".cyan().bold());
    println!("  CORS middleware is automatically added based on configuration.");
}

fn show_compression_middleware() {
    println!("{}", "Adding Compression Middleware".bold());
    println!();
    println!("{}", "Response compression is built-in and enabled by default.".bold());
    println!();

    println!("{}", "Configure in config.toml:".green().bold());
    println!();
    println!("   [middleware]");
    println!("   compression = true  # default: true");
    println!();

    println!("{}", "Automatic integration:".cyan().bold());
    println!("  Compression middleware is automatically added.");
    println!("  Supports gzip and deflate based on Accept-Encoding header.");
}

fn show_panic_middleware() {
    println!("{}", "Adding Panic Recovery Middleware".bold());
    println!();
    println!("{}", "Panic recovery is built-in and enabled by default.".bold());
    println!();

    println!("{}", "Configure in config.toml:".green().bold());
    println!();
    println!("   [middleware]");
    println!("   catch_panic = true  # default: true");
    println!();

    println!("{}", "Automatic integration:".cyan().bold());
    println!("  Panic recovery middleware is automatically added.");
    println!("  Prevents panics in handlers from crashing the service.");
    println!("  Returns 500 Internal Server Error on panic.");
}

fn show_request_tracking_middleware() {
    println!("{}", "Adding Request Tracking Middleware".bold());
    println!();
    println!("{}", "Request ID generation and header propagation are built-in.".bold());
    println!();

    println!("{}", "Configure in config.toml:".green().bold());
    println!();
    println!("   [middleware.request_tracking]");
    println!("   request_id_enabled = true");
    println!(r#"   request_id_header = "X-Request-Id""#);
    println!("   propagate_headers = true");
    println!("   mask_sensitive_headers = true");
    println!();

    println!("{}", "Automatic integration:".cyan().bold());
    println!("  Request tracking middleware is automatically added.");
    println!();

    println!("{}", "Features:".cyan().bold());
    println!("  - Generates unique request ID for each request");
    println!("  - Propagates correlation headers to downstream services");
    println!("  - Masks sensitive headers (Authorization, Cookie) in logs");
    println!();

    println!("{}", "Learn more:".yellow().bold());
    println!("  See acton-service/src/middleware/request_tracking.rs");
}

fn show_timeout_middleware() {
    println!("{}", "Adding Timeout Middleware".bold());
    println!();
    println!("{}", "Request timeout is built-in and configured via config.toml.".bold());
    println!();

    println!("{}", "Configure in config.toml:".green().bold());
    println!();
    println!("   [service]");
    println!("   timeout_secs = 30  # default request timeout");
    println!();

    println!("{}", "Automatic integration:".cyan().bold());
    println!("  Timeout middleware is automatically added to all routes.");
    println!("  Returns 408 Request Timeout if request exceeds configured duration.");
}
