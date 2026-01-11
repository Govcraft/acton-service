---
title: Password Hashing
nextjs:
  metadata:
    title: Password Hashing
    description: Secure password storage with Argon2id - OWASP-compliant defaults, simple API, and automatic parameter upgrading.
---

{% callout type="note" title="Part of the Auth Module" %}
This guide covers password hashing. See the [Authentication Overview](/docs/auth) for all auth capabilities, or jump to [Token Generation](/docs/token-generation), [API Keys](/docs/api-keys), or [OAuth/OIDC](/docs/oauth).
{% /callout %}

---

## Introduction

Password hashing in acton-service uses Argon2id, the algorithm recommended by OWASP for password storage. The `PasswordHasher` provides three operations: hash passwords during registration, verify passwords during login, and detect when stored hashes need upgrading to stronger parameters.

The defaults follow OWASP guidelines: 64 MiB memory, 3 iterations, 4 parallel threads. These parameters make brute-force attacks computationally expensive while keeping login latency acceptable. You can adjust parameters based on your hardware and security requirements.

**Key characteristics:**

- **Argon2id algorithm**: Combines Argon2i (side-channel resistance) and Argon2d (GPU resistance)
- **Random salts**: Each hash uses a unique cryptographically random salt
- **PHC string format**: Self-describing hashes that include algorithm, parameters, salt, and hash value
- **Constant-time verification**: Prevents timing attacks during password checking

---

## Quick Start

```toml
[dependencies]
acton-service = { version = "{% version() %}", features = ["auth"] }
```

```rust
use acton_service::auth::PasswordHasher;

// Create hasher with OWASP defaults
let hasher = PasswordHasher::default();

// Registration: hash the password
let hash = hasher.hash("user_password_123")?;
// Returns: $argon2id$v=19$m=65536,t=3,p=4$<salt>$<hash>

// Login: verify the password
let is_valid = hasher.verify("user_password_123", &hash)?;
assert!(is_valid);

// Wrong password returns false (not an error)
let is_valid = hasher.verify("wrong_password", &hash)?;
assert!(!is_valid);
```

---

## Configuration

The `PasswordConfig` struct controls hashing parameters. All values have OWASP-recommended defaults.

```rust
use acton_service::auth::{PasswordHasher, PasswordConfig};

let config = PasswordConfig {
    memory_cost_kib: 65536,      // 64 MiB (default)
    time_cost: 3,                // 3 iterations (default)
    parallelism: 4,              // 4 threads (default)
    min_password_length: 12,     // Override default of 8
};

let hasher = PasswordHasher::new(config);
```

Or configure via TOML:

```toml
[auth.password]
memory_cost_kib = 65536
time_cost = 3
parallelism = 4
min_password_length = 12
```

### Parameter Guidelines

| Parameter | Default | Effect |
|-----------|---------|--------|
| `memory_cost_kib` | 65536 (64 MiB) | Higher = more GPU-resistant, more RAM needed |
| `time_cost` | 3 | Higher = slower hashing, more CPU time |
| `parallelism` | 4 | Should match available CPU cores |
| `min_password_length` | 8 | Enforced before hashing |

**Tuning for your hardware**: Hash time should be 0.5-1 second for interactive logins. Measure on your production hardware and adjust parameters to hit this target.

---

## Upgrading Hash Parameters

When you increase security parameters, existing hashes become outdated. The `needs_rehash()` method detects this, letting you upgrade hashes transparently during login.

```rust
use acton_service::auth::{PasswordHasher, PasswordConfig};

// New hasher with stronger parameters
let hasher = PasswordHasher::new(PasswordConfig {
    memory_cost_kib: 131072, // Upgraded from 65536
    time_cost: 4,            // Upgraded from 3
    ..Default::default()
});

async fn login(password: &str, stored_hash: &str, user_id: &str) -> Result<bool, Error> {
    // Verify with stored parameters (encoded in hash)
    if !hasher.verify(password, stored_hash)? {
        return Ok(false);
    }

    // Check if hash uses old parameters
    if hasher.needs_rehash(stored_hash) {
        // Re-hash with new parameters
        let new_hash = hasher.hash(password)?;
        update_password_hash(user_id, &new_hash).await?;
    }

    Ok(true)
}
```

The PHC string format stores all parameters with the hash, so `verify()` always uses the correct parameters for each hash. Only newly created hashes use the current configuration.

---

## PHC String Format

Hashes are stored as PHC (Password Hashing Competition) strings, which are self-describing:

```text
$argon2id$v=19$m=65536,t=3,p=4$c2FsdHNhbHRzYWx0$aGFzaGhhc2hoYXNo
│        │    │              │                   └─ Hash (base64)
│        │    │              └─ Salt (base64)
│        │    └─ Parameters: m=memory, t=time, p=parallelism
│        └─ Version (19 = 0x13)
└─ Algorithm identifier
```

**Benefits of PHC format:**

- No separate salt storage needed
- Parameters stored with each hash
- Supports mixed parameters in database
- Future-proof for algorithm changes

---

## Error Handling

```rust
use acton_service::auth::PasswordHasher;
use acton_service::error::Error;

let hasher = PasswordHasher::default();

// Password too short
match hasher.hash("short") {
    Err(Error::ValidationError(msg)) => {
        println!("Validation failed: {}", msg);
        // "Password must be at least 8 characters"
    }
    _ => {}
}

// Invalid hash format during verification
match hasher.verify("password", "not_a_valid_hash") {
    Err(Error::Auth(msg)) => {
        println!("Invalid hash: {}", msg);
    }
    _ => {}
}

// Wrong password returns Ok(false), not an error
match hasher.verify("wrong", &valid_hash) {
    Ok(false) => println!("Invalid credentials"),
    Ok(true) => println!("Login successful"),
    Err(e) => println!("System error: {}", e),
}
```

---

## Security Considerations

### Salt Generation

Each call to `hash()` generates a new random salt using the operating system's cryptographically secure random number generator. The same password always produces different hashes:

```rust
let hash1 = hasher.hash("password")?;
let hash2 = hasher.hash("password")?;
assert_ne!(hash1, hash2); // Different salts

// Both verify correctly
assert!(hasher.verify("password", &hash1)?);
assert!(hasher.verify("password", &hash2)?);
```

### Timing Attacks

Password verification uses constant-time comparison internally. The time to verify a password doesn't reveal information about whether characters matched.

### Memory Safety

Argon2 requires allocating significant memory (64 MiB by default). Ensure your service has sufficient memory, especially under load. Consider:

- Setting memory limits appropriate for concurrent requests
- Using a thread pool to limit parallel hashing operations
- Monitoring memory usage under peak authentication load

---

## Integration Patterns

### With Token Authentication

```rust
use acton_service::auth::{PasswordHasher, PasetoGenerator, TokenGenerator};
use acton_service::middleware::Claims;

async fn login(
    credentials: LoginRequest,
    hasher: &PasswordHasher,
    generator: &PasetoGenerator,
) -> Result<LoginResponse, Error> {
    // Load user from database
    let user = find_user(&credentials.email).await?;

    // Verify password
    if !hasher.verify(&credentials.password, &user.password_hash)? {
        return Err(Error::Auth("Invalid credentials".into()));
    }

    // Generate access token
    let claims = Claims {
        sub: user.id.to_string(),
        ..Default::default()
    };
    let token = generator.generate_token(&claims)?;

    Ok(LoginResponse { token })
}
```

### Registration Flow

```rust
async fn register(
    request: RegisterRequest,
    hasher: &PasswordHasher,
) -> Result<User, Error> {
    // Hash password (validates length internally)
    let password_hash = hasher.hash(&request.password)?;

    // Create user with hashed password
    let user = create_user(CreateUser {
        email: request.email,
        password_hash,
        ..Default::default()
    }).await?;

    Ok(user)
}
```

---

## API Reference

### PasswordHasher

```rust
impl PasswordHasher {
    /// Create with custom configuration
    pub fn new(config: PasswordConfig) -> Self;

    /// Create with OWASP defaults
    pub fn default() -> Self;

    /// Hash a password, returning PHC string
    pub fn hash(&self, password: &str) -> Result<String, Error>;

    /// Verify password against PHC hash
    pub fn verify(&self, password: &str, hash: &str) -> Result<bool, Error>;

    /// Check if hash needs upgrading to current parameters
    pub fn needs_rehash(&self, hash: &str) -> bool;

    /// Get configured minimum password length
    pub fn min_password_length(&self) -> usize;
}
```

### PasswordConfig

```rust
pub struct PasswordConfig {
    /// Memory cost in KiB (default: 65536 = 64 MiB)
    pub memory_cost_kib: u32,

    /// Time cost / iterations (default: 3)
    pub time_cost: u32,

    /// Parallelism degree (default: 4)
    pub parallelism: u32,

    /// Minimum password length (default: 8)
    pub min_password_length: usize,
}
```

---

## Next Steps

- [Token Generation](/docs/token-generation) - Generate access and refresh tokens after authentication
- [Authentication Overview](/docs/auth) - All auth capabilities
- [API Keys](/docs/api-keys) - Machine-to-machine authentication
