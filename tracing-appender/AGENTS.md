# AGENTS.md

`tracing-appender` provides non-blocking and rolling file writers for `tracing`, plus the `WorkerGuard` that flushes them on shutdown. Workspace-wide build/test/feature/commit conventions live in the root `AGENTS.md`.

## Architecture

- `non_blocking.rs` — `NonBlocking` is a `Clone` writer that implements both `std::io::Write` and `tracing_subscriber`'s `MakeWriter`; each write ships a `Msg::Line(Vec<u8>)` over a bounded `crossbeam-channel` to an off-thread `Worker`. `NonBlockingBuilder` configures `buffered_lines_limit` (default `DEFAULT_BUFFERED_LINES_LIMIT` = 128_000), `lossy` (default `true`), and `thread_name`. `ErrorCounter` (an `Arc<AtomicUsize>`, exposed via `dropped_lines()`) counts drops; it only increments in lossy mode. The crate-level `non_blocking()` fn is the convenience constructor returning `(NonBlocking, WorkerGuard)`.
- `worker.rs` — `Worker<T: Write>` owns the receiver and the real writer; `work()` blocks on the first `recv()`, then drains via `try_recv()` and flushes per batch. `WorkerState` (`Empty`/`Disconnected`/`Continue`/`Shutdown`) drives the loop in `worker_thread()`.
- `rolling.rs` / `rolling/builder.rs` — `RollingFileAppender` (blocking `Write` + `MakeWriter`) rotates per `Rotation` (`MINUTELY`/`HOURLY`/`DAILY`/`WEEKLY`/`NEVER`, all UTC). The free fns `minutely`/`hourly`/`daily`/`weekly`/`never` wrap `new()`; `Builder` adds `filename_prefix`/`filename_suffix`/`max_log_files`/`latest_symlink` and its `build()` returns `Result<_, InitError>` instead of panicking. Rollover timing is a lock-free `AtomicUsize` (`next_date` as a unix timestamp; `0` means never-rotate) advanced via `compare_exchange`. Date formats are parsed with `time::format_description::parse_borrowed::<1>`, so the manifest must keep `time`'s `formatting` and `parsing` features.
- `sync.rs` — internal `RwLock` wrapper around the `File`, swappable for `parking_lot` (see Features).

## Features

- `parking_lot` (optional, off by default) — swaps `std::sync::RwLock` for `parking_lot::RwLock` in `sync.rs`. The `std` wrapper deliberately ignores lock poisoning (`unwrap_or_else(PoisonError::into_inner)`) to match `parking_lot`'s panic-free API.

## Testing & benches

- Unit tests live inline (`#[cfg(test)] mod test` in `non_blocking.rs` and `rolling.rs`); there is no `tests/` dir. Run `cargo nextest run -p tracing-appender`.
- `non_blocking::test::logs_dropped_if_lossy` is `#[ignore]`d as flaky (timing-dependent); `test_max_log_files` / `test_latest_symlink` `sleep` to force distinct file-creation timestamps, so they are inherently slow.
- One Criterion bench (`benches/bench.rs`, `harness = false`) comparing synchronous vs non-blocking throughput against a no-op writer: `cargo bench -p tracing-appender`.

## Gotchas

- The `WorkerGuard` MUST be bound (never `let _ = ...`, which drops it immediately) and held for the program's lifetime — its `Drop` sends `Msg::Shutdown` and waits up to ~1s for the worker to flush. If dropped early or never held, buffered lines are silently lost on exit/panic. `#[must_use]` warns about discarding it but cannot catch a `_` binding.
- In lossy mode (the default) writes that find a full channel are dropped and counted, never block; set `.lossy(false)` to exert backpressure (blocks the writing thread) instead.
- `RollingFileAppender::new()` and the free constructors panic on init failure (bad path, non-UTF-8 prefix); use `Builder::build()` when you need to handle that gracefully.
- `max_log_files(n)` can retain as few as `n-1` files (it prunes before opening the next), so request `m+1` to guarantee `m`; passing `0` disables pruning entirely.
- This crate still has panic/assert-heavy tests, but production changes inherit workspace lints. Preserve the existing named `_guard`/`_send_result` style for intentionally ignored guard and channel outcomes.
