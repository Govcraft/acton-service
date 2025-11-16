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

### Configuration Options

- **`max_retries`**: Maximum number of connection attempts (default: 5)
- **`retry_delay_secs`**: Base delay between retries in seconds (default: 2)
  - Uses exponential backoff: delay = base_delay × 2^(attempt-1)
- **`optional`**: Whether the dependency is optional (default: false)
  - `true`: Service starts even if connection fails
  - `false`: Service fails to start if connection fails (in eager mode)
- **`lazy_init`**: Whether to initialize connection in background (default: true)
  - `true`: Service starts immediately, connects in background with retries
  - `false`: Service waits for connection before starting (blocks startup)

### Operation Modes

| `lazy_init` | `optional` | Behavior |
|-------------|-----------|----------|
| `true` | `true` | **Recommended for HA**: Starts immediately, connects in background, continues if connection fails |
| `true` | `false` | Starts immediately, connects in background, reports degraded if connection fails |
| `false` | `true` | Blocks startup, retries connection, continues if all retries fail |
| `false` | `false` | **Strict mode**: Blocks startup, fails if connection cannot be established |

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
