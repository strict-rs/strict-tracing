# AGENTS.md

`tracing` is the instrumentation API that libraries and applications call to emit structured spans and events; it depends only on `tracing-core` (+ optional `tracing-attributes`). Workspace-wide build/test/feature/commit conventions live in the root `AGENTS.md`.

## Architecture

- `macros.rs` (the bulk of the crate, ~3k lines) defines the user-facing macros: `event!`/`span!` and their level shorthands (`trace!`..`error!`, `trace_span!`..`error_span!`). They expand to a static `DefaultCallsite` + `Metadata` per call site and consult `Interest`/`STATIC_MAX_LEVEL` before constructing anything, so disabled instrumentation costs nothing.
- `span.rs` defines `Span` (the central handle) plus its RAII guards: `Entered<'a>` (borrowed, from `enter()`), `EnteredSpan` (owned, from `entered()`), and the `in_scope()` closure form. A `Span` holds an optional `Id` + `&'static Metadata`; entering/exiting drives the active `Dispatch`.
- `instrument.rs` defines the future combinators: the `Instrument` trait (attach a `Span` to a `Future` → `Instrumented<T>`, entered on every poll/drop) and `WithSubscriber` (attach a `Dispatch` → `WithDispatch<T>`). Both use `pin-project-lite`.
- Thin re-export modules over `tracing-core`: `dispatcher` (`Dispatch`, `set_default`/`with_default`/`set_global_default`), `field` (`Value`, `Empty`, `field::debug`/`display`), `subscriber` (`Subscriber`, `set_default`/`with_default` taking an owned subscriber), `level_filters`, `event`.
- `level_filters.rs` computes `STATIC_MAX_LEVEL` at compile time from the `max_level_*` / `release_max_level_*` features (release set wins in non-debug builds; most-permissive enabled feature wins since features are additive).
- `__macro_support` and `log` are `#[doc(hidden)]` private APIs invoked only by the macros — not semver-stable despite being `pub`.

## Features

- `default = ["std", "attributes"]`. `attributes` re-exports `#[instrument]` from `tracing-attributes` (pulls in `syn`); `std` enables `tracing-core/std` and the closure-based `subscriber::with_default`/`set_default` (`no_std` still requires `liballoc`).
- `log` emits `log` records when no subscriber is set; `log-always` emits them even when one is (implies `log`). Both gate the private `log`/`__macro_support::__tracing_log` paths.
- `max_level_*` / `release_max_level_*` and `valuable` (→ `tracing-core/valuable`, also needs `--cfg tracing_unstable`) per the root feature notes. `async-await` is a no-op kept for compatibility.

## Testing & benches

- `cargo nextest run -p tracing`; doctests are heavy here — `cargo test --doc -p tracing`.
- `tests/macros.rs` (~70k) and `tests/span.rs`/`tests/event.rs` assert exact emitted output via `tracing-mock`. `tests/multiple_max_level_hints.rs` and `tests/max_level_hint.rs` cover level-hint propagation.
- Two **non-workspace** test crates must be entered to run (each has its own `[workspace]`); they enable filtering features that would otherwise leak into the whole graph:
  ```bash
  (cd tracing/test_static_max_level_features && cargo test)   # built with max_level_debug + release_max_level_info
  (cd tracing/test-log-support && cargo test)                 # built with log + log-always
  ```
- Benches (`harness = false`, criterion) are the perf surface: `dispatch_get_clone`, `dispatch_get_ref`, `empty_span`, `enter_span`, `event`, `span_fields`, `span_no_fields`, `span_repeated`, plus `baseline`. `benches/shared.rs` is shared support, not a `[[bench]]`.

## Gotchas

- `#![no_std]` crate; `std` only adds the `extern crate std` paths. Keep new code `core`/`alloc`-only unless behind `#[cfg(feature = "std")]`.
- The `__macro_support::FieldName` const fn uses `unsafe { str::from_utf8_unchecked }` to strip `r#` from raw-identifier field names at compile time — its safety rests on the private field having been built by `FieldName::new`.
