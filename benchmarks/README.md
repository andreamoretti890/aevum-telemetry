# Local ingestion baseline

Aevum was benchmarked on 2026-10-09 using 1,000-event batches against ClickHouse
in Docker. The release build peaked at about 2.2M events/s at concurrency 200.
At 400, throughput declined, p99 reached 1.2s, and overload errors appeared
during warm-up. These are short-duration local benchmarks, not production
capacity claims.

| Concurrency | Events/s | p50 | p95 | p99 |
| ---: | ---: | ---: | ---: | ---: |
| 100 | 1.77M | 58ms | 84ms | 127ms |
| 200 | 2.20M | 81ms | 148ms | 255ms |
| 400 | 2.02M | 168ms | 311ms | 1198ms |

Each level used a 3-second warm-up and a 15-second measurement, with HTTP/1.1
keep-alive. Measured requests all returned HTTP 201. The warm-up at 400 returned
1,196 HTTP 500 responses out of 6,132 requests, with ClickHouse connection
resets and broken pipes in the application logs. The database gained exactly
112,987,000 rows, matching acknowledged events across the full 50/100/200/400
sweep, including warm-ups and the initial probe.

Environment: Apple M4 Pro, 14 cores, 48 GiB RAM; Docker had 14 CPUs and about
7.75 GiB RAM; ClickHouse 26.9.11.2 and oha 1.16.0. The host already had memory
pressure. Payloads reuse deterministic events and a narrow timestamp range;
the 50-to-100 throughput jump was nonlinear and remains unexplained.

## Reproduce

Requires Python 3, curl, `oha`, and the configured local ClickHouse setup from
the main README. Install a missing `oha` on macOS with `brew install oha`.
Start the existing release binary in another terminal:

```sh
RUST_LOG=warn ./target/release/aevum-telemetry
```

Then run:

```sh
./benchmarks/run.sh 50 100 200 400
```

Without arguments, the runner tests 1/5/10/25/50. It generates fixed payloads,
warms each level, saves raw JSON, and prints counts, throughput, and latency.
Success means HTTP 201; other statuses and transport errors remain visible.
The command exits nonzero if any warm-up or measurement requests fail.
Events/s is requests/s × 1,000, including failed attempts if present, so check
success counts when comparing ingestion throughput.

Payloads and results are ignored by Git. The generator also creates 100- and
10,000-event fixtures; the latter exceeds the current 1,000-event API limit.
The runner writes persistent `performance-test` rows and never resets data.
It leaves the server you started running. Row-count verification and Samply
capture were performed for the recorded baseline; this small runner only tests
HTTP ingestion.
