# AGENTS.md

`tracing-tower` instruments `tower` services with per-request and per-service spans. **Experimental and unreleased** — version `0.1.0`, never published to crates.io, and still no `tests/` directory. Its public API now carries docs and inherits the workspace lint policy, but the API remains unstable. Workspace-wide build/test/feature/commit conventions live in the root `AGENTS.md`.

## Architecture

- `lib.rs` defines the entry-point traits. `InstrumentableService` (blanket-impl'd for every `Service<R>`) composes `trace_requests` + `trace_service` into `InstrumentedService<S, R>` (a `service_span::Service` wrapping a `request_span::Service`). `GetSpan<T>` (sealed through the private `sealed::Sealed`) abstracts "how to produce a span for `T`" and is implemented for both `Fn(&T) -> Span` closures and a plain `Span` (cloned per call).
- `request_span.rs` — `Service<S, R, G>` opens a fresh span per request and attaches it to the inner service's response future via `tracing::Instrument` (`Future = tracing::instrument::Instrumented<S::Future>`). Includes a `tower-layer` `Layer` and, under `tower-make`, a `MakeService`/`MakeLayer`/`MakeFuture` set.
- `service_span.rs` — `Service<S>` holds one span entered around `poll_ready`/`call` (does not wrap the future). Same `Layer` + `make` sub-module structure.
- `http.rs` (`http` feature) — convenience `*_request` span constructors for `http::Request` at each level (e.g. `trace_request` records method/uri/version/headers).

## Features

- `tower-layer` (default) — compiles the `Layer`/`MakeLayer` impls in both span modules (enables the `tower-layer` dep).
- `tower-make` (default) — compiles the `MakeService` machinery (enables `tower_make` + `pin-project-lite`).
- `http` — enables the `http` module and its `http`-crate dependency (off by default).

## Testing

- No `tests/` directory exists. `cargo check -p tracing-tower --all-features` (CI's check job covers it) is the only gate; `--all-features` is required to compile `http.rs`.
- Because there are no behavior tests, do not treat docs-only compilation as proof of runtime behavior when changing service/make-service polling. Add focused tests before making semantic changes.
