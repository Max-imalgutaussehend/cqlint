-- Cassandra 5.0 features: vector type, SAI indexes, dynamic data masking, a materialized-view-free model.
CREATE TABLE IF NOT EXISTS products (
    id uuid PRIMARY KEY,
    name text,
    category text,
    price double,
    stock int,
    tags set<text>,
    embedding vector<float, 3>,
    supplier_email text MASKED WITH DEFAULT
);

CREATE INDEX IF NOT EXISTS products_embedding_idx ON products (embedding) USING 'sai';
CREATE INDEX IF NOT EXISTS products_category_idx ON products (category) USING 'sai';
CREATE INDEX IF NOT EXISTS products_price_idx ON products (price) USING 'sai';
CREATE INDEX IF NOT EXISTS products_tags_idx ON products (tags) USING 'sai';
