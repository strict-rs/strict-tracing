# AGENTS.md

`tracing-mock` is a test-only utility crate, used as a dev-dependency across the workspace to assert exactly which spans, events, and fields a piece of instrumented code emits, and in what order. It is currently unreleased (`0.1.0-beta.3`); pin an exact version when depending on it. Workspace-wide build/test/feature/commit conventions live in the root `AGENTS.md`.

## The expectation model

You build a script of expectations, install it as the active collector while running the code under test, then validate that the script was satisfied:

1. Construct expectations with the `expect::{...}` constructors — `event()`, `span()`, `field(name)`/`msg(text)`, `id()`, and the ancestry helpers (`is_contextual_root`, `has_contextual_parent`, `is_explicit_root`, `has_explicit_parent`). Each returns a builder (`ExpectedEvent`, `ExpectedSpan`/`NewSpan`, `ExpectedField`/`ExpectedFields`, `ExpectedId`, `ExpectedAncestry`) refined with `.named(...)`, `.with_fields(...)`, `.with_value(...)`, `.and(...)`, `.with_ancestry(...)`, `.only()`, etc.
2. Feed them in order to a builder via `subscriber::mock()` (or `layer::mock()`): chain `.new_span(...)`, `.enter(...)`, `.event(...)`, `.record(...)`, `.exit(...)`, `.clone_span(...)`, `.close_span(...)`. Terminate with `.only()` to additionally expect *nothing else* happens. Wrap part of the script in `.expect_when(enabled, build)` — a conditional-expectation combinator on both `MockSubscriber` and `MockLayerBuilder` — to add the expectations produced by `build` only when `enabled` is `true`, returning the builder unchanged otherwise; delivery tests predicate it on `STATIC_MAX_LEVEL.enables(level)` so one expectation script stays valid when a `max_level_*` / `release_max_level_*` feature cap statically compiles a verbose event (and thus its expectation) out.
3. `.run()` yields the collector; `.run_with_handle()` yields `(collector, MockHandle)`. Drive the code under test (`subscriber::mock` -> `tracing::subscriber::with_default`; `layer::mock` -> compose onto a `registry()` and install it with `tracing::subscriber::set_default`). In `Result<(), strict_test_support::TestFailure>` tests, finish positive cases with `strict_test_support::ensure_ok(handle.finished(), "mock expectations should finish")?`; `TestFailure` does not implement `From<tracing_core::subscriber::SubscriberError>`, so do not use `handle.finished()?` directly in those tests.

Internally each step is an `expect::Expect` variant pushed onto a `VecDeque`; the mock pops and matches as `tracing` calls arrive, and `Expect::bad(...)` produces the `[name] expected … but instead …` error messages.

## Architecture

- `subscriber.rs` — `MockSubscriber` (the builder, generic over a `Fn(&Metadata) -> bool` filter set via `.with_filter`) and `MockHandle`. Implements `tracing_core::Subscriber` directly, so it works anywhere a bare `Subscriber` does — including `no_std`-shaped tests and `tracing-core`'s own suite. This module is always available.
- `layer.rs` — `MockLayer` + `MockLayerBuilder`, the `tracing-subscriber` `Layer` equivalent, for testing a layer *within* a `Registry` stack (add `.on_register_dispatch()` and per-layer `.named(...)`). **Gated on the optional `tracing-subscriber` feature** (`#[cfg(feature = "tracing-subscriber")]` in `lib.rs`); without it only the `MockSubscriber` path exists.
- `expect.rs` — the `expect::{...}` constructors and the private `Expect` enum that unifies them.
- `event.rs`, `span.rs`, `field.rs`, `metadata.rs`, `ancestry.rs` — the matcher types named above plus shared `ExpectedMetadata`/`ExpectedAncestry` matching logic.

## Gotchas

- It's a tool, not instrumentation: depend on it under `[dev-dependencies]` only.
- The `tracing-subscriber` feature is what unlocks `MockLayer`/`layer::mock` — enable it (it turns on `tracing-subscriber/registry`) in any consumer that tests layers rather than whole subscribers.
- `tests/event_ancestry.rs`, `tests/span_ancestry.rs`, and `tests/on_register_dispatch.rs` cover failure text, ancestry matching, and dispatch registration hooks in addition to doctests. Negative runtime tests should drive a mismatch, extract the returned `tracing_core::subscriber::SubscriberError` with `strict_test_support::ensure_some(handle.finished().err(), "...")?`, and validate its text with `ensure_contains`; do not use panic-based test attributes for active runtime coverage. `tests/ui.rs` covers the `finished()` completion idiom with `strict_test_support::ensure_compiles` / `ensure_compile_fail`; do not use raw `trybuild`. Run `cargo nextest run -p tracing-mock`; use `--features tracing-subscriber` when changing `MockLayer` behavior.
- All the module-level rustdoc examples are runnable doctests, including negative examples that inspect returned errors, so `cargo test --doc -p tracing-mock` exercises real behavior — keep them accurate when changing the API.
