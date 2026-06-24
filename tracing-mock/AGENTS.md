# AGENTS.md

`tracing-mock` is a test-only utility crate, used as a dev-dependency across the workspace to assert exactly which spans, events, and fields a piece of instrumented code emits, and in what order. It is currently unreleased (`0.1.0-beta.3`); pin an exact version when depending on it. Workspace-wide build/test/feature/commit conventions live in the root `AGENTS.md`.

## The expectation/assertion model

You build a script of expectations, install it as the active collector while running the code under test, then assert the script was satisfied:

1. Construct expectations with the `expect::{...}` constructors — `event()`, `span()`, `field(name)`/`msg(text)`, `id()`, and the ancestry helpers (`is_contextual_root`, `has_contextual_parent`, `is_explicit_root`, `has_explicit_parent`). Each returns a builder (`ExpectedEvent`, `ExpectedSpan`/`NewSpan`, `ExpectedField`/`ExpectedFields`, `ExpectedId`, `ExpectedAncestry`) refined with `.named(...)`, `.with_fields(...)`, `.with_value(...)`, `.and(...)`, `.with_ancestry(...)`, `.only()`, etc.
2. Feed them in order to a builder via `subscriber::mock()` (or `layer::mock()`): chain `.new_span(...)`, `.enter(...)`, `.event(...)`, `.record(...)`, `.exit(...)`, `.clone_span(...)`, `.drop_span(...)`. Terminate with `.only()` to additionally assert *nothing else* happens.
3. `.run()` yields the collector; `.run_with_handle()` yields `(collector, MockHandle)`. Drive the code under test (`subscriber::mock` -> `tracing::subscriber::with_default`; `layer::mock` -> compose onto a `registry()` and `set_default()`), then call `handle.assert_finished()`, which **panics** if the expected sequence was not consumed exactly.

Internally each step is an `expect::Expect` variant pushed onto a `VecDeque`; the mock pops and matches as `tracing` calls arrive, and `Expect::bad(...)` produces the `[name] expected … but instead …` panic messages.

## Architecture

- `subscriber.rs` — `MockSubscriber` (the builder, generic over a `Fn(&Metadata) -> bool` filter set via `.with_filter`) and `MockHandle`. Implements `tracing_core::Subscriber` directly, so it works anywhere a bare `Subscriber` does — including `no_std`-shaped tests and `tracing-core`'s own suite. This module is always available.
- `layer.rs` — `MockLayer` + `MockLayerBuilder`, the `tracing-subscriber` `Layer` equivalent, for testing a layer *within* a `Registry` stack (add `.on_register_dispatch()` and per-layer `.named(...)`). **Gated on the optional `tracing-subscriber` feature** (`#[cfg(feature = "tracing-subscriber")]` in `lib.rs`); without it only the `MockSubscriber` path exists.
- `expect.rs` — the `expect::{...}` constructors and the private `Expect` enum that unifies them.
- `event.rs`, `span.rs`, `field.rs`, `metadata.rs`, `ancestry.rs` — the matcher types named above plus shared `ExpectedMetadata`/`ExpectedAncestry` matching logic.

## Gotchas

- It's a tool, not instrumentation: depend on it under `[dev-dependencies]` only.
- The `tracing-subscriber` feature is what unlocks `MockLayer`/`layer::mock` — enable it (it turns on `tracing-subscriber/registry`) in any consumer that tests layers rather than whole subscribers.
- All the module-level rustdoc examples are runnable doctests (including `should_panic` ones showing failed assertions), so `cargo test --doc -p tracing-mock` exercises real behavior — keep them accurate when changing the API.
