# AGENTS.md

`tracing-error` enriches Rust errors with `tracing` span context: it captures the active span scope as a `SpanTrace` (a backtrace-like value) and can bundle it onto existing errors. Workspace-wide build/test/feature/commit conventions live in the root `AGENTS.md`.

## Architecture

- `backtrace.rs` — `SpanTrace` wraps a single `tracing::Span` (the current span at `SpanTrace::capture()`); it is cheap to capture and formats lazily. `with_spans()` walks the captured scope by downcasting the active subscriber to the crate-private `WithContext` marker and invoking it. `SpanTraceStatus` (`UNSUPPORTED`/`EMPTY`/`CAPTURED`) reports whether capture actually worked. `Display` renders panic-style frames; `Debug` renders a list.
- `layer.rs` — `ErrorLayer<S, F = DefaultFields>` is the `tracing_subscriber::Layer` that makes capture work: `on_new_span` formats each span's fields into a `FormattedFields<F>` extension, and `downcast_raw` exposes a `WithContext` function pointer that "remembers" `S`/`F` so `SpanTrace` can read field strings back without knowing those types. Requires `S: Subscriber + LookupSpan`.
- `error.rs` (feature `traced-error`) — `TracedError<E>` bundles a `SpanTrace` with an inner error via a `#[repr(C)]` `ErrorImpl<E>` + manual vtable for type erasure (so it can be downcast from `dyn Error`). Extension traits: `InstrumentError`/`InstrumentResult` add `in_current_span()` to wrap an error/`Result`; `ExtractSpanTrace` adds `span_trace()` to pull a `&SpanTrace` back out of a `dyn Error`. Re-exported through the `prelude` module (also gated on `traced-error`).

## Features

- `traced-error` (default) — gates the entire `error` module: `TracedError`, the `InstrumentError`/`InstrumentResult`/`ExtractSpanTrace` traits, and `prelude`. With it off, only `SpanTrace`/`SpanTraceStatus`/`ErrorLayer` remain.

## Testing

- Tests are inline (`#[cfg(test)] mod tests` in `backtrace.rs`); no `tests/` dir. Run `cargo nextest run -p tracing-error`; doctests with `cargo test --doc -p tracing-error`.
- Several doctests in `lib.rs`/`error.rs` are `compile_fail` illustrations — they are not meant to compile.

## Gotchas

- `SpanTrace::capture()` only records anything if an `ErrorLayer` is installed in the active subscriber; otherwise `status()` is `UNSUPPORTED` (no layer / wrong tracing-error version) or `EMPTY` (no current span). Capturing without the layer silently yields an empty trace.
- `error.rs` relies on `unsafe` type erasure: the `#[repr(C)]` layout of `ErrorImpl<E>` and the vtable's `object_ref` are load-bearing — the erased `error` field must only ever be accessed through the vtable. Don't reorder fields or drop `#[repr(C)]`.
