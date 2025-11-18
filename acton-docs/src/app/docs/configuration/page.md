---
title: Configuration
nextjs:
  metadata:
    title: Configuration Guide
    description: XDG-compliant configuration with environment variable overrides and sensible defaults
---

{% callout type="note" title="New to acton-service?" %}
Start with the [homepage](/) to understand what acton-service is, then explore [Core Concepts](/docs/concepts) for foundational explanations. See the [Glossary](/docs/glossary) for technical term definitions.
{% /callout %}

---


The acton-service framework uses the XDG Base Directory specification for configuration file management, providing a standard and user-friendly way to organize configuration files for multiple services.

{% callout type="note" title="What is XDG?" %}
The **XDG Base Directory Specification** is a standard from freedesktop.org that defines where applications should store user-specific configuration, data, and cache files on Linux/Unix systems. Instead of cluttering your home directory with dotfiles, XDG organizes everything under `~/.config/`, `~/.local/share/`, and `~/.cache/`.

acton-service follows this standard by placing config files in `~/.config/acton-service/{service_name}/config.toml` - making them easy to find, back up, and manage.
{% /callout %}

---

## Configuration File Locations

Configuration files are searched in the following order (highest priority first):

1. **Current working directory**: `./config.toml`
   - Useful for development and testing
   - Takes precedence over all other locations

2. **XDG config directory**: `~/.config/acton-service/{service_name}/config.toml`
   - Standard user configuration location
   - Example: `~/.config/acton-service/users-api/config.toml`
   - Recommended for production deployments

3. **System directory**: `/etc/acton-service/{service_name}/config.toml`
   - System-wide configuration
   - Requires root access to modify
   - Useful for default configurations

4. **Environment variables**: `ACTON_*`
   - Highest priority (overrides all file-based configs)
   - Format: `ACTON_SERVICE_NAME="my-service"`
   - Useful for containerized deployments

---

## Directory Structure

```bash
~/.config/acton-service/
├── users-api/
│   └── config.toml
├── auth-api/
│   └── config.toml
└── notifications-api/
    └── config.toml
```

Each service has its own subdirectory under `~/.config/acton-service/`, allowing multiple services to run with independent configurations.

---

## Setting Up Configuration

### For Development

During development, simply place a `config.toml` in your project directory:

```bash
cd my-service
cat > config.toml <<EOF
[service]
name = "my-service"
port = 8080
log_level = "debug"
EOF
```

### For Production

For production deployments, place configuration in the XDG directory:

```bash
# Create config directory
mkdir -p ~/.config/acton-service/my-service

# Create config file
cat > ~/.config/acton-service/my-service/config.toml <<EOF
[service]
name = "my-service"
port = 8080
log_level = "info"

[database]
url = "postgres://user:pass@localhost:5432/mydb"
optional = true
lazy_init = true
max_retries = 5
retry_delay_secs = 2

[redis]
url = "redis://localhost:6379"
optional = true
lazy_init = true

[nats]
url = "nats://localhost:4222"
optional = true
lazy_init = true
EOF
```

### For System-Wide Defaults

For system-wide defaults (requires root):

```bash
# Create system config directory
sudo mkdir -p /etc/acton-service/my-service

# Create config file
sudo cat > /etc/acton-service/my-service/config.toml <<EOF
[service]
name = "my-service"
port = 8080
log_level = "info"
EOF
```

---

## Using Environment Variables

Override specific configuration values using environment variables:

```bash
# Override service port
export ACTON_SERVICE_PORT=9090

# Override log level
export ACTON_SERVICE_LOG_LEVEL=debug

# Override database URL
export ACTON_DATABASE_URL=postgres://user:pass@localhost:5432/mydb

# Run service (will use environment variables)
./my-service
```

---

## Configuration API

### Loading Configuration

```rust
use acton_service::prelude::*;

// Automatically detect service name from binary
let config = Config::load()?;

// Explicitly specify service name
let config = Config::load_for_service("my-service")?;

// Load from a specific file (bypasses XDG)
let config = Config::load_from("path/to/config.toml")?;
```

### Getting the Recommended Config Path

```rust
use acton_service::Config;

// Get the recommended path for a service
let path = Config::recommended_path("my-service");
// Returns: ~/.config/acton-service/my-service/config.toml
```

### Creating Config Directory

```rust
use acton_service::Config;

// Create config directory if it doesn't exist
let dir = Config::create_config_dir("my-service")?;
// Creates: ~/.config/acton-service/my-service/
```

---

## Configuration Precedence

When the same configuration value is defined in multiple locations, the following precedence applies:

1. **Environment variables** (highest priority)
2. **Current directory** `./config.toml`
3. **XDG user directory** `~/.config/acton-service/{service}/config.toml`
4. **System directory** `/etc/acton-service/{service}/config.toml`
5. **Default values** (lowest priority)

**Configuration Override Example:**

```bash
# System config sets port to 8080
# cat /etc/acton-service/my-service/config.toml
[service]
port = 8080

# User config overrides to 9090
# cat ~/.config/acton-service/my-service/config.toml
[service]
port = 9090

# Environment variable overrides to 7070 (highest priority)
export ACTON_SERVICE_PORT=7070

# Service will listen on port 7070
```

---

## Custom Configuration Extensions

{% callout type="note" title="New Feature" %}
As of version 0.7.0, you can extend the framework's configuration with your own custom fields that are automatically loaded from the same `config.toml` file.
{% /callout %}

The framework's `Config` type is generic, allowing you to add application-specific configuration fields alongside the built-in framework configuration. Custom fields are seamlessly integrated using Serde's `#[serde(flatten)]` attribute.

### Why Use Custom Config Extensions?

**Benefits:**
- **Single source of truth**: All config in one `config.toml` file
- **XDG directory support**: Custom fields get same XDG path resolution as framework config
- **Environment variable overrides**: Use `ACTON_` prefix for custom fields too
- **Type safety**: Your custom config is strongly typed
- **Zero boilerplate**: No manual file loading or parsing needed

### Basic Usage

**1. Define your custom configuration:**

```rust
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct MyCustomConfig {
    /// API key for external service
    api_key: String,

    /// Feature flags
    feature_flags: HashMap<String, bool>,

    /// Custom timeout in milliseconds
    timeout_ms: u32,
}
```

**2. Specify the custom type when building your service:**

```rust
use acton_service::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    // Config<MyCustomConfig> automatically loads from config.toml
    ServiceBuilder::<MyCustomConfig>::new()
        .with_routes(routes)
        .build()
        .serve()
        .await
}
```

**3. Create a unified config.toml:**

```toml
# Framework configuration (standard fields)
[service]
name = "my-service"
port = 8080
log_level = "info"

[database]
url = "postgres://localhost/mydb"

# Custom configuration (your fields)
api_key = "sk_live_abc123xyz"
timeout_ms = 5000

[feature_flags]
new_dashboard = true
analytics = false
beta_features = true
```

### Accessing Custom Configuration

Custom config is accessed through the `config.custom` field in handlers:

```rust
use axum::extract::State;
use acton_service::AppState;

async fn handler(State(state): State<AppState<MyCustomConfig>>) -> String {
    let config = state.config();

    // Access framework config
    let service_name = &config.service.name;
    let port = config.service.port;

    // Access custom config
    let api_key = &config.custom.api_key;
    let timeout = config.custom.timeout_ms;
    let new_ui_enabled = config.custom.feature_flags
        .get("new_dashboard")
        .copied()
        .unwrap_or(false);

    format!("Service: {service_name}, Timeout: {timeout}ms")
}
```

### Environment Variable Overrides

Custom config fields support environment variable overrides using the `ACTON_` prefix:

```bash
# Override custom fields
export ACTON_API_KEY="sk_test_xyz789"
export ACTON_TIMEOUT_MS=3000
export ACTON_FEATURE_FLAGS_NEW_DASHBOARD=false

# Service automatically loads overrides
./my-service
```

### Default Configuration

If you don't need custom configuration, simply omit the type parameter (defaults to `()`):

```rust
// No custom config - uses default behavior
ServiceBuilder::new()  // Same as ServiceBuilder::<()>::new()
    .with_routes(routes)
    .build()
    .serve()
    .await
```

### Complex Custom Configuration

You can nest structures and use all Serde features:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct MyCustomConfig {
    external_services: ExternalServices,
    feature_flags: HashMap<String, bool>,

    #[serde(default)]
    retry_config: RetryConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ExternalServices {
    payment_api: ServiceEndpoint,
    analytics_api: ServiceEndpoint,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ServiceEndpoint {
    url: String,
    api_key: String,
    timeout_ms: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RetryConfig {
    max_attempts: u32,
    backoff_ms: u32,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            backoff_ms: 1000,
        }
    }
}
```

**Corresponding config.toml:**

```toml
[service]
name = "my-service"
port = 8080

[external_services.payment_api]
url = "https://api.stripe.com"
api_key = "sk_live_..."
timeout_ms = 5000

[external_services.analytics_api]
url = "https://api.analytics.com"
api_key = "key_..."
timeout_ms = 3000

[feature_flags]
payments_v2 = true
new_analytics = false

[retry_config]
max_attempts = 5
backoff_ms = 2000
```

### Loading Custom Config Manually

For advanced use cases, you can load custom config explicitly:

```rust
use acton_service::prelude::*;

// Load from default XDG locations
let config = Config::<MyCustomConfig>::load()?;

// Load from specific file
let config = Config::<MyCustomConfig>::load_from("custom-path.toml")?;

// Load for specific service name
let config = Config::<MyCustomConfig>::load_for_service("my-service")?;

// Create AppState with custom config
let state = AppState::new(config);

ServiceBuilder::new()
    .with_state(state)
    .with_routes(routes)
    .build()
    .serve()
    .await
```

### Requirements for Custom Config Types

Your custom config type must implement:

```rust
trait CustomConfigRequirements:
    Serialize +
    DeserializeOwned +
    Clone +
    Default +
    Send +
    Sync +
    'static
{}
```

**Why these requirements?**
- `Serialize + DeserializeOwned`: Load from and save to config files
- `Clone`: Config is shared across handlers
- `Default`: Provides fallback values
- `Send + Sync + 'static`: Required for async web handlers in Axum

### Example: Feature Flags Service

Complete example with custom config:

```rust
use acton_service::prelude::*;
use axum::{extract::State, routing::get, Json};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct MyCustomConfig {
    feature_flags: HashMap<String, bool>,
    rollout_percentage: HashMap<String, u8>,
}

#[derive(Serialize)]
struct FeatureStatus {
    feature: String,
    enabled: bool,
    rollout_percentage: u8,
}

async fn check_feature(
    State(state): State<AppState<MyCustomConfig>>,
    Path(feature): Path<String>,
) -> Json<FeatureStatus> {
    let config = state.config();
    let enabled = config.custom.feature_flags
        .get(&feature)
        .copied()
        .unwrap_or(false);
    let rollout = config.custom.rollout_percentage
        .get(&feature)
        .copied()
        .unwrap_or(0);

    Json(FeatureStatus {
        feature,
        enabled,
        rollout_percentage: rollout,
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    let routes = VersionedApiBuilder::new()
        .add_version(ApiVersion::V1, |router| {
            router.route("/features/:feature", get(check_feature))
        })
        .build_routes();

    ServiceBuilder::<MyCustomConfig>::new()
        .with_routes(routes)
        .build()
        .serve()
        .await
}
```

**config.toml:**
```toml
[service]
name = "feature-flags"
port = 8080

[feature_flags]
new_ui = true
dark_mode = true
analytics = false

[rollout_percentage]
new_ui = 100
dark_mode = 50
analytics = 10
```

### Best Practices

{% callout type="warning" title="Custom Config Best Practices" %}
1. **Always derive Default**: Provides sensible fallback values
2. **Use `#[serde(default)]` on optional fields**: Prevents errors if fields are missing
3. **Document your custom fields**: Add doc comments explaining each field's purpose
4. **Validate on load**: Add validation logic in a `validate()` method
5. **Keep it flat when possible**: Deeply nested config can be hard to override with env vars
6. **Use type aliases for clarity**: `type MyAppState = AppState<MyCustomConfig>`
{% /callout %}

---

## High Availability Configuration

All external dependencies (Database, Redis, NATS) support high-availability options:

```toml
[database]
url = "postgres://localhost:5432/mydb"
max_retries = 5           # Retry up to 5 times
retry_delay_secs = 2      # 2 seconds base delay (exponential backoff)
optional = true           # Service can start without database
lazy_init = true          # Connect in background (default)

[redis]
url = "redis://localhost:6379"
max_retries = 5
retry_delay_secs = 2
optional = true
lazy_init = true

[nats]
url = "nats://localhost:4222"
max_retries = 5
retry_delay_secs = 2
optional = true
lazy_init = true
```

### Understanding lazy_init

{% callout type="note" title="Default Behavior" %}
By default, `lazy_init = true` makes services start immediately while connecting to dependencies in the background. This prevents slow dependencies from blocking your service startup.
{% /callout %}

**The Problem lazy_init Solves:**

Without lazy initialization, a slow database connection can block service startup for 30+ seconds:

```bash
# Without lazy_init (blocking startup)
2024-01-01 10:00:00 INFO Starting service
2024-01-01 10:00:00 INFO Connecting to database...
[30 second pause while waiting for database]
2024-01-01 10:00:30 INFO Database connected
2024-01-01 10:00:30 INFO Service ready on port 8080
```

With lazy_init enabled (default), service starts immediately:

```bash
# With lazy_init=true (non-blocking)
2024-01-01 10:00:00 INFO Starting service
2024-01-01 10:00:00 INFO Database connection starting in background
2024-01-01 10:00:00 INFO Service ready on port 8080  ← Started immediately!
2024-01-01 10:00:05 INFO Database connected successfully
```

### Configuration Options

**`lazy_init`** - Connection initialization strategy (default: `true`)
- **`true`** (recommended): Service starts immediately, connections happen in background
- **`false`**: Service waits for all connections before starting (blocks startup)

**When startup begins:**
```rust
lazy_init = true  → Service binds port, accepts requests immediately
                    Connections attempt in background with retries

lazy_init = false → Service waits for connection before binding port
                    Retries happen during startup (blocks)
```

**`optional`** - Dependency requirement level (default: `false`)
- **`true`**: Service can operate without this dependency (degrades gracefully)
- **`false`**: Dependency is required for service operation

**When connection fails:**
```rust
optional = true  → Service continues running (degraded state)
                   `/health` → 200 (alive)
                   `/ready` → 503 (not ready)

optional = false → Depends on lazy_init:
                   lazy_init=true  → Service runs but reports degraded
                   lazy_init=false → Service fails to start
```

{% callout type="warning" title="What is Degraded State?" %}
**Degraded state** means the service is alive and running, but one or more dependencies are unavailable. The service can handle some requests but not all:

- **`/health`** returns `200 OK` (service process is alive)
- **`/ready`** returns `503 Service Unavailable` (dependencies are down)
- **Kubernetes behavior**: Pod stays running but is removed from load balancer
- **Requests requiring the dependency**: Return `503` with error message
- **Requests not requiring the dependency**: Work normally

**Example:** Database is down, but service is degraded (not dead):
- `GET /health` → `200 OK` (service alive)
- `GET /ready` → `503` (database unavailable)
- `GET /api/v1/users` → `503` (needs database)
- `GET /api/v1/version` → `200 OK` (doesn't need database)

Once the dependency recovers, the service automatically transitions from degraded to fully healthy, and Kubernetes adds it back to the load balancer.
{% /callout %}

**`max_retries`** - Maximum connection attempts (default: `5`)
- Number of times to retry connection before giving up
- Applies during both startup and background initialization

**`retry_delay_secs`** - Base delay between retries (default: `2` seconds)
- Uses exponential backoff: `delay = base_delay × 2^(attempt-1)`
- Each retry waits twice as long as the previous one

**Exponential Backoff Timing Examples:**

| Attempt | Formula | With base=1s | With base=2s | With base=5s |
|---------|---------|-------------|-------------|-------------|
| 1 | base × 2^0 | 1 second | 2 seconds | 5 seconds |
| 2 | base × 2^1 | 2 seconds | 4 seconds | 10 seconds |
| 3 | base × 2^2 | 4 seconds | 8 seconds | 20 seconds |
| 4 | base × 2^3 | 8 seconds | 16 seconds | 40 seconds |
| 5 | base × 2^4 | 16 seconds | 32 seconds | 80 seconds |
| **Total** | Sum of all | **31 seconds** | **62 seconds** | **155 seconds** |

**Example:** With `max_retries = 5` and `retry_delay_secs = 2`:
```
00:00 - Initial attempt fails
00:02 - Retry #1 (waited 2s) fails
00:06 - Retry #2 (waited 4s) fails
00:14 - Retry #3 (waited 8s) fails
00:30 - Retry #4 (waited 16s) fails
00:62 - Retry #5 (waited 32s) succeeds
Total: 62 seconds from start to success
```

This exponential backoff prevents overwhelming a recovering service with constant retry attempts.

### Operation Modes (Detailed)

| `lazy_init` | `optional` | Startup Behavior | Connection Failure | `/health` | `/ready` | Use Case |
|-------------|-----------|------------------|-------------------|-----------|----------|----------|
| `true` | `true` | ✅ Starts immediately | Continues running | 200 OK | 503 Degraded | **Production HA** |
| `true` | `false` | ✅ Starts immediately | Reports degraded | 200 OK | 503 Degraded | Production with dependencies |
| `false` | `true` | ⏸️ Waits, then continues | Continues running | 200 OK | 200 OK | Eager connection, graceful fallback |
| `false` | `false` | ⏸️ Waits or fails | Startup fails | - | - | **Strict mode** (dev/testing) |

### Example Scenarios

**Scenario 1: Production HA (Recommended)**

```toml
[database]
url = "postgres://db-cluster/mydb"
lazy_init = true    # Start immediately
optional = true     # Continue if DB unavailable
max_retries = 10    # Keep trying
retry_delay_secs = 3
```

**Timeline:**
```
00:00 - Service starts immediately, binds port 8080
00:00 - /health → 200 OK (service alive)
00:00 - /ready → 503 (database not connected yet)
00:00 - Background: Attempting database connection (1/10)
00:03 - Background: Retry (2/10) - Database still unavailable
00:09 - Background: Retry (3/10) - Database still unavailable
00:21 - Background: Connection succeeded!
00:21 - /ready → 200 OK (fully ready)
```

**During connection attempts, requests using database:**
```rust
GET /api/v1/users → 503 Service Unavailable
{
  "error": "Database unavailable",
  "status": 503,
  "retry_after": 5
}
```

**After connection succeeds:**
```rust
GET /api/v1/users → 200 OK
[...]
```

**Scenario 2: Strict Startup (Development)**

```toml
[database]
url = "postgres://localhost:5432/dev"
lazy_init = false   # Wait for connection
optional = false    # Must connect or fail
max_retries = 3
retry_delay_secs = 1
```

**Timeline if database is down:**
```
00:00 - Service starting...
00:00 - Attempting database connection (1/3)
00:01 - Retry (2/3) - Failed
00:03 - Retry (3/3) - Failed
00:07 - ERROR: Failed to connect to required dependency: database
00:07 - Service exits with error code 1
```

Service never starts if database is unavailable.

**Scenario 3: Mixed Dependencies**

```toml
[database]
lazy_init = true
optional = false    # Database required

[cache]
lazy_init = true
optional = true     # Cache optional (can continue without it)

[events]
lazy_init = false
optional = false    # Events required, must connect at startup
```

**Timeline:**
```
00:00 - Service starting...
00:00 - Events: Waiting for connection... (blocks startup)
00:02 - Events: Connected
00:02 - Service starts, binds port
00:02 - Database: Connecting in background
00:02 - Cache: Connecting in background
00:02 - /health → 200 OK
00:02 - /ready → 503 (database and cache not ready)
00:05 - Database: Connected
00:05 - /ready → 503 (cache still connecting)
00:08 - Cache: Connection failed (optional=true, continues)
00:08 - /ready → 200 OK (database ready, cache optional)
```

### What You See in Logs

**lazy_init=true (background connection):**
```
INFO  Starting service
DEBUG Initializing database pool (lazy)
INFO  HTTP server listening on 0.0.0.0:8080
INFO  Background task: Connecting to database
DEBUG Database connection attempt 1/5
INFO  Database pool established
INFO  Service fully ready
```

**lazy_init=false (blocking startup):**
```
INFO  Starting service
DEBUG Initializing database pool (eager)
INFO  Connecting to database...
DEBUG Database connection attempt 1/5
INFO  Database pool established
INFO  HTTP server listening on 0.0.0.0:8080
INFO  Service ready
```

### Best Practices

**Production Services (Recommended):**
```toml
lazy_init = true    # Fast startup
optional = true     # Graceful degradation
max_retries = 10    # Keep trying
```

**Development/Testing:**
```toml
lazy_init = false   # Catch connection issues early
optional = false    # Fail fast if dependencies missing
max_retries = 3     # Quick feedback
```

**When to use lazy_init=false:**
- Local development (want immediate feedback if database is down)
- Integration tests (want tests to fail if dependencies unavailable)
- Services that can't operate at all without dependencies (no degraded mode)

**When to use lazy_init=true:**
- Production deployments (fast startup, health checks pass quickly)
- Kubernetes deployments (liveness probes succeed during rolling updates)
- Services with multiple dependencies (don't want one slow dep blocking everything)

### Common Mistakes

**❌ Mistake 1: lazy_init=false with optional=true in Kubernetes**
```toml
lazy_init = false
optional = true
```
Problem: Startup can be slow (30s+) waiting for connection attempts, causing liveness probe failures.

**✅ Fix:**
```toml
lazy_init = true   # Start immediately
optional = true
```

**❌ Mistake 2: lazy_init=true without handling unavailable dependencies**
```rust
// Assumes database is always available
async fn get_user(State(state): State<AppState>) -> User {
    let db = state.db().await.unwrap();  // ← Panics if not connected!
    // ...
}
```

**✅ Fix:**
```rust
async fn get_user(State(state): State<AppState>) -> Result<User, AppError> {
    let db = state.db().await
        .ok_or(AppError::ServiceUnavailable("Database unavailable"))?;
    // ...
}
```

---

## Best Practices

{% callout type="warning" title="Configuration Best Practices" %}
1. **Use XDG directories in production**: Place configs in `~/.config/acton-service/{service}/` for standard compliance

2. **Keep development configs in working directory**: Use `./config.toml` during development for quick iterations

3. **Use environment variables for secrets**: Never commit sensitive data to config files. Example:

       bash
       export ACTON_DATABASE_URL="postgres://user:${DB_PASSWORD}@localhost/db"

4. **Enable high availability options**: Set `optional=true` and `lazy_init=true` for all external dependencies

5. **Configure appropriate retries**: Adjust `max_retries` and `retry_delay_secs` based on your infrastructure

6. **Use system configs for defaults only**: System-wide configs should contain only non-sensitive defaults
{% /callout %}

---

## Troubleshooting

### Config file not found

If you see errors about missing config files:

```bash
# Check which paths are being searched
RUST_LOG=acton_service::config=debug ./my-service

# Verify XDG directory exists
ls -la ~/.config/acton-service/my-service/
```

### Wrong config being loaded

Check the search order and ensure higher-priority configs are correct:

```bash
# List all possible config locations
ls -la ./config.toml
ls -la ~/.config/acton-service/my-service/config.toml
ls -la /etc/acton-service/my-service/config.toml

# Check environment variables
env | grep ACTON_
```

### Permission denied

If you get permission errors:

```bash
# Fix permissions on XDG directory
chmod 755 ~/.config/acton-service
chmod 755 ~/.config/acton-service/my-service
chmod 644 ~/.config/acton-service/my-service/config.toml
```

---

## See Also

- [XDG Base Directory Specification](https://specifications.freedesktop.org/basedir-spec/basedir-spec-latest.html)
- [Getting Started](/docs/getting-started) - Service setup guide
- [Configuration Reference](/docs) - Complete configuration options
