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
- ✅ **Auto-setup**: Example automatically creates all necessary files!

## Quick Start

### 1. Optional: Start Redis (Recommended for caching)

```bash
docker run -d -p 6379:6379 redis:latest
```

### 2. Run the Example

```bash
cargo run --example cedar-authz --features cedar-authz,cache
```

**That's it!** The example will:
- ✅ Automatically create `~/.config/acton-service/cedar-authz-example/`
- ✅ Copy Cedar policies to `policies.cedar`
- ✅ Copy JWT public key to `jwt-public.pem`
- ✅ Copy configuration to `config.toml`
- ✅ Start the service on `http://localhost:8080`

You'll see output like:
```
🔧 Setting up example files...
   ✓ policies.cedar created
   ✓ jwt-public.pem created
   ✓ config.toml created

🚀 Cedar Authorization Example
================================
```

## Testing

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

Install PyJWT (using uv for fast installation):

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

**Test 5: Get specific document** (Ownership check)
```bash
curl -H "Authorization: Bearer $USER_TOKEN" \
     http://localhost:8080/api/v1/documents/user123/doc1

# Expected: 200 OK if user:123 matches the user_id in path
```

**Test 6: Update document** (Owner only)
```bash
curl -X PUT \
     -H "Authorization: Bearer $USER_TOKEN" \
     -H "Content-Type: application/json" \
     -d '{"id":"doc1","owner_id":"user123","title":"Updated","content":"New"}' \
     http://localhost:8080/api/v1/documents/user123/doc1

# Expected: 200 OK if user owns the document
```

**Test 7: Delete document** (Owner or admin)
```bash
curl -X DELETE \
     -H "Authorization: Bearer $USER_TOKEN" \
     http://localhost:8080/api/v1/documents/user123/doc1

# Expected: 200 OK if user owns the document
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
    action in [Action::"GET /api/v1/documents/{user_id}/{doc_id}", ...],
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
- Format: `Action::"GET /api/v1/documents/{user_id}/{doc_id}"`

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
