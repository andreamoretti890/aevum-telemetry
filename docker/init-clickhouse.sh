#!/bin/sh
set -eu

clickhouse-client --user "$CLICKHOUSE_USER" --password "$CLICKHOUSE_PASSWORD" \
    --database "$CLICKHOUSE_DB" --multiquery <<'SQL'
CREATE TABLE IF NOT EXISTS logs (
    timestamp DateTime64(3, 'UTC'),
    level Enum8('trace' = 0, 'debug' = 1, 'info' = 2, 'warn' = 3, 'error' = 4),
    service String,
    message String,
    attributes String
) ENGINE = MergeTree ORDER BY timestamp;
SQL
