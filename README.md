# cqlint

A very fast **linter and formatter for CQL** (Cassandra and ScyllaDB), written in Rust. Part of the
[cqlz family](https://max-imalgutaussehend.github.io/cqlz/cqlint.html). Built on the lexer of
[`cqlc-core`](https://github.com/Max-imalgutaussehend/cqlc).

```sh
cargo install cqlint

cqlint check .                 # lint every .cql file below the current directory
cqlint check --fix .           # apply automatic fixes
cqlint format .                # format in place
cqlint format --check .        # CI: fail if a file would change
cqlint check --schema schema/ queries/   # add type-aware checks from cqlc-core
cqlint rules                   # list the rules; `cqlint rules CQLINT004` explains one
```

## Formatter

```cql
select id,name from ks.users where id = ? and name > 'a' order by name desc limit 10;
```
```cql
SELECT id, name
FROM ks.users
WHERE id = ?
  AND name > 'a'
ORDER BY name DESC
LIMIT 10;
```

Keywords are upper-cased, each clause gets its own line, long lists wrap at 88 columns, `CREATE TABLE` puts one definition per
line, batches are indented, comments and blank lines are kept. Identifiers and quoted names are never touched. It is
**safe by construction**: after formatting, the token stream and the comments must equal the input (keyword case aside),
otherwise the file is left unchanged and an error is printed. Formatting is idempotent (tested on real schema and query files).

## Rules

| Code | Name | |
|---|---|---|
| CQLINT001 | allow-filtering | `ALLOW FILTERING` scans partitions and filters on the server |
| CQLINT002 | select-star | `SELECT *` reads every column |
| CQLINT003 | unbounded-select | `SELECT` without `WHERE` or `LIMIT` scans the whole table |
| CQLINT004 | count-scan | `COUNT` without `WHERE` reads the whole table |
| CQLINT005 | large-in | `IN` with many values fans out to many partitions |
| CQLINT006 | large-batch | a batch with many statements |
| CQLINT007 | native-secondary-index | `CREATE INDEX` without `USING` |
| CQLINT008 | lightweight-transaction | `IF ...` runs a Paxos round (note) |
| CQLINT009 | destructive | `DROP`, `TRUNCATE` |
| CQLINT010 | non-idempotent-ddl | `CREATE` without `IF NOT EXISTS`, fixable (off by default) |
| CQLINT011 | list-column | non-frozen `list<...>` (note) |
| CQLINT012 | null-write | writing `NULL` creates a tombstone |
| CQLINT013 | wide-table | table with very many columns |

With `--schema DIR` every statement is also checked against your schema by `cqlc-core` (unknown tables and columns, type
mismatches, missing partition key restrictions, ...). Those findings keep cqlc's `CQL1xx` and `CQL2xx` codes.

Suppress a finding with `-- cqlint: ignore CQLINT002` (or the rule name) on the line or above the statement, and a whole file
with `-- cqlint: ignore-file`.

## Configuration (`cqlint.toml`)

```toml
line-width = 88
indent = 4
select = ["CQLINT010"]      # on top of the defaults; "ALL" enables everything
ignore = ["select-star"]
exclude = ["generated"]
in-list-limit = 20
batch-limit = 10
wide-table-columns = 64
schema = "schema"           # enables type-aware checks
```

## In CI

```yaml
- run: cargo install cqlint
- run: cqlint format --check .
- run: cqlint check --output-format github .
```

`--output-format github` creates inline annotations on the pull request; `json` is for tooling. Exit code 1 means findings
(notes only with `--strict`), 2 means an error.

## Speed

Linting 3,600 files (23 MB, 79,000 statements) takes 0.24 s and checking the formatting 0.53 s on an Apple M3 (8 cores).

Apache-2.0
