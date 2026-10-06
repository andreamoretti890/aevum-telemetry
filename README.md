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

## Learning approach

I write the Rust implementation myself. AI assistance is for explanations, hints,
small next steps, and code review. Each milestone should leave me able to explain
the code and its tradeoffs.

See [ROADMAP.md](ROADMAP.md) for the planned milestones.

## License

[MIT](LICENSE).
