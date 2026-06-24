# AGENTS.md

`tracing-futures` instruments `futures` / async types (futures, streams, sinks, executors) with `tracing` spans and subscribers. Workspace-wide build/test/feature/commit conventions live in the root `AGENTS.md`.

## Architecture

- `lib.rs` defines the two blanket extension traits and their wrapper types. `Instrument` (implemented for all `Sized` types) wraps a value in `Instrumented<T>`, which enters the attached `Span` on every poll/drop; `WithSubscriber` (gated on `std`) wraps in `WithDispatch<T>`, which sets a `Dispatch` as the thread-default while the inner value is polled. The actual `Future`/`Stream`/`Sink` impls for these wrappers are conditional on the integration features below.
- `Instrumented<T>` stores `inner: Option<T>` in both representations. Under `std-future`, the field is pinned and `PinnedDrop` uses safe `Pin::set(None)` while the span is entered; without `std-future`, `Drop` takes the `Option` while the span is entered. Accessors and consuming APIs return `Option` to reflect that state honestly.
- `executor/` instruments task spawners. `futures_01.rs` impls futures 0.1's `Executor`; its `tokio_executor` submodule (`tokio-executor` feature, implied by `tokio`) impls `tokio_executor::Executor`/`TypedExecutor` for the wrappers, and its `tokio_runtime` submodule (`tokio` only) adds tokio 0.1 `Runtime`/`current_thread` conveniences. `futures_03.rs` impls futures 0.3's `Spawn`/`LocalSpawn` from `futures-task`. `mod.rs` just `cfg`-gates these.
- `stdlib.rs` re-exports `std::*` or `core`/`alloc` (as `crate::stdlib::...`) so the crate can build `no_std` when `std` is off.

## Features

- `std-future` (default) — `std::future::Future` integration; pulls `pin-project-lite` and switches `Instrumented` to the pinned `Option<T>` representation.
- `std` (default) — depends on `std` (via `tracing/std`); required for `WithSubscriber`/`WithDispatch` and gates the `futures-01`/`futures-03` features (both imply `std`).
- `futures-01` — futures 0.1.x compat (`Future`/`Stream`/`Sink`/`Executor`).
- `futures-03` — futures 0.3.x `Spawn`/`LocalSpawn` + `Stream`/`Sink` (implies `std-future`).
- `tokio` — tokio 0.1 executor **and** runtime compat (enables `tokio_01`); a strict superset of `tokio-executor`, which it implies. *Not* needed for tokio 1.x.
- `tokio-executor` — tokio 0.1 executor-trait compat only (`Executor`/`TypedExecutor` impls for the wrappers, via the standalone `tokio-executor` crate); for crates depending on `tokio-executor` directly. Implied by `tokio`.

## Testing & benches

- `cargo nextest run -p tracing-futures` runs the inline `#[cfg(test)]` modules plus the one integration test, `tests/std_future.rs`. Most tests are feature-gated: the futures 0.1 / 0.3 unit-test modules need `--features futures-01` / `--features futures-03`, and `std_future.rs` needs `futures-03` (it uses `tracing_test::{PollN, block_on_future}` and `tracing_mock`).
- CI's `cargo minimal-versions check --feature-powerset` step excludes `futures-01 futures_01 tokio tokio_01 tokio-executor` for this crate (those legacy deps don't satisfy minimal-versions), so a full powerset check locally will hit combinations CI deliberately skips.
- The `std-future` path now imports `Future`/`Poll` through `stdlib`; keep `no_std` compatibility by routing std/core/alloc references through `stdlib.rs` instead of sprinkling direct `std::...` paths.
