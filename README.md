# cloud-assignment-4

Built on top of HW3. We added a sliding-window rate limiter that tracks requests per IP and returns 429 once the limit is hit. The limiter runs as a middleware layer so no handler code needed to change.

---

## Local Setup (Docker)

```bash
echo "DB_PASSWORD=testpass123" > .env
docker compose up -d --build
curl -i http://localhost:8080/health
```

---

## Rate Limiter Demo

### Step 1 - Check the system is healthy

```bash
curl -i http://localhost:8080/health
```

You should see `200 OK` with `X-RateLimit-Limit` and `X-RateLimit-Remaining` in the headers.

### Step 2 - Hit the limit with rapid requests

Send 8 requests back to back (limit is set to 5):

```bash
for i in $(seq 1 8); do
  echo -n "Request $i: "
  curl -s -o /dev/null -w "%{http_code}\n" http://localhost:8080/health
done
```

Expected output:

```
Request 1: 200
Request 2: 200
Request 3: 200
Request 4: 200
Request 5: 200
Request 6: 429
Request 7: 429
Request 8: 429
```

### Step 3 - Look at the 429 response

```bash
curl -i http://localhost:8080/health
```

You'll see `retry-after: 10`, `x-ratelimit-remaining: 0`, and a JSON error body.

### Step 4 - Wait for the window to reset

```bash
sleep 11
curl -i http://localhost:8080/health
```

### Step 5 - Confirm everything still works

```bash
curl -s -X POST http://localhost:8080/items \
  -H "Content-Type: application/json" \
  -d '{"name":"widget","value":42}'

curl -s http://localhost:8080/items/1

curl -s -X POST http://localhost:8080/orders \
  -H "Content-Type: application/json" \
  -H "Idempotency-Key: demo-order-1" \
  -d '{"customer_id":"cust-1","item_id":"item-1","quantity":3}'
```

---

## Demo Commands

**1. Start the system**
```bash
docker compose up -d --build
curl -i http://localhost:8080/health
```

**2. Normal operations**
```bash
curl -s -X POST http://localhost:8080/items \
  -H "Content-Type: application/json" \
  -d '{"name":"widget","value":42}'

curl -s http://localhost:8080/items/1

curl -s -X POST http://localhost:8080/orders \
  -H "Content-Type: application/json" \
  -H "Idempotency-Key: demo-order-1" \
  -d '{"customer_id":"cust-1","item_id":"item-1","quantity":3}'
```

**3. Trigger rate limiting**
```bash
for i in $(seq 1 8); do
  echo -n "Request $i: "
  curl -s -o /dev/null -w "%{http_code}\n" http://localhost:8080/health
done
```

**4. Inspect the 429**
```bash
curl -i http://localhost:8080/health
```

**5. Wait for window reset**
```bash
sleep 11
curl -i http://localhost:8080/health
```

**6. Idempotency replay**
```bash
curl -s -X POST http://localhost:8080/orders \
  -H "Content-Type: application/json" \
  -H "Idempotency-Key: demo-order-1" \
  -d '{"customer_id":"cust-1","item_id":"item-1","quantity":3}'
```

---

## Load Test

```bash
k6 run loadtest.js
```

---

## API Reference

| Method | Path | Description |
|---|---|---|
| `GET` | `/health` | Returns `{"status":"ok","db":"connected"}` |
| `POST` | `/items` | Create item, body: `{"name":string,"value":int}` |
| `GET` | `/items/{id}` | Get item by id |
| `POST` | `/orders` | Create order, requires `Idempotency-Key` header |
| `GET` | `/orders/{order_id}` | Get order by UUID |

---

## Database Schema

| Table | Primary Key | Purpose |
|---|---|---|
| `items` | `id` SERIAL | Product catalogue |
| `orders` | `order_id` TEXT | Customer orders |
| `ledger` | `ledger_id` TEXT | Financial record per order |
| `idempotency_records` | `idempotency_key` TEXT | Deduplication for POST /orders |

---

# Test Scenarios

## 1) Scaling - 10x Traffic

**What scales:**
- The ALB handles it automatically, nothing needs to change there.
- Since ECS tasks are stateless, we can just bump up `desired_count` to run more of them.
- Axum is async so each task handles a lot of connections at once, horizontal scaling works well here.

**What becomes a bottleneck:**
- RDS is the main issue. `POST /orders` does 3 table inserts per request inside a transaction, and at 10x that starts to overwhelm a `db.t3.micro`. We'd probably upgrade the instance class or move to Aurora.
- When a new container spins up, its rate-limiter counters start from zero, so a client gets a short free window on that container until it catches up.

## 2) Failure - One Container Crashes

1. ECS picks up that the task is gone within a few seconds.
2. The ALB removes it from rotation after 3 failed health checks.
3. All traffic goes to the other running tasks. If we have 2 or more, users don't notice anything.
4. ECS spins up a replacement and it's back and healthy in about 60 seconds.
5. That container's rate-limiter state is gone but it doesn't touch any data so there's no real harm.

## 3) Consistency - Stale Data

Right now with a single primary, stale reads can't happen. Every request hits the same database.

If we added read replicas later, someone could write a new item and then immediately read it back from a replica that hasn't caught up yet. That's just replica lag and it's usually under 10ms.

Data correctness is still fine because:
- All writes go to the primary.
- The idempotency check for orders always hits the primary, so duplicate orders can't slip through even if a read replica is a bit behind.

## 4) Extension - Rate Limiting

See the **Rate Limiter Demo** section above for the commands.

What you'll see:
- The first N requests come back `200 OK` and `X-RateLimit-Remaining` counts down with each one.
- Once the limit is hit, the next request gets `429 Too Many Requests` with a `Retry-After` header.
- After the window expires, requests go through normally again.
- Each IP is tracked on its own, so one client hitting the limit has no effect on anyone else.
