-- name: AddProduct :exec
INSERT INTO products (id, name, category, price, stock, tags, embedding, supplier_email)
VALUES (?, ?, ?, ?, ?, ?, ?, ?);

-- allow: CQL201
-- name: SimilarProducts :many
SELECT id, name, similarity_cosine(embedding, ?) AS score
FROM products
ORDER BY embedding ANN OF ?
LIMIT 3;

-- allow: CQL201
-- name: CheapInCategory :many
SELECT id, name, price FROM products WHERE category = ? AND price < ?;

-- allow: CQL201
-- name: ByTag :many
SELECT id, name FROM products WHERE tags CONTAINS ?;

-- allow: CQL201
-- name: PriceWithTax :many
SELECT id, price * 1.19 AS gross, abs(stock - 10) AS delta, collection_count(tags) AS tag_count
FROM products WHERE category = ?;
