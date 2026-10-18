CREATE TYPE address (
    street text,
    city text,
    zip int
);

CREATE TABLE customers (
    customer_id uuid PRIMARY KEY,
    name text,
    email text,
    vip boolean,
    birthday date,
    created_at timestamp,
    primary_address frozen<address>,
    addresses map<text, frozen<address>>,
    tags set<text>,
    phones list<text>
);

CREATE TABLE orders_by_customer (
    customer_id uuid,
    order_time timeuuid,
    order_id uuid,
    status text,
    total decimal,
    PRIMARY KEY ((customer_id), order_time, order_id)
) WITH CLUSTERING ORDER BY (order_time DESC, order_id ASC);

CREATE TABLE product_views (
    product_id uuid PRIMARY KEY,
    views counter
);

CREATE TABLE sensor_readings (
    sensor_id text,
    day date,
    ts timestamp,
    value double,
    PRIMARY KEY ((sensor_id, day), ts)
);

CREATE TABLE inventory (
    sku text PRIMARY KEY,
    stock int,
    reserved int
);

CREATE TABLE sessions (
    session_token text PRIMARY KEY,
    customer_id uuid
);
