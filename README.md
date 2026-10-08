# Aevum

Aevum (`aevum-telemetry`) is a telemetry and observability platform written in Rust.
It provides an HTTP API for storing and querying logs in ClickHouse.

## Current state

The HTTP API accepts a log at `POST /v1/logs`, an array of logs at
`POST /v1/logs/batch`, and reads them from ClickHouse at `GET /v1/logs`.
Reads support `service`, `level`, `limit`, `from`, and `to` query parameters.
Time bounds are inclusive. Results are newest first, with 100 results by default
and limits above 1000 clamped to 1000. `/health` reports that the HTTP server is running.

## API errors and validation

Error responses use the same JSON envelope, including JSON/query parsing errors.

```json
{
  "error": {
    "code": "invalid_request",
    "message": "service must not be blank",
    "request_id": "a95f7c76-3bf3-49f7-9fb7-97f3131082c0",
    "field": "service",
    "event_index": 1
  }
}
```

Each error response receives a server-generated request ID matching its structured
log entry on stderr. Internal failures retain their full cause chain in server
logs; clients receive a safe message and a stable code. Application validation
errors include `field` and a zero-based `event_index` when applicable. Size errors
include `limit`, in bytes for body limits and events for batch limits.

- `storage_error`: HTTP 500 when ClickHouse reads or writes fail.
- `stored_log_invalid`: HTTP 500 when a stored timestamp, level, or attributes
  cannot be decoded. The whole GET fails instead of returning partial results.
- `invalid_request`: HTTP 400 for reversed time bounds, empty batches, or blank
  service names. Empty messages are allowed. Parsing errors retain Axum's statuses:
  400 for malformed JSON/query parameters, 422 for invalid JSON field values or
  missing required fields, and 415 for missing/unsupported JSON content types.
- `batch_too_large`: HTTP 413 above 1000 events.
- `payload_too_large`: HTTP 413 above 10 MiB, on both POST endpoints.
- `not_found` and `method_not_allowed`: HTTP 404 and 405 respectively.

Batch validation stops at the first invalid event, before any insert starts.
A storage failure does not guarantee that no rows were written; retrying a failed
write can produce duplicates.

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
    message String,
    attributes String
) ENGINE = MergeTree ORDER BY timestamp;
```

`cargo run` loads credentials from `.env` and serves the API at
`http://127.0.0.1:3000`. Logs remain in ClickHouse when Aevum restarts.

Setup follows the [official ClickHouse Docker guide](https://clickhouse.com/docs/get-started/setup/self-managed/docker).

## Roadmap

See [ROADMAP.md](ROADMAP.md) for the planned milestones.

## License

[MIT](LICENSE).
