# Aevum

Aevum V1 is a telemetry backend written in Rust with Tokio and Axum. It stores
structured logs in ClickHouse and exposes single/batch ingestion and filtered
queries over HTTP.

## Quickstart

Prerequisites:

- Git, curl, and a current stable Rust toolchain with Cargo.
- Docker with Compose. Start Docker Desktop on macOS/Windows, or the Docker
  daemon on Linux.
- Free local ports 3000, 8123, and 9000.

On macOS, Rust also needs Apple's Command Line Tools or Xcode, with a compatible
compiler and SDK. See the SDK troubleshooting note below if linking fails.

Clone the repository and create your local configuration:

```sh
git clone https://github.com/andreamoretti890/aevum-telemetry.git
cd aevum-telemetry
cp .env.example .env
```

Set `CLICKHOUSE_PASSWORD` in `.env` to a local password, keeping the quotes.
The example database and user are both `aevum`, and the table is `logs`.
Both Compose and Aevum use this file. It is ignored by Git.

Start ClickHouse and wait for it to become healthy:

```sh
docker compose up -d --wait
```

On a fresh data volume, Compose creates the database, user, and logs table.
No manual SQL is needed. Then start Aevum from the repository root:

```sh
cargo run
```

Aevum loads `.env` and listens at `http://127.0.0.1:3000`. Leave it running
and use another terminal for the examples below. Stop Aevum with Ctrl-C.

## Try the API

Health returns HTTP 200 and `{"status":"ok"}`. It checks that the HTTP server
is running, not whether ClickHouse is reachable.

```sh
curl -i http://127.0.0.1:3000/health
```

Ingest one event:

```sh
curl -i http://127.0.0.1:3000/v1/logs \
  -H 'Content-Type: application/json' --data-binary @- <<'JSON'
{
  "timestamp": "2026-10-12T08:00:00.000Z",
  "level": "info",
  "service": "readme-demo",
  "message": "Single log event",
  "attributes": {"shop_id": 42, "connected": true}
}
JSON
```

Ingest a batch. The body is a JSON array of events:

```sh
curl -i http://127.0.0.1:3000/v1/logs/batch \
  -H 'Content-Type: application/json' --data-binary @- <<'JSON'
[
  {
    "timestamp": "2026-10-12T08:00:01.000Z",
    "level": "info",
    "service": "readme-demo",
    "message": "First batch event",
    "attributes": {"index": 1, "shop_id": 42}
  },
  {
    "timestamp": "2026-10-12T08:00:02.000Z",
    "level": "warn",
    "service": "readme-demo",
    "message": "Second batch event",
    "attributes": {"index": 2, "connected": false}
  }
]
JSON
```

Both ingestion requests return HTTP 201 with an empty response body.

Query the persisted events with all filters together:

```sh
curl -i --get http://127.0.0.1:3000/v1/logs \
  --data-urlencode 'service=readme-demo' \
  --data-urlencode 'level=info' \
  --data-urlencode 'from=2026-10-12T08:00:00Z' \
  --data-urlencode 'to=2026-10-12T08:00:02Z' \
  --data-urlencode 'limit=10'
```

On the first pass through these examples, this returns the two `info` events
newest first. Omit filters to query all services and levels. `from` and `to`
are inclusive RFC3339 time bounds; either can be used alone. The default limit
is 100, values above 1,000 are clamped to 1,000, and zero returns an empty array.

Each event requires `timestamp`, `level`, `service`, `message`, and an
`attributes` JSON object. Levels are `trace`, `debug`, `info`, `warn`,
and `error`. Service names must not be blank; empty messages are allowed.
Batches contain 1–1,000 events. Both POST endpoints accept bodies up to 10 MiB.

## Tests and checks

The default suite needs no running database. It covers models, configuration,
query parsing, error responses, and API behavior with mocked ClickHouse:

```sh
cargo test
cargo clippy --all-targets -- -D warnings
```

If Clippy is missing, install it with `rustup component add clippy`.

To also run the real ClickHouse round-trip and API tests, start the Compose
service as above and export the configuration in your test terminal:

```sh
set -a
. ./.env
set +a
cargo test -- --include-ignored
```

The API integration test creates and removes a temporary database; the other
ClickHouse test creates and removes a temporary table. The configured user
needs permission to create them. These tests check single and batch persistence,
combined filters, inclusive time bounds, ordering, limits, and reads after
recreating the app. They do not reset the application's logs table.

## Local ClickHouse

ClickHouse is pinned to `26.9.11.2`. HTTP is available at
`http://127.0.0.1:8123` and native TCP at `127.0.0.1:9000`; both are bound
to localhost. Data and server logs persist in Docker named volumes.

The [initializer](docker/init-clickhouse.sh) creates `logs` in
`CLICKHOUSE_DB` using the image's
[initialization hook](https://clickhouse.com/docs/get-started/setup/self-managed/docker).
It runs on fresh volumes. If you have an older volume without the table, start
ClickHouse, then apply the same idempotent initializer:

```sh
docker compose exec -T clickhouse sh /docker-entrypoint-initdb.d/01-create-logs.sh
```

The quickstart uses `CLICKHOUSE_TABLE=logs`. If you change that variable,
create the matching table yourself.

Open the bundled SQL client using the container's credentials:

```sh
docker compose exec clickhouse sh -c 'exec clickhouse-client --user "$CLICKHOUSE_USER" --password "$CLICKHOUSE_PASSWORD" --database "$CLICKHOUSE_DB"'
```

Stop ClickHouse with `docker compose stop`; start it again with
`docker compose up -d --wait`. `docker compose down` also preserves the
named volumes. Adding `-v` removes them and deletes their data.

## macOS SDK troubleshooting

If linking reports a malformed `.tbd` file or an unknown architecture, check that
the compiler and SDK come from a compatible developer-tools installation. With
a full Xcode installation selected, use its bundled SDK for the current terminal:

```sh
export SDKROOT="$(xcode-select -p)/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk"
```

Then retry `cargo run` or `cargo test`. This does not change the global Xcode
selection. For a Command Line Tools-only installation, use a matching tools/SDK
version rather than the full-Xcode path above.

## API errors

Errors use a JSON envelope:

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

Each error receives a request ID matching its structured server log entry.
Internal causes stay in server logs; clients receive a safe message and code.
Validation errors include `field` and a zero-based `event_index` when
applicable. Size errors include `limit`, in bytes for bodies and events for
batches.

- `invalid_request`: HTTP 400 for empty batches, blank services, or reversed
  time bounds. Parsing errors retain Axum's statuses: 400 for JSON syntax/query
  errors, 422 for invalid field values or missing required fields, and 415 for
  unsupported/missing JSON content types.
- `batch_too_large`: HTTP 413 above 1,000 events.
- `payload_too_large`: HTTP 413 above 10 MiB.
- `storage_error`: HTTP 500 when ClickHouse reads or writes fail.
- `stored_log_invalid`: HTTP 500 when a stored timestamp, level, or attributes
  cannot be decoded. The whole GET fails rather than returning partial results.
- `not_found` and `method_not_allowed`: HTTP 404 and 405.

Batch validation stops at the first invalid event before insertion starts.
A storage failure can occur after some rows were written; retrying can produce
duplicates.

## Performance baseline

See [benchmarks/README.md](benchmarks/README.md) for the measured local ingestion
baseline and reproduction commands.

## Roadmap

See [ROADMAP.md](ROADMAP.md) for the project's direction.

## License

[MIT](LICENSE).
