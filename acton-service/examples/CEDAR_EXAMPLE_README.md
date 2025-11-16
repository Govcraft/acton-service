# Cedar Authorization Example

This example demonstrates fine-grained, policy-based authorization using AWS Cedar with acton-service.

## Features

- ✅ JWT Authentication + Cedar Authorization (layered security)
- ✅ Policy-based access control with Cedar policies
- ✅ Resource ownership patterns (users can only access their own documents)
- ✅ Role-based access control (admin vs user)
- ✅ HTTP middleware integration
- ✅ Redis caching for policy decisions (optional but recommended)
- ✅ Hot-reload of policy files (optional)
- ✅ Fail-open vs fail-closed configuration

## Prerequisites

### 1. Cedar Policy File

Create the directory and policy file:

```bash
mkdir -p ~/.config/acton-service/cedar-authz-example
cp examples/policies.cedar ~/.config/acton-service/cedar-authz-example/
```

### 2. JWT Public Key

You need a JWT public key for RS256 algorithm. Generate one for testing:

```bash
# Generate private key
openssl genrsa -out jwt-private.pem 2048

# Extract public key
openssl rsa -in jwt-private.pem -pubout -out ~/.config/acton-service/cedar-authz-example/jwt-public.pem
```

### 3. Configuration File

Copy the example configuration:

```bash
cp examples/config.toml.example ~/.config/acton-service/cedar-authz-example/config.toml
```

Edit the config file to match your setup (especially JWT settings).

### 4. Redis (Optional but Recommended)

For policy decision caching, start Redis:

```bash
docker run -d -p 6379:6379 redis:latest
```

Or install Redis locally and start it.

## Running the Example

```bash
cargo run --example cedar-authz --features cedar-authz,cache
```

The service will start on `http://localhost:8080`.

## Testing

### 1. Generate a Test JWT Token

You can use various tools to generate JWT tokens. Here's an example using Python:

```python
import jwt
from datetime import datetime, timedelta

private_key = open("jwt-private.pem").read()

payload = {
    "sub": "user:123",  # User ID
    "username": "alice",
    "email": "alice@example.com",
    "roles": ["user"],  # Try ["user", "admin"] for admin access
    "perms": ["read:documents", "write:documents"],
    "exp": datetime.utcnow() + timedelta(hours=1),
    "iat": datetime.utcnow(),
    "jti": "unique-token-id-123",
}

token = jwt.encode(payload, private_key, algorithm="RS256")
print(token)
```

Save this token:

```bash
export TOKEN="your-generated-token-here"
```

### 2. Test Endpoints

**Health check (no auth required):**
```bash
curl http://localhost:8080/health
```

**List documents (any authenticated user):**
```bash
curl -H "Authorization: Bearer $TOKEN" \
     http://localhost:8080/api/v1/documents
```

**Get a specific document (owner only):**
```bash
# This will succeed if the token's sub matches the user_id in the path
curl -H "Authorization: Bearer $TOKEN" \
     http://localhost:8080/api/v1/documents/user:123/doc1
```

**Create a document (any authenticated user):**
```bash
curl -X POST \
     -H "Authorization: Bearer $TOKEN" \
     -H "Content-Type: application/json" \
     -d '{"id":"doc3","owner_id":"user:123","title":"New Doc","content":"Content"}' \
     http://localhost:8080/api/v1/documents
```

**Update a document (owner only):**
```bash
curl -X PUT \
     -H "Authorization: Bearer $TOKEN" \
     -H "Content-Type: application/json" \
     -d '{"id":"doc1","owner_id":"user:123","title":"Updated","content":"New content"}' \
     http://localhost:8080/api/v1/documents/user:123/doc1
```

**Delete a document (owner or admin only):**
```bash
curl -X DELETE \
     -H "Authorization: Bearer $TOKEN" \
     http://localhost:8080/api/v1/documents/user:123/doc1
```

**List users (admin only):**
```bash
# This will fail with 403 Forbidden unless your token has "admin" role
curl -H "Authorization: Bearer $TOKEN" \
     http://localhost:8080/api/v1/admin/users
```

### 3. Test with Different Roles

Generate tokens with different roles to test authorization:

**Regular user (can only access own documents):**
```python
payload = {
    "sub": "user:123",
    "roles": ["user"],
    # ... other fields
}
```

**Admin user (can access everything):**
```python
payload = {
    "sub": "user:456",
    "roles": ["user", "admin"],
    # ... other fields
}
```

## Cedar Policy Explanation

The example policies demonstrate common patterns:

### 1. Admin Override
```cedar
permit(principal, action, resource)
when { principal.roles.contains("admin") };
```
Admins can do everything.

### 2. Resource Listing
```cedar
permit(
    principal,
    action == Action::"GET /api/v1/documents",
    resource
);
```
Any authenticated user can list documents.

### 3. Ownership-based Access
```cedar
permit(
    principal,
    action in [Action::"GET /api/v1/documents/:user_id/:doc_id", ...],
    resource
)
when { principal.sub == resource.owner_id };
```
Users can only access their own documents.

### 4. Forbid with Unless (Restrictive)
```cedar
forbid(
    principal,
    action == Action::"GET /api/v1/admin/users",
    resource
)
unless { principal.roles.contains("admin") };
```
Only admins can access admin endpoints.

## How It Works

### 1. Request Flow

```
Client Request
    ↓
JWT Authentication Middleware (validates token, extracts claims)
    ↓
Cedar Authorization Middleware (evaluates policies)
    ↓
Business Logic (your handlers)
    ↓
Response
```

### 2. Cedar Authorization Process

For each request, Cedar evaluates:

**Principal**: Who is making the request?
- Extracted from JWT claims
- Format: `User::"user:123"`
- Attributes: `sub`, `roles`, `perms`, `email`, etc.

**Action**: What operation is being performed?
- Extracted from HTTP method + path
- Format: `Action::"GET /api/v1/documents/:user_id/:doc_id"`

**Resource**: What is being accessed?
- Extracted from path parameters or request body
- Attributes: `owner_id`, etc.

**Context**: Additional request metadata
- `ip_address`: Client IP
- `timestamp`: Request time
- `hour`, `minute`: Time-based policies

**Decision**: Allow or Deny
- If any `forbid` policy matches → **Deny**
- Else if any `permit` policy matches → **Allow**
- Otherwise → **Deny** (default)

### 3. Caching (Optional)

When cache is enabled:
- Policy decisions are cached in Redis
- Cache key: `cedar:authz:{principal}:{action}:{resource}:{context_hash}`
- TTL: Configurable (default 5 minutes)
- Reduces latency from 10-50ms to 1-5ms

## Configuration Options

### Cedar Configuration

```toml
[cedar]
enabled = true                      # Enable/disable Cedar authorization
policy_path = "path/to/policies.cedar"  # Path to policy file
hot_reload = false                  # Watch policy file for changes
hot_reload_interval_secs = 60       # Check interval for hot-reload
cache_enabled = true                # Enable policy decision caching
cache_ttl_secs = 300                # Cache TTL in seconds
fail_open = false                   # true = allow on errors, false = deny on errors
```

### Fail-Open vs Fail-Closed

**Fail-Closed (Recommended for Production)**:
```toml
fail_open = false
```
- Deny requests if policy evaluation fails
- More secure
- May cause downtime if policies are misconfigured

**Fail-Open (Development Only)**:
```toml
fail_open = true
```
- Allow requests if policy evaluation fails
- Less secure
- Useful for debugging policy issues

## Troubleshooting

### 403 Forbidden

**Symptom**: All requests return 403 Forbidden

**Possible causes**:
1. Cedar is enabled but policies are too restrictive
2. Policy file not found or invalid
3. JWT claims don't match policy conditions

**Solutions**:
- Check logs for Cedar evaluation details
- Verify policy file exists and is valid
- Ensure JWT contains required claims (`roles`, `sub`, etc.)
- Set `fail_open = true` temporarily to debug

### 500 Internal Server Error

**Symptom**: Requests return 500 errors

**Possible causes**:
1. Policy file syntax errors
2. Policy evaluation errors
3. Cache connection issues (if enabled)

**Solutions**:
- Check logs for policy parsing errors
- Validate policy syntax
- Verify Redis is running (if cache enabled)

### Policy Not Reloading

**Symptom**: Policy changes don't take effect

**Possible causes**:
1. Hot-reload is disabled
2. Hot-reload interval not reached
3. File watcher not working

**Solutions**:
- Enable hot-reload: `hot_reload = true`
- Restart the service
- Check file permissions on policy file

## Performance Tips

1. **Enable caching**: Reduces latency by 90%
2. **Use simple policies**: Complex policies increase evaluation time
3. **Cache warm-up**: First requests may be slower
4. **Monitor cache hit rate**: Aim for >80%

## Security Best Practices

1. **Always use fail-closed in production**: `fail_open = false`
2. **Validate JWT properly**: Use strong algorithms (RS256, ES256)
3. **Least privilege**: Only grant necessary permissions
4. **Audit policies regularly**: Review and update policies
5. **Use forbid for sensitive operations**: Explicit denials are safer
6. **Enable hot-reload carefully**: Only in trusted environments
7. **Secure policy files**: Restrict file permissions

## Next Steps

1. **Add more policies**: Extend the example with your use cases
2. **Integrate with database**: Load resource attributes from DB
3. **Add gRPC support**: Use `CedarAuthzLayer` for gRPC services
4. **Implement policy management API**: CRUD operations for policies
5. **Add policy testing**: Unit tests for Cedar policies
6. **Monitor policy decisions**: Track allow/deny metrics

## References

- [Cedar Policy Language Documentation](https://docs.cedarpolicy.com/)
- [Cedar Rust Crate Documentation](https://docs.rs/cedar-policy/)
- [acton-service Documentation](https://github.com/Govcraft/acton-service)
