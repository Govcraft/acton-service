# Database Example

This example demonstrates how to use acton-service with PostgreSQL for database operations including CRUD operations, queries, and transactions.

## Features Demonstrated

- PostgreSQL connection pooling via SQLx
- CRUD operations on products and orders
- Query filtering and pagination
- Database statistics endpoint
- Error handling for database operations
- Automatic configuration via environment variables

## Prerequisites

- Docker and Docker Compose
- Rust toolchain

## Quick Start

### 1. Start the Database

```bash
cd acton-service/examples/database
docker compose up -d
```

This will:
- Start a PostgreSQL 16 container
- Create the database schema (products, orders, order_items tables)
- Insert seed data for testing

### 2. Run the Example

```bash
# From the workspace root
# Port 5433 to avoid conflicts with local PostgreSQL installations
export ACTON_DATABASE_URL="postgres://acton:acton_secret@localhost:5433/acton_example"
cargo run --example database-api --features database
```

The service will start on `http://localhost:8080`.

## API Endpoints

### Health & Readiness

```bash
# Health check (includes database connectivity)
curl http://localhost:8080/health

# Readiness check
curl http://localhost:8080/ready
```

### Products

```bash
# List all products
curl http://localhost:8080/api/v1/products

# List products by category
curl "http://localhost:8080/api/v1/products?category=Electronics"

# List only active products
curl "http://localhost:8080/api/v1/products?active_only=true"

# Get a specific product
curl http://localhost:8080/api/v1/products/550e8400-e29b-41d4-a716-446655440001

# Create a new product
curl -X POST http://localhost:8080/api/v1/products \
  -H "Content-Type: application/json" \
  -d '{
    "name": "New Gadget",
    "description": "A cool new gadget",
    "price_cents": 4999,
    "stock_quantity": 100,
    "category": "Electronics"
  }'

# Update a product
curl -X PUT http://localhost:8080/api/v1/products/550e8400-e29b-41d4-a716-446655440001 \
  -H "Content-Type: application/json" \
  -d '{
    "name": "Updated Mouse",
    "price_cents": 3499
  }'

# Delete a product
curl -X DELETE http://localhost:8080/api/v1/products/550e8400-e29b-41d4-a716-446655440001
```

### Orders

```bash
# List all orders
curl http://localhost:8080/api/v1/orders

# Get order with items
curl http://localhost:8080/api/v1/orders/660e8400-e29b-41d4-a716-446655440001
```

### Statistics

```bash
# Get database statistics
curl http://localhost:8080/api/v1/stats
```

Example response:
```json
{
  "total_products": 10,
  "active_products": 9,
  "total_orders": 4,
  "orders_by_status": [
    {"status": "delivered", "count": 1},
    {"status": "shipped", "count": 1},
    {"status": "confirmed", "count": 1},
    {"status": "pending", "count": 1}
  ],
  "total_revenue_cents": 49494
}
```

## Configuration

The example uses acton-service's standard configuration system. Database configuration can be set via environment variables:

| Variable | Description | Default |
|----------|-------------|---------|
| `ACTON_DATABASE_URL` | PostgreSQL connection URL (use port 5433 for this example) | (required) |
| `ACTON_DATABASE_MAX_CONNECTIONS` | Maximum pool connections | 50 |
| `ACTON_DATABASE_MIN_CONNECTIONS` | Minimum pool connections | 5 |
| `ACTON_DATABASE_CONNECTION_TIMEOUT_SECS` | Connection timeout | 10 |
| `ACTON_DATABASE_MAX_RETRIES` | Connection retry attempts | 5 |
| `ACTON_DATABASE_RETRY_DELAY_SECS` | Delay between retries | 2 |
| `ACTON_DATABASE_LAZY_INIT` | Lazy pool initialization | true |
| `ACTON_DATABASE_OPTIONAL` | Service starts without DB | false |

## Database Schema

### Products Table

| Column | Type | Description |
|--------|------|-------------|
| id | UUID | Primary key |
| name | VARCHAR(255) | Product name |
| description | TEXT | Product description |
| price_cents | BIGINT | Price in cents |
| stock_quantity | INTEGER | Stock count |
| category | VARCHAR(100) | Product category |
| is_active | BOOLEAN | Active status |
| created_at | TIMESTAMPTZ | Creation timestamp |
| updated_at | TIMESTAMPTZ | Last update timestamp |

### Orders Table

| Column | Type | Description |
|--------|------|-------------|
| id | UUID | Primary key |
| customer_email | VARCHAR(255) | Customer email |
| customer_name | VARCHAR(255) | Customer name |
| status | VARCHAR(50) | Order status |
| total_cents | BIGINT | Total in cents |
| created_at | TIMESTAMPTZ | Creation timestamp |
| updated_at | TIMESTAMPTZ | Last update timestamp |

### Order Items Table

| Column | Type | Description |
|--------|------|-------------|
| id | UUID | Primary key |
| order_id | UUID | Foreign key to orders |
| product_id | UUID | Foreign key to products |
| quantity | INTEGER | Item quantity |
| unit_price_cents | BIGINT | Price at purchase |
| created_at | TIMESTAMPTZ | Creation timestamp |

## Cleanup

Stop and remove the database container:

```bash
cd acton-service/examples/database
docker compose down -v
```

The `-v` flag removes the data volume for a clean slate.

## Troubleshooting

### Database Connection Failed

1. Verify Docker is running: `docker ps`
2. Check container logs: `docker compose logs postgres`
3. Verify the connection URL matches the docker-compose configuration
4. Ensure port 5432 is not in use by another service

### Migrations Not Applied

The migrations run automatically when the container starts. If you need to reapply:

```bash
docker compose down -v
docker compose up -d
```

### Permission Denied

If you see permission errors, ensure your user has access to Docker:

```bash
sudo usermod -aG docker $USER
# Log out and back in
```
