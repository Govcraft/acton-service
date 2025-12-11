-- Seed data for the database example
-- This file is automatically executed by PostgreSQL on container startup

-- Insert sample products
INSERT INTO products (id, name, description, price_cents, stock_quantity, category, is_active) VALUES
    ('550e8400-e29b-41d4-a716-446655440001', 'Wireless Mouse', 'Ergonomic wireless mouse with USB receiver', 2999, 150, 'Electronics', true),
    ('550e8400-e29b-41d4-a716-446655440002', 'Mechanical Keyboard', 'RGB mechanical keyboard with Cherry MX switches', 12999, 75, 'Electronics', true),
    ('550e8400-e29b-41d4-a716-446655440003', 'USB-C Hub', '7-in-1 USB-C hub with HDMI and SD card reader', 4999, 200, 'Electronics', true),
    ('550e8400-e29b-41d4-a716-446655440004', 'Monitor Stand', 'Adjustable aluminum monitor stand', 7999, 50, 'Accessories', true),
    ('550e8400-e29b-41d4-a716-446655440005', 'Desk Lamp', 'LED desk lamp with adjustable brightness', 3499, 100, 'Accessories', true),
    ('550e8400-e29b-41d4-a716-446655440006', 'Webcam HD', '1080p webcam with built-in microphone', 5999, 80, 'Electronics', true),
    ('550e8400-e29b-41d4-a716-446655440007', 'Laptop Sleeve', '15-inch neoprene laptop sleeve', 1999, 300, 'Accessories', true),
    ('550e8400-e29b-41d4-a716-446655440008', 'Wireless Earbuds', 'Bluetooth 5.0 wireless earbuds with charging case', 8999, 120, 'Electronics', true),
    ('550e8400-e29b-41d4-a716-446655440009', 'Mouse Pad XL', 'Extended gaming mouse pad 900x400mm', 2499, 250, 'Accessories', true),
    ('550e8400-e29b-41d4-a716-446655440010', 'Cable Management Kit', 'Complete cable management solution', 1499, 180, 'Accessories', false)
ON CONFLICT (id) DO NOTHING;

-- Insert sample orders
INSERT INTO orders (id, customer_email, customer_name, status, total_cents) VALUES
    ('660e8400-e29b-41d4-a716-446655440001', 'alice@example.com', 'Alice Johnson', 'delivered', 15998),
    ('660e8400-e29b-41d4-a716-446655440002', 'bob@example.com', 'Bob Smith', 'shipped', 12999),
    ('660e8400-e29b-41d4-a716-446655440003', 'carol@example.com', 'Carol Williams', 'confirmed', 20497),
    ('660e8400-e29b-41d4-a716-446655440004', 'dave@example.com', 'Dave Brown', 'pending', 8999)
ON CONFLICT (id) DO NOTHING;

-- Insert sample order items
INSERT INTO order_items (order_id, product_id, quantity, unit_price_cents) VALUES
    -- Alice's order: 2x Wireless Mouse
    ('660e8400-e29b-41d4-a716-446655440001', '550e8400-e29b-41d4-a716-446655440001', 2, 2999),
    ('660e8400-e29b-41d4-a716-446655440001', '550e8400-e29b-41d4-a716-446655440007', 5, 1999),
    -- Bob's order: 1x Mechanical Keyboard
    ('660e8400-e29b-41d4-a716-446655440002', '550e8400-e29b-41d4-a716-446655440002', 1, 12999),
    -- Carol's order: USB-C Hub + Monitor Stand + Desk Lamp
    ('660e8400-e29b-41d4-a716-446655440003', '550e8400-e29b-41d4-a716-446655440003', 1, 4999),
    ('660e8400-e29b-41d4-a716-446655440003', '550e8400-e29b-41d4-a716-446655440004', 1, 7999),
    ('660e8400-e29b-41d4-a716-446655440003', '550e8400-e29b-41d4-a716-446655440005', 1, 3499),
    ('660e8400-e29b-41d4-a716-446655440003', '550e8400-e29b-41d4-a716-446655440009', 1, 2499),
    ('660e8400-e29b-41d4-a716-446655440003', '550e8400-e29b-41d4-a716-446655440007', 1, 1999),
    -- Dave's order: 1x Wireless Earbuds
    ('660e8400-e29b-41d4-a716-446655440004', '550e8400-e29b-41d4-a716-446655440008', 1, 8999);
