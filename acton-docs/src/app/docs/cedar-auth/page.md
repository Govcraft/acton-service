---
title: Cedar Authorization
nextjs:
  metadata:
    title: Cedar Authorization
    description: Implement fine-grained authorization with AWS Cedar policies for resource-based, role-based, and attribute-based access control.
---

{% callout type="note" title="New to acton-service?" %}
Start with the [homepage](/) to understand what acton-service is, then explore [Core Concepts](/docs/concepts) for foundational explanations. See the [Glossary](/docs/glossary) for technical term definitions.
{% /callout %}

---


acton-service integrates AWS Cedar for declarative, policy-based authorization. Define who can do what with which resources using human-readable policy files.

## What You'll Learn

- Policy-based access control with admin vs user roles
- Resource ownership patterns (users can only access their own documents)
- Custom path normalization for alphanumeric IDs
- Layered security with JWT authentication + Cedar authorization
- Optional Redis caching for sub-5ms policy decisions

## Quick Start

```bash
cargo run --example cedar-authz --features cedar-authz,cache
```

The example automatically creates configuration files in `~/.config/acton-service/cedar-authz-example/`:
- `policies.cedar` - Cedar policy definitions
- `jwt-public.pem` - JWT public key for token validation
- `config.toml` - Service configuration

Server starts on `http://localhost:8080`

### Optional: Enable Policy Decision Caching

For faster policy decisions (1-5ms instead of 10-50ms), start Redis:

```bash
docker run -d -p 6379:6379 redis:latest
```

Without Redis, policy evaluation is still perfectly usable at 10-50ms latency.

## Testing Authorization

### Step 1: Verify Health Endpoints (No Auth Required)

```bash
# Health check - should return 200 OK
curl http://localhost:8080/health

# Readiness check - should return 200 OK
curl http://localhost:8080/ready
```

### Step 2: Test Without Authentication (Should Fail)

```bash
# Try to access documents without a token - should return 401 Unauthorized
curl http://localhost:8080/api/v1/documents
```

### Step 3: Generate Test JWT Tokens

Install PyJWT for token generation:

```bash
# Create virtual environment and install PyJWT
uv venv .venv
source .venv/bin/activate
uv pip install pyjwt cryptography
```

Generate tokens with Python:

```python
import jwt
from datetime import datetime, timedelta, UTC

# Read the JWT private key (included in examples/)
with open("acton-service/examples/jwt-private.pem", "r") as f:
    private_key = f.read()

# Generate USER token (regular user)
user_payload = {
    "sub": "user:123",
    "username": "alice",
    "email": "alice@example.com",
    "roles": ["user"],  # Regular user role
    "perms": ["read:documents", "write:documents"],
    "exp": int((datetime.now(UTC) + timedelta(hours=1)).timestamp()),
    "iat": int(datetime.now(UTC).timestamp()),
    "jti": "test-user-token"
}
user_token = jwt.encode(user_payload, private_key, algorithm="RS256")
print("USER TOKEN:")
print(user_token)
print()

# Generate ADMIN token (admin user)
admin_payload = {
    "sub": "user:456",
    "username": "bob",
    "email": "bob@example.com",
    "roles": ["user", "admin"],  # Admin role
    "perms": ["read:documents", "write:documents", "admin:all"],
    "exp": int((datetime.now(UTC) + timedelta(hours=1)).timestamp()),
    "iat": int(datetime.now(UTC).timestamp()),
    "jti": "test-admin-token"
}
admin_token = jwt.encode(admin_payload, private_key, algorithm="RS256")
print("ADMIN TOKEN:")
print(admin_token)
```

Save the tokens for testing:

```bash
export USER_TOKEN="<paste-user-token-here>"
export ADMIN_TOKEN="<paste-admin-token-here>"
```

### Step 4: Test Cedar Authorization Policies

**Test 1: User can list documents** ✅

```bash
curl -H "Authorization: Bearer $USER_TOKEN" \
     http://localhost:8080/api/v1/documents

# Expected: 200 OK with documents array
# [{"id":"doc1","owner_id":"user123","title":"My Document",...},...]
```

**Test 2: User CANNOT access admin endpoint** ❌

```bash
curl -H "Authorization: Bearer $USER_TOKEN" \
     http://localhost:8080/api/v1/admin/users

# Expected: 403 Forbidden
# {"error":"Access denied by policy","code":"FORBIDDEN","status":403}
```

**Test 3: Admin CAN access admin endpoint** ✅

```bash
curl -H "Authorization: Bearer $ADMIN_TOKEN" \
     http://localhost:8080/api/v1/admin/users

# Expected: 200 OK with users array
# [{"id":"user123","username":"alice","roles":["user"]},...]
```

**Test 4: User can create documents** ✅

```bash
curl -X POST \
     -H "Authorization: Bearer $USER_TOKEN" \
     -H "Content-Type: application/json" \
     -d '{"id":"doc-new","owner_id":"user123","title":"New Document","content":"Test"}' \
     http://localhost:8080/api/v1/documents

# Expected: 200 OK with created document
# {"id":"doc-new","owner_id":"user123",...}
```

**Test 5: Get specific document (Ownership check)**

```bash
curl -H "Authorization: Bearer $USER_TOKEN" \
     http://localhost:8080/api/v1/documents/user123/doc1

# Expected: 200 OK if user:123 matches the user_id in path
```

**Test 6: Update document (Owner only)**

```bash
curl -X PUT \
     -H "Authorization: Bearer $USER_TOKEN" \
     -H "Content-Type: application/json" \
     -d '{"id":"doc1","owner_id":"user123","title":"Updated","content":"New"}' \
     http://localhost:8080/api/v1/documents/user123/doc1

# Expected: 200 OK if user owns the document
```

**Test 7: Delete document (Owner or admin)**

```bash
curl -X DELETE \
     -H "Authorization: Bearer $USER_TOKEN" \
     http://localhost:8080/api/v1/documents/user123/doc1

# Expected: 200 OK if user owns the document
```

## Cedar Policy Explanation

The example policies demonstrate common authorization patterns:

### 1. Admin Override

```cedar
permit(principal, action, resource)
when { principal.roles.contains("admin") };
```

Admins bypass all restrictions and can perform any action on any resource.

### 2. Resource Listing

```cedar
permit(
    principal,
    action == Action::"GET /api/v1/documents",
    resource
);
```

Any authenticated user can list documents. No ownership check required for browsing.

### 3. Ownership-based Access

```cedar
permit(
    principal,
    action in [Action::"GET /api/v1/documents/{user_id}/{doc_id}", ...],
    resource
)
when { principal.sub == resource.owner_id };
```

Users can only access documents they own. The `owner_id` attribute from the resource must match the principal's `sub` claim.

### 4. Forbid with Unless (Restrictive)

```cedar
forbid(
    principal,
    action == Action::"GET /api/v1/admin/users",
    resource
)
unless { principal.roles.contains("admin") };
```

Explicitly deny admin endpoints to non-admin users. More restrictive than permit-only policies.

## How It Works

### Request Flow

```text
Client Request
    ↓
JWT Authentication (validates token, extracts claims)
    ↓
Cedar Authorization (evaluates policies)
    ↓
Your Handler
```

### Cedar Evaluation Model

Cedar evaluates each request using four components:

**Principal (who)**
- Extracted from JWT claims: `sub`, `roles`, `perms`, `username`, `email`
- Represents the authenticated user or service making the request

**Action (what)**
- HTTP method + normalized path
- Examples: `GET /api/v1/documents/{user_id}/{doc_id}`, `POST /api/v1/documents`

**Resource (which)**
- Path parameters or request body attributes
- Examples: `owner_id`, `document_id`, `user_id`

**Context (when/where)**
- Request metadata: `ip_address`, `timestamp`, `user_agent`
- Environmental factors for conditional policies

### Decision Logic

1. If any `forbid` policy matches → **Deny**
2. Else if any `permit` policy matches → **Allow**
3. Otherwise → **Deny** (default deny)

### Caching (Optional)

Redis caching reduces policy evaluation latency from 10-50ms to 1-5ms:

- Cache key: Hash of principal, action, resource, context
- Default TTL: 5 minutes (configurable)
- Automatic invalidation on policy reload
- Significant performance improvement for high-traffic endpoints

## Configuration Options

```toml
[cedar]
enabled = true                      # Enable/disable Cedar authorization
policy_path = "path/to/policies.cedar"  # Path to policy file
hot_reload = false                  # [IN PROGRESS] Automatic policy file watching
hot_reload_interval_secs = 60       # [IN PROGRESS] Check interval for hot-reload
cache_enabled = true                # Enable policy decision caching
cache_ttl_secs = 300                # Cache TTL in seconds
fail_open = false                   # true = allow on errors, false = deny on errors
```

**Note**: Automatic hot-reload is currently in progress. Use the manual reload endpoint (`POST /admin/reload-policies`) to reload policies without restarting the service.

### Fail-Open vs Fail-Closed

**Fail-Closed (Recommended for Production)**

```toml
fail_open = false
```

- Deny requests if policy evaluation fails
- More secure - prevents accidental access during errors
- May cause downtime if policies are misconfigured
- **Always use in production environments**

**Fail-Open (Development Only)**

```toml
fail_open = true
```

- Allow requests if policy evaluation fails
- Less secure - grants access during errors
- Useful for debugging policy issues
- **Never use in production**

## Custom Path Normalization

acton-service supports customizable path normalization to handle various ID formats:

```rust
use acton_service::middleware::CedarAuthzLayer;

let authz = CedarAuthzLayer::builder()
    .policy_path("policies.cedar")
    .path_normalizer(|path, method| {
        // Custom normalization for alphanumeric IDs
        let normalized = path
            .replace(|c: char| c.is_alphanumeric(), "{id}");
        format!("{} {}", method, normalized)
    })
    .build();
```

**Common Patterns:**

- UUID IDs: `/api/v1/documents/550e8400-e29b-41d4-a716-446655440000` → `/api/v1/documents/{id}`
- Numeric IDs: `/api/v1/users/12345` → `/api/v1/users/{id}`
- Slug IDs: `/api/v1/posts/my-blog-post` → `/api/v1/posts/{slug}`

## Troubleshooting

### 403 Forbidden

**Symptom**: All requests return 403 Forbidden

**Possible Causes:**
1. Cedar is enabled but policies are too restrictive
2. Policy file not found or invalid syntax
3. JWT claims don't match policy conditions
4. Default deny with no matching permit policies

**Solutions:**
- Check logs for Cedar evaluation details
- Verify policy file exists and is valid Cedar syntax
- Ensure JWT contains required claims (`roles`, `sub`, etc.)
- Set `fail_open = true` temporarily to debug (development only)
- Add logging to see which policies are evaluated

### 500 Internal Server Error

**Symptom**: Requests return 500 errors

**Possible Causes:**
1. Policy file syntax errors (invalid Cedar)
2. Policy evaluation errors (missing attributes)
3. Cache connection issues (if Redis enabled)

**Solutions:**
- Check logs for policy parsing errors
- Validate policy syntax with Cedar CLI tools
- Verify Redis is running (if cache enabled)
- Test with `cache_enabled = false` to isolate issue

### Policy Not Reloading

**Symptom**: Policy changes don't take effect

**Current Status**: Automatic hot-reload is in progress. Policies must be reloaded manually.

**Solutions:**
- Use the manual reload endpoint: `POST /admin/reload-policies` (requires admin role)
- Restart the service to load updated policies
- Check file permissions on policy file (must be readable)

**Future**: Automatic file watching and hot-reload will be implemented soon.

## Performance Tips

1. **Enable caching**: Reduces latency by 90% (10-50ms → 1-5ms)
2. **Use simple policies**: Complex conditions increase evaluation time
3. **Cache warm-up**: First requests may be slower as cache populates
4. **Monitor cache hit rate**: Aim for >80% hit rate in production
5. **Optimize policy order**: Put most common permits first
6. **Use forbid sparingly**: Permit-based policies are typically faster

## Security Best Practices

1. **Always use fail-closed in production**: `fail_open = false`
2. **Validate JWT properly**: Use strong algorithms (RS256, ES256)
3. **Principle of least privilege**: Only grant necessary permissions
4. **Audit policies regularly**: Review and update policies quarterly
5. **Use forbid for sensitive operations**: Explicit denials are safer than implicit
6. **Secure policy reload endpoint**: Protect `/admin/reload-policies` with admin-only access
7. **Secure policy files**: Restrict file permissions (automatic hot-reload in progress)
8. **Test policy changes**: Validate in staging before production deployment
9. **Monitor authorization decisions**: Track allow/deny rates and investigate anomalies
10. **Version control policies**: Track policy changes in git for audit trail

## Integration Patterns

### JWT + Cedar Layered Security

```rust
ServiceBuilder::new()
    .with_routes(routes)
    .with_middleware(|router| {
        router
            .layer(JwtAuth::new("secret"))      // First: Authenticate
            .layer(CedarAuthzLayer::new(config)) // Second: Authorize
    })
    .build()
```

JWT provides authentication (who you are), Cedar provides authorization (what you can do).

### gRPC Support

Cedar works identically for gRPC services:

```rust
ServiceBuilder::new()
    .with_grpc_services(grpc_services)
    .with_middleware(|router| {
        router.layer(CedarAuthzLayer::new(config))
    })
    .build()
```

Path normalization handles gRPC method names automatically.

## Next Steps

1. **Add more policies**: Extend the example with your use cases
2. **Integrate with database**: Load resource attributes from DB
3. **Implement policy management API**: CRUD operations for policies
4. **Add policy testing**: Unit tests for Cedar policies
5. **Monitor policy decisions**: Track allow/deny metrics
6. **Implement policy versioning**: Deploy policies with rollback capability

## References

- [Cedar Policy Language Documentation](https://docs.cedarpolicy.com/)
- [Cedar Rust Crate Documentation](https://docs.rs/cedar-policy/)
- [JWT Authentication](/docs/jwt-auth) - Configure authentication
- [Redis Caching](/docs/cache) - Enable policy decision caching
