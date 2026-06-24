# AGENTS.md

`tracing-test` is a small (~86 LOC) **internal, `publish = false`** test-support crate — distinct from the unrelated third-party `tracing-test` on crates.io. Per its manifest's "BIG SCARY NOTE" it must never be published; it exists so async-future test helpers can be shared without bloating `tracing-mock`. Workspace-wide build/test/feature/commit conventions live in the root `AGENTS.md`.

## What it provides

All of `src/lib.rs`, two items:

- `PollN<T, E>` — a `Future` (where `T: Unpin, E: Unpin`) that returns `Pending` (re-waking itself) until its Nth poll, then resolves to a stored `Result`. Constructed via `PollN::new_ok(finish_at)` / `PollN::new_err(finish_at)` (both on `PollN<(), ()>`). Used to assert how instrumented futures behave across multiple polls.
- `block_on_future(future)` — drives a future to completion on a `tokio_test::task::spawn` loop (its only dependency is `tokio-test`).

## Consumers

Pulled in only as a dev-dependency. Used by `tracing-attributes` tests (`async_fn`, `err`, `ret`, `follows_from`), `tracing-futures`' `tests/std_future.rs`, and the non-workspace `tracing/test_static_max_level_features`. (Note: `tracing-futures`' own `src/lib.rs` defines a *separate* local `PollN` for its `futures_01` tests — unrelated to this crate.)

## Testing

- `cargo test -p tracing-test` runs the crate's own `PollN::new_ok` / `new_err` smoke tests. It is still only support code, so keep coverage behavior-focused and tiny.

## Gotchas

- Keep it dependency-light and `publish = false`. If a helper is needed by published crates, the manifest note says move it back into `tracing-mock` rather than publishing this crate.
