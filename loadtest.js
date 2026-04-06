import http from 'k6/http';
import { check, sleep } from 'k6';
import { Rate, Trend } from 'k6/metrics';

const errorRate = new Rate('errors');
const healthLatency = new Trend('health_latency', true);
const createOrderLatency = new Trend('create_order_latency', true);
const getOrderLatency = new Trend('get_order_latency', true);
const createItemLatency = new Trend('create_item_latency', true);

const BASE_URL = __ENV.BASE_URL || 'http://localhost:8080';
const VUS = parseInt(__ENV.VUS || '10');
const DURATION = __ENV.DURATION || '30s';

export const options = {
  stages: [
    { duration: '10s', target: VUS },
    { duration: DURATION, target: VUS },
    { duration: '5s', target: 0 },
  ],
  thresholds: {
    http_req_duration: ['p(95)<500', 'p(99)<1000'],
    http_req_failed: ['rate<0.01'],
    errors: ['rate<0.01'],
  },
};

export default function () {
  const headers = { 'Content-Type': 'application/json' };

  {
    const res = http.get(`${BASE_URL}/health`);
    healthLatency.add(res.timings.duration);
    errorRate.add(!check(res, { 'health 200': (r) => r.status === 200 }));
  }

  let orderId = null;
  {
    const res = http.post(
      `${BASE_URL}/orders`,
      JSON.stringify({
        customer_id: `cust-${__VU}`,
        item_id: `item-${Math.floor(Math.random() * 100)}`,
        quantity: Math.floor(Math.random() * 10) + 1,
      }),
      {
        headers: {
          ...headers,
          'Idempotency-Key': `vu${__VU}-iter${__ITER}`,
        },
      }
    );
    createOrderLatency.add(res.timings.duration);
    errorRate.add(!check(res, { 'create order 201': (r) => r.status === 201 }));
    if (res.status === 201) {
      try { orderId = JSON.parse(res.body).order_id; } catch (_) {}
    }
  }

  if (orderId) {
    const res = http.get(`${BASE_URL}/orders/${orderId}`);
    getOrderLatency.add(res.timings.duration);
    errorRate.add(!check(res, { 'get order 200': (r) => r.status === 200 }));
  }

  let itemId = null;
  {
    const res = http.post(
      `${BASE_URL}/items`,
      JSON.stringify({
        name: `item-vu${__VU}-iter${__ITER}`,
        value: Math.floor(Math.random() * 1000),
      }),
      { headers }
    );
    createItemLatency.add(res.timings.duration);
    errorRate.add(!check(res, { 'create item 201': (r) => r.status === 201 }));
    if (res.status === 201) {
      try { itemId = JSON.parse(res.body).id; } catch (_) {}
    }
  }

  if (itemId) {
    const res = http.get(`${BASE_URL}/items/${itemId}`);
    errorRate.add(!check(res, { 'get item 200': (r) => r.status === 200 }));
  }

  sleep(0.5);
}
