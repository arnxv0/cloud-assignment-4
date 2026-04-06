CREATE TABLE IF NOT EXISTS orders (
    order_id    TEXT PRIMARY KEY,
    customer_id TEXT NOT NULL,
    item_id     TEXT NOT NULL,
    quantity    INTEGER NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_orders_customer_id ON orders(customer_id);

CREATE TABLE IF NOT EXISTS ledger (
    ledger_id   TEXT PRIMARY KEY,
    order_id    TEXT NOT NULL REFERENCES orders(order_id),
    customer_id TEXT NOT NULL,
    amount      INTEGER NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_ledger_order_id ON ledger(order_id);

CREATE TABLE IF NOT EXISTS idempotency_records (
    idempotency_key TEXT PRIMARY KEY,
    request_hash    TEXT NOT NULL,
    response_body   TEXT NOT NULL,
    status_code     INTEGER NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS items (
    id         SERIAL PRIMARY KEY,
    name       TEXT NOT NULL,
    value      INTEGER NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_items_name ON items(name);
