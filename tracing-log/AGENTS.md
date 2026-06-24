# AGENTS.md

`tracing-log` bridges the `log` crate and `tracing` in both directions. Workspace-wide build/test/feature/commit conventions live in the root `AGENTS.md`.

## Architecture

- `lib.rs` holds the type conversions and the core dispatch path. `AsLog`/`AsTrace` (both sealed via the private `sealed::Sealed` trait) convert `Level`, `LevelFilter`, `Metadata`, and `log::Record` between the two ecosystems. `dispatch_record` (public via `format_trace`) turns a `log::Record` into a `tracing::Event` emitted on the current `Dispatch`, mapping the record's message/target/module/file/line onto a fixed `FIELD_NAMES` field set. Each log level has a dedicated static callsite (`TRACE_CS`..`ERROR_CS`, built by the `log_cs!` macro) so events from logs share a stable `Callsite`/`Metadata`.
- `NormalizeEvent` (impl'd for `Event`) lets subscribers recover full metadata from a log-sourced event: `is_log()` checks the callsite identity, and `normalized_metadata()` reconstructs a borrowed `Metadata` (file/line/module/target) by replaying the event's fields through `LogVisitor`. Note `LogVisitor::record_str` casts field string lifetimes via `unsafe` — it is sound only because the visitor is constructed with the same lifetime as the event it visits.
- `log_tracer.rs` defines `LogTracer` (a `log::Log` impl that forwards records to `tracing`) and its `Builder` (`ignore_crate`/`ignore_all`, `with_max_level`, and — under `interest-cache` — `with_interest_cache`). `init`/`init_with_filter` install it as the global `log` logger.
- `interest_cache.rs` (the `interest-cache` feature) backs `InterestCacheConfig` with a per-thread `ahash`/`lru` `LruCache` keyed on level+target, short-circuiting `LogTracer::enabled` for repeated callsites.

## Features

- `std` (default) — enables `log/std`; required by `interest-cache`.
- `log-tracer` (default) — compiles the `log_tracer` module / `LogTracer` type; the `AsTrace`/`AsLog`/`NormalizeEvent` conversions in `lib.rs` are always available regardless.
- `interest-cache` — pulls `lru` + `ahash` and enables the interest cache; only active when combined with `log-tracer` and `std`.

## Testing & benches

- `cargo nextest run -p tracing-log` runs the inline callsite tests plus integration tests `tests/log_tracer.rs` (asserts normalized metadata round-trips through a custom `Subscriber`) and `tests/reexport_log_crate.rs` (confirms `log` is reachable as `tracing_log::log`).
- `cargo bench -p tracing-log` runs the criterion bench `benches/logging.rs` (`harness = false`).
