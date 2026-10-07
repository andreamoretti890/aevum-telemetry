# Aevum

Aevum (`aevum-telemetry`) is an experimental telemetry and observability platform
written in Rust. It is a personal learning and portfolio project: the goal is to
learn Rust while progressively building a real system.

The long-term direction is to ingest telemetry, process it asynchronously, store
it efficiently, and query it. Development will happen incrementally, with each
step introducing a concrete problem to solve. The architecture will grow with
those needs rather than start at production scale.

## Current state

The HTTP API accepts logs at `POST /v1/logs` and reads them from ClickHouse at
`GET /v1/logs`. Reads support `service`, `level`, and `limit` query parameters,
return newest logs first, and default to 100 results with a maximum of 1000.
Storage failures return HTTP 500. `/health` reports that the HTTP server is running.

## Getting started

Install a current stable Rust toolchain with Cargo and Clippy, then run:

```sh
cargo run
cargo check
cargo test
cargo clippy -- -D warnings
```

`cargo test` runs the tests that need no database. To include the ClickHouse
round-trip and API tests, start ClickHouse, then run:

```sh
set -a
source .env
set +a
cargo test -- --include-ignored
```

The API test creates and removes a temporary database, so the test user needs
permission to create databases. It checks storage, filters, ordering, limits,
and reads after recreating the app. The default suite also checks HTTP 500
responses when ClickHouse rejects requests or cannot be reached.

## Local ClickHouse

The Compose setup pins ClickHouse to version `26.9.11.2`. Install and start Docker
Desktop. On first setup, create your local credentials:

```sh
cp .env.example .env
```

Replace `CLICKHOUSE_PASSWORD` in `.env` with your own password. This file is ignored
by Git. Start ClickHouse from the repository root:

```sh
docker compose up -d --wait
```

Connect over HTTP at `http://127.0.0.1:8123`, or use native TCP at
`127.0.0.1:9000`. The database and username are `aevum`; the password is in `.env`.
Open the bundled SQL client with:

```sh
docker compose exec clickhouse sh -c 'exec clickhouse-client --user "$CLICKHOUSE_USER" --password "$CLICKHOUSE_PASSWORD" --database "$CLICKHOUSE_DB"'
```

Use `docker compose stop` to stop the server and `docker compose up -d --wait` to
start it again. Data and server logs live in Docker named volumes and survive
container recreation. `docker compose down` also preserves them; adding `-v`
deletes the database and logs.

Before running the app, create its table in the SQL client:

```sql
CREATE TABLE IF NOT EXISTS logs (
    timestamp DateTime64(3, 'UTC'),
    level Enum8('trace' = 0, 'debug' = 1, 'info' = 2, 'warn' = 3, 'error' = 4),
    service String,
    message String
) ENGINE = MergeTree ORDER BY timestamp;
```

`cargo run` loads credentials from `.env` and serves the API at
`http://127.0.0.1:3000`. Logs remain in ClickHouse when Aevum restarts.

Setup follows the [official ClickHouse Docker guide](https://clickhouse.com/docs/get-started/setup/self-managed/docker).

## Learning approach

I write the Rust implementation myself. AI assistance is for explanations, hints,
small next steps, and code review. Each milestone should leave me able to explain
the code and its tradeoffs.

See [ROADMAP.md](ROADMAP.md) for the planned milestones.

## License

[MIT](LICENSE).
