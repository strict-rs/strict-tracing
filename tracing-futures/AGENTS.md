# AGENTS.md

`tracing-futures` instruments `futures` / async types (futures, streams, sinks, executors) with `tracing` spans and subscribers. Workspace-wide build/test/feature/commit conventions live in the root `AGENTS.md`.

## Architecture

- `lib.rs` defines the two blanket extension traits and their wrapper types. `Instrument` (implemented for all `Sized` types) wraps a value in `Instrumented<T>`, which enters the attached `Span` on every poll/drop; `WithSubscriber` (gated on `std`) wraps in `WithDispatch<T>`, which sets a `Dispatch` as the thread-default while the inner value is polled. The actual `Future`/`Stream`/`Sink` impls for these wrappers are conditional on the integration features below.
- `Instrumented<T>` stores `inner: Option<T>` in both representations. Under `std-future`, the field is pinned and `PinnedDrop` uses safe `Pin::set(None)` while the span is entered; without `std-future`, `Drop` takes the `Option` while the span is entered. Accessors and consuming APIs return `Option` to reflect that state honestly.
- `executor/` instruments task spawners: `futures_03.rs` impls futures 0.3's `Spawn`/`LocalSpawn` from `futures-task` for both `Instrumented` and `WithDispatch`. The gate file `executor.rs` just `cfg`-gates that submodule behind the `futures-03` feature.
- `lib.rs` imports `Future`/`Pin`/`Context`/`Poll` from `core` directly and builds `no_std` when `std` is off (`#![cfg_attr(not(feature = "std"), no_std)]`); keep `no_std` compatibility by routing code that is not `std`-gated through `core`/`alloc` paths rather than direct `std::...` references.

## Features

- `std-future` (default) — `std::future::Future` integration; pulls `pin-project-lite` and switches `Instrumented` to the pinned `Option<T>` representation.
- `std` (default) — depends on `std` (via `tracing/std`); required for `WithSubscriber`/`WithDispatch` and implied by `futures-03`.
- `futures-03` — futures 0.3.x `Spawn`/`LocalSpawn` + `Stream`/`Sink` (implies `std-future` and `std`).

## Testing

- `cargo nextest run -p tracing-futures` runs the inline `#[cfg(test)]` module plus the one integration test, `tests/std_future.rs`. The inline `futures_03_tests` module is feature-gated and needs `--features futures-03`; `std_future.rs` has no feature gate — it exercises `tracing::Instrument` on std futures with `tracing_test::{PollN, block_on_future}` and `tracing_mock`.
