# Aevum

Aevum (`aevum-telemetry`) is an experimental telemetry and observability platform
written in Rust. It is a personal learning and portfolio project: the goal is to
learn Rust while progressively building a real system.

The long-term direction is to ingest telemetry, process it asynchronously, store
it efficiently, and query it. Development will happen incrementally, with each
step introducing a concrete problem to solve. The architecture will grow with
those needs rather than start at production scale.

## Current state

The repository contains Cargo's starter binary, which prints `Hello, world!`.
There are no telemetry features or external dependencies yet.

## Getting started

Install a current stable Rust toolchain with Cargo and Clippy, then run:

```sh
cargo run
cargo check
cargo test
cargo clippy -- -D warnings
```

There are no tests yet. Tests will accompany behavior as it is implemented.

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

This sets up the database for development. The Rust app still stores events in
memory; connecting it to ClickHouse is a separate implementation step.

Setup follows the [official ClickHouse Docker guide](https://clickhouse.com/docs/get-started/setup/self-managed/docker).

## Learning approach

I write the Rust implementation myself. AI assistance is for explanations, hints,
small next steps, and code review. Each milestone should leave me able to explain
the code and its tradeoffs.

See [ROADMAP.md](ROADMAP.md) for the planned milestones.

## License

[MIT](LICENSE).
