-- name: PlaceOrder :exec
INSERT INTO orders_by_customer (customer_id, order_time, order_id, status, total)
VALUES (?, now(), uuid(), 'new', ?);

-- name: OrdersSince :many
SELECT order_time, order_id, status, total, toTimestamp(order_time) AS placed_at
FROM orders_by_customer
WHERE customer_id = ? AND order_time > minTimeuuid(?) AND order_time < maxTimeuuid(?)
ORDER BY order_time DESC
LIMIT ?;

-- name: OrdersPage :many
SELECT order_time, order_id, status FROM orders_by_customer
WHERE customer_id = ? AND (order_time, order_id) < (?, ?)
LIMIT ?;

-- name: CountOrders :one
SELECT count(*) AS n FROM orders_by_customer WHERE customer_id = ?;

-- name: Totals :one
SELECT min(total) AS lowest, max(total) AS highest, sum(total) AS sum_total, avg(total) AS avg_total
FROM orders_by_customer WHERE customer_id = ?;

-- allow: CQL201
-- name: CustomersWithOrders :many
SELECT DISTINCT customer_id FROM orders_by_customer;

-- allow: CQL201
-- name: LatestOrderPerCustomer :many
SELECT customer_id, order_time, status FROM orders_by_customer PER PARTITION LIMIT 1;

-- allow: CQL201
-- name: OrdersPerCustomer :many
SELECT customer_id, count(*) AS n FROM orders_by_customer GROUP BY customer_id;

-- name: ShipOrder :lwt
UPDATE orders_by_customer SET status = 'shipped'
WHERE customer_id = ? AND order_time = ? AND order_id = ? IF status = ?;
