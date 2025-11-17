#!/usr/bin/env bash
set -e

# Cedar Authorization Example - Automated Test Suite
#
# Usage: ./test-cedar.sh
#
# This script:
# 1. Generates JWT tokens for test users
# 2. Starts the Cedar example service
# 3. Runs comprehensive authorization tests
# 4. Cleans up and reports results
#
# Requirements: python3, uv, cargo

# Colors
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

PORT=8080
BASE_URL="http://localhost:${PORT}"
PRIVATE_KEY="acton-service/examples/jwt-private.pem"

print_info() { echo -e "${BLUE}ℹ${NC} $1"; }
print_success() { echo -e "${GREEN}✓${NC} $1"; }

# Generate JWT tokens
generate_tokens() {
    print_info "Generating JWT tokens..."

    if ! command -v python3 &>/dev/null; then
        echo "Error: python3 not found. Please install Python 3."
        exit 1
    fi

    if ! command -v uv &>/dev/null; then
        echo "Error: uv not found. Install with: curl -LsSf https://astral.sh/uv/install.sh | sh"
        exit 1
    fi

    # Create venv if it doesn't exist
    if [ ! -d ".venv" ]; then
        print_info "Creating Python virtual environment..."
        if ! uv venv .venv 2>&1 | grep -v "Using Python"; then
            echo "Error: Failed to create virtual environment"
            exit 1
        fi
    fi

    # Install PyJWT if needed (use venv python directly, no need to activate)
    if ! .venv/bin/python3 -c "import jwt" 2>/dev/null; then
        print_info "Installing PyJWT..."
        if ! uv pip install --python .venv/bin/python3 -q pyjwt cryptography 2>&1; then
            echo "Error: Failed to install PyJWT"
            exit 1
        fi
    fi

    # Generate tokens using venv python
    if ! .venv/bin/python3 - 2>&1 <<EOF
import jwt
from datetime import datetime, timedelta, UTC

with open("${PRIVATE_KEY}") as f:
    key = f.read()

tokens = {}
for role, sub in [("user", "user:123"), ("admin", "user:456")]:
    payload = {
        "sub": sub,
        "username": "alice" if role == "user" else "bob",
        "email": f"{'alice' if role == 'user' else 'bob'}@example.com",
        "roles": ["user"] if role == "user" else ["user", "admin"],
        "perms": ["read:documents", "write:documents"] + (["admin:all"] if role == "admin" else []),
        "exp": int((datetime.now(UTC) + timedelta(hours=1)).timestamp()),
        "iat": int(datetime.now(UTC).timestamp())
    }
    tokens[role] = jwt.encode(payload, key, algorithm="RS256")

with open("/tmp/cedar-tokens.sh", "w") as f:
    f.write(f'export USER_TOKEN="{tokens["user"]}"\n')
    f.write(f'export ADMIN_TOKEN="{tokens["admin"]}"\n')
EOF
    then
        echo "Error: Failed to generate JWT tokens"
        exit 1
    fi

    source /tmp/cedar-tokens.sh
    print_success "JWT tokens generated"
}

# Run authorization tests
run_tests() {
    echo ""
    print_info "Running Cedar authorization tests..."
    echo ""

    local test_num=1
    run_test() {
        echo -e "${BLUE}Test $test_num: $1${NC}"
        shift
        curl -s -w "\nStatus: %{http_code}\n\n" "$@"
        test_num=$((test_num + 1))
    }

    run_test "Health endpoint (no auth)" \
        ${BASE_URL}/health

    run_test "No token (expect 401)" \
        ${BASE_URL}/api/v1/documents

    run_test "User lists documents (expect 200)" \
        -H "Authorization: Bearer ${USER_TOKEN}" \
        ${BASE_URL}/api/v1/documents

    run_test "User accesses admin endpoint (expect 403)" \
        -H "Authorization: Bearer ${USER_TOKEN}" \
        ${BASE_URL}/api/v1/admin/users

    run_test "Admin accesses admin endpoint (expect 200)" \
        -H "Authorization: Bearer ${ADMIN_TOKEN}" \
        ${BASE_URL}/api/v1/admin/users

    run_test "User creates document (expect 200)" \
        -X POST \
        -H "Authorization: Bearer ${USER_TOKEN}" \
        -H "Content-Type: application/json" \
        -d '{"id":"doc-test","owner_id":"user123","title":"Test","content":"Test"}' \
        ${BASE_URL}/api/v1/documents

    run_test "User gets own document (expect 200)" \
        -H "Authorization: Bearer ${USER_TOKEN}" \
        ${BASE_URL}/api/v1/documents/user123/doc1

    run_test "User updates own document (expect 200)" \
        -X PUT \
        -H "Authorization: Bearer ${USER_TOKEN}" \
        -H "Content-Type: application/json" \
        -d '{"id":"doc1","owner_id":"user123","title":"Updated","content":"Updated"}' \
        ${BASE_URL}/api/v1/documents/user123/doc1

    print_success "Tests complete"
}

# Wait for service to be ready
wait_for_service() {
    local max_attempts=30
    local attempt=0

    while [ $attempt -lt $max_attempts ]; do
        if curl -s ${BASE_URL}/health >/dev/null 2>&1; then
            print_success "Service is ready"
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 1
    done

    echo "Error: Service failed to start within 30 seconds"
    return 1
}

# Main execution
main() {
    echo ""
    echo -e "${GREEN}Cedar Authorization Test Script${NC}"
    echo ""

    generate_tokens

    # Start service in background
    print_info "Starting Cedar example service..."
    cargo run --manifest-path=acton-service/Cargo.toml --example cedar-authz --features cedar-authz,cache >/dev/null 2>&1 &
    SERVICE_PID=$!

    # Wait for service to start
    if ! wait_for_service; then
        kill $SERVICE_PID 2>/dev/null
        exit 1
    fi

    # Run tests
    run_tests

    # Cleanup
    echo ""
    print_info "Stopping service (PID: $SERVICE_PID)..."
    kill $SERVICE_PID 2>/dev/null
    wait $SERVICE_PID 2>/dev/null

    echo ""
    print_success "Test run complete"
}

main "$@"
