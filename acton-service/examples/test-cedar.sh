#!/usr/bin/env bash
set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
PORT=8080
BASE_URL="http://localhost:${PORT}"
PRIVATE_KEY="acton-service/examples/jwt-private.pem"

# Print colored message
print_info() {
    echo -e "${BLUE}ℹ${NC} $1"
}

print_success() {
    echo -e "${GREEN}✓${NC} $1"
}

print_error() {
    echo -e "${RED}✗${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}⚠${NC} $1"
}

# Check if Redis is running
check_redis() {
    if command -v redis-cli &> /dev/null; then
        if redis-cli ping &> /dev/null; then
            print_success "Redis is running"
            return 0
        else
            print_warning "Redis is not running (optional but recommended)"
            return 1
        fi
    else
        print_warning "Redis CLI not found (optional but recommended)"
        return 1
    fi
}

# Start Redis using Docker
start_redis() {
    if ! command -v docker &> /dev/null; then
        print_warning "Docker not found. Skipping Redis (optional for caching)"
        return 1
    fi

    print_info "Starting Redis with Docker..."
    if ! docker ps | grep -q redis; then
        docker run -d -p 6379:6379 --name acton-redis redis:latest
        sleep 2
        print_success "Redis started"
    else
        print_info "Redis container already running"
    fi
}

# Generate JWT tokens using Python
generate_tokens() {
    print_info "Generating JWT tokens..."

    # Check if Python is available
    if ! command -v python3 &> /dev/null; then
        print_error "Python3 not found. Please install Python3 to generate tokens."
        exit 1
    fi

    # Check if uv is available
    if ! command -v uv &> /dev/null; then
        print_error "uv package manager not found. Please install uv first:"
        print_info "curl -LsSf https://astral.sh/uv/install.sh | sh"
        exit 1
    fi

    # Create venv if it doesn't exist
    if [ ! -d ".venv" ]; then
        print_info "Creating virtual environment..."
        uv venv .venv
    fi

    # Activate venv and install dependencies
    source .venv/bin/activate

    # Check if PyJWT is installed
    if ! python3 -c "import jwt" 2> /dev/null; then
        print_info "Installing PyJWT with uv..."
        uv pip install pyjwt cryptography
    fi

    # Generate tokens using Python
    python3 - <<EOF
import jwt
from datetime import datetime, timedelta, UTC

# Read the JWT private key
with open("${PRIVATE_KEY}", "r") as f:
    private_key = f.read()

# Generate USER token (regular user)
user_payload = {
    "sub": "user:123",
    "username": "alice",
    "email": "alice@example.com",
    "roles": ["user"],
    "perms": ["read:documents", "write:documents"],
    "exp": int((datetime.now(UTC) + timedelta(hours=1)).timestamp()),
    "iat": int(datetime.now(UTC).timestamp()),
    "jti": "test-user-token"
}
user_token = jwt.encode(user_payload, private_key, algorithm="RS256")

# Generate ADMIN token
admin_payload = {
    "sub": "user:456",
    "username": "bob",
    "email": "bob@example.com",
    "roles": ["user", "admin"],
    "perms": ["read:documents", "write:documents", "admin:all"],
    "exp": int((datetime.now(UTC) + timedelta(hours=1)).timestamp()),
    "iat": int(datetime.now(UTC).timestamp()),
    "jti": "test-admin-token"
}
admin_token = jwt.encode(admin_payload, private_key, algorithm="RS256")

# Save tokens to temporary file
with open("/tmp/cedar-tokens.sh", "w") as f:
    f.write(f'export USER_TOKEN="{user_token}"\n')
    f.write(f'export ADMIN_TOKEN="{admin_token}"\n')

print("Tokens generated successfully!")
EOF

    # Source the tokens
    source /tmp/cedar-tokens.sh
    print_success "Tokens generated and exported"
    echo ""
    echo "USER_TOKEN (alice - regular user):"
    echo "${USER_TOKEN}" | cut -c1-50
    echo "..."
    echo ""
    echo "ADMIN_TOKEN (bob - admin user):"
    echo "${ADMIN_TOKEN}" | cut -c1-50
    echo "..."
}

# Run tests
run_tests() {
    print_info "Running Cedar authorization tests..."
    echo ""

    # Test 1: Health endpoint (no auth)
    echo -e "${BLUE}Test 1: Health endpoint (no auth required)${NC}"
    curl -s -w "\nStatus: %{http_code}\n" ${BASE_URL}/health
    echo ""

    # Test 2: No authentication (should fail)
    echo -e "${BLUE}Test 2: Access documents without token (should fail with 401)${NC}"
    curl -s -w "\nStatus: %{http_code}\n" ${BASE_URL}/api/v1/documents
    echo ""

    # Test 3: User can list documents
    echo -e "${BLUE}Test 3: User lists documents (should succeed)${NC}"
    curl -s -w "\nStatus: %{http_code}\n" \
        -H "Authorization: Bearer ${USER_TOKEN}" \
        ${BASE_URL}/api/v1/documents
    echo ""

    # Test 4: User cannot access admin endpoint
    echo -e "${BLUE}Test 4: User accesses admin endpoint (should fail with 403)${NC}"
    curl -s -w "\nStatus: %{http_code}\n" \
        -H "Authorization: Bearer ${USER_TOKEN}" \
        ${BASE_URL}/api/v1/admin/users
    echo ""

    # Test 5: Admin can access admin endpoint
    echo -e "${BLUE}Test 5: Admin accesses admin endpoint (should succeed)${NC}"
    curl -s -w "\nStatus: %{http_code}\n" \
        -H "Authorization: Bearer ${ADMIN_TOKEN}" \
        ${BASE_URL}/api/v1/admin/users
    echo ""

    # Test 6: User creates a document
    echo -e "${BLUE}Test 6: User creates document (should succeed)${NC}"
    curl -s -w "\nStatus: %{http_code}\n" \
        -X POST \
        -H "Authorization: Bearer ${USER_TOKEN}" \
        -H "Content-Type: application/json" \
        -d '{"id":"doc-test","owner_id":"user123","title":"Test Document","content":"Test content"}' \
        ${BASE_URL}/api/v1/documents
    echo ""

    # Test 7: User gets their own document
    echo -e "${BLUE}Test 7: User gets own document (should succeed)${NC}"
    curl -s -w "\nStatus: %{http_code}\n" \
        -H "Authorization: Bearer ${USER_TOKEN}" \
        ${BASE_URL}/api/v1/documents/user123/doc1
    echo ""

    # Test 8: User updates their own document
    echo -e "${BLUE}Test 8: User updates own document (should succeed)${NC}"
    curl -s -w "\nStatus: %{http_code}\n" \
        -X PUT \
        -H "Authorization: Bearer ${USER_TOKEN}" \
        -H "Content-Type: application/json" \
        -d '{"id":"doc1","owner_id":"user123","title":"Updated Title","content":"Updated content"}' \
        ${BASE_URL}/api/v1/documents/user123/doc1
    echo ""
}

# Main menu
show_menu() {
    echo ""
    echo -e "${GREEN}═══════════════════════════════════════${NC}"
    echo -e "${GREEN}  Cedar Authorization Test Script${NC}"
    echo -e "${GREEN}═══════════════════════════════════════${NC}"
    echo ""
    echo "1) Start Redis (Docker)"
    echo "2) Run Cedar example"
    echo "3) Generate JWT tokens"
    echo "4) Run all tests"
    echo "5) Run custom curl command"
    echo "6) Full setup (Redis + Example + Tokens + Tests)"
    echo "0) Exit"
    echo ""
}

# Run the example
run_example() {
    print_info "Starting Cedar authorization example..."
    cargo run --manifest-path=acton-service/Cargo.toml --example cedar-authz --features cedar-authz,cache
}

# Custom curl command
custom_curl() {
    echo ""
    echo "Available tokens:"
    echo "  \$USER_TOKEN  - Regular user (alice)"
    echo "  \$ADMIN_TOKEN - Admin user (bob)"
    echo ""
    read -p "Enter curl command (e.g., curl -H \"Authorization: Bearer \$USER_TOKEN\" ${BASE_URL}/api/v1/documents): " cmd
    eval "$cmd"
    echo ""
}

# Full setup
full_setup() {
    check_redis || start_redis
    generate_tokens

    print_info "Starting the Cedar example in the background..."
    cargo run --manifest-path=acton-service/Cargo.toml --example cedar-authz --features cedar-authz,cache &
    EXAMPLE_PID=$!

    print_info "Waiting for service to start..."
    sleep 5

    # Wait for service to be ready
    max_attempts=30
    attempt=0
    while [ $attempt -lt $max_attempts ]; do
        if curl -s ${BASE_URL}/health > /dev/null 2>&1; then
            print_success "Service is ready!"
            break
        fi
        attempt=$((attempt + 1))
        sleep 1
    done

    if [ $attempt -eq $max_attempts ]; then
        print_error "Service failed to start"
        kill $EXAMPLE_PID 2>/dev/null
        exit 1
    fi

    run_tests

    print_info "Example is still running in background (PID: $EXAMPLE_PID)"
    print_info "Press Ctrl+C to stop or run 'kill $EXAMPLE_PID'"
}

# Main loop
main() {
    # If no arguments, show interactive menu
    if [ $# -eq 0 ]; then
        while true; do
            show_menu
            read -p "Select option: " choice
            case $choice in
                1) start_redis ;;
                2) run_example ;;
                3) generate_tokens ;;
                4)
                    if [ -z "$USER_TOKEN" ]; then
                        generate_tokens
                    fi
                    run_tests
                    ;;
                5)
                    if [ -z "$USER_TOKEN" ]; then
                        generate_tokens
                    fi
                    custom_curl
                    ;;
                6) full_setup ;;
                0) exit 0 ;;
                *) print_error "Invalid option" ;;
            esac
        done
    else
        # Handle command-line arguments
        case "$1" in
            redis) start_redis ;;
            run) run_example ;;
            tokens) generate_tokens ;;
            test)
                if [ -z "$USER_TOKEN" ]; then
                    generate_tokens
                fi
                run_tests
                ;;
            full) full_setup ;;
            *)
                echo "Usage: $0 [redis|run|tokens|test|full]"
                echo ""
                echo "  redis  - Start Redis with Docker"
                echo "  run    - Run the Cedar example"
                echo "  tokens - Generate JWT tokens"
                echo "  test   - Run all tests"
                echo "  full   - Full setup and test"
                echo ""
                echo "Or run without arguments for interactive menu"
                exit 1
                ;;
        esac
    fi
}

main "$@"
