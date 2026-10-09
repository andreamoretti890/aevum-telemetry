#!/bin/bash
# Target an already-running release server. Payloads and results stay local.
set -euo pipefail
cd "$(dirname "$0")/.."
if [ "$#" -eq 0 ]; then set -- 1 5 10 25 50; fi
seen=' '
for concurrency in "$@"; do
    if ! [[ "$concurrency" =~ ^[1-9][0-9]*$ ]]; then
        echo "Invalid concurrency: $concurrency" >&2
        exit 1
    fi
    case "$seen" in *" $concurrency "*) echo "Duplicate concurrency: $concurrency" >&2; exit 1;; esac
    seen="$seen$concurrency "
done
for tool in oha python3 curl; do
    if ! command -v "$tool" >/dev/null; then
        echo "Missing $tool" >&2
        if [ "$tool" = oha ]; then echo 'Install on macOS: brew install oha' >&2; fi
        exit 1
    fi
done
curl --fail --silent --show-error --max-time 5 http://127.0.0.1:3000/health >/dev/null
curl --fail --silent --show-error --max-time 5 http://127.0.0.1:8123/ping >/dev/null
python3 benchmarks/generate_payload.py
mkdir -p benchmarks/results
results="$(mktemp -d "benchmarks/results/$(date -u +%Y%m%dT%H%M%SZ)-XXXXXX")"
echo "Results: $results"
failed=0
for concurrency in "$@"; do
    for phase in warmup measurement; do
        duration=15s
        if [ "$phase" = warmup ]; then duration=3s; fi
        output="$results/$phase-c$concurrency.json"
        echo "$phase: concurrency=$concurrency, duration=$duration"
        oha --no-tui --no-color --output-format json --http-version 1.1 \
            -w -t 30s -m POST -H 'Content-Type: application/json' \
            -D benchmarks/payloads/batch-1000.json -c "$concurrency" -z "$duration" \
            http://127.0.0.1:3000/v1/logs/batch > "$output"
        python3 - "$output" <<'PY' || failed=1
import json
import sys

with open(sys.argv[1]) as file:
    result = json.load(file)
statuses = result["statusCodeDistribution"]
errors = result["errorDistribution"]
total = sum(statuses.values()) + sum(errors.values())
successful = statuses.get("201", 0)
summary = result["summary"]
latency = lambda seconds: "n/a" if seconds is None else f"{seconds * 1000:.2f} ms"
print(f"Requests: {total} total, {successful} successful, {total - successful} failed")
print(f"Requests/s: {summary['requestsPerSec']:.2f}; events/s: {summary['requestsPerSec'] * 1000:,.0f}")
print("Latency: " + ", ".join(f"{key} {latency(result['latencyPercentiles'].get(key))}" for key in ("p50", "p95", "p99")) + f"; average {latency(summary['average'])}")
print(f"HTTP statuses: {statuses}; transport errors: {errors}")
sys.exit(1 if total != successful else 0)
PY
    done
done
exit "$failed"
