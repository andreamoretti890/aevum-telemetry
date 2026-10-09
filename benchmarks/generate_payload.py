#!/usr/bin/env python3
"""Generate identical JSON fixtures on every run, without external packages."""

import json
from datetime import datetime, timedelta, timezone
from pathlib import Path


def generate():
    directory = Path(__file__).resolve().parent / "payloads"
    directory.mkdir(exist_ok=True)
    start = datetime(2026, 10, 9, 12, tzinfo=timezone.utc)
    events = []
    for index in range(10_000):
        events.append({
            "timestamp": (start + timedelta(milliseconds=index))
                .isoformat(timespec="milliseconds").replace("+00:00", "Z"),
            "level": "info",
            "service": "performance-test",
            "message": f"Performance test event {index}",
            "attributes": {
                "index": index,
                "shop_id": 42 + index % 8,
                "connected": index % 20 != 0,
                "response_time_ms": 12.5 + (index % 100) / 10,
                "operation": "checkout",
                "terminal_id": f"pos-{index % 16:02d}",
            },
        })
    for count in (100, 1000, 10_000):
        path = directory / f"batch-{count}.json"
        path.write_text(json.dumps(events[:count], separators=(",", ":")) + "\n")
        print(f"Generated {path.name}: {count} events, {path.stat().st_size} bytes")


if __name__ == "__main__":
    generate()
