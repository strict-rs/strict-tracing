# AGENTS.md

`tracing-macros` is an **experimental, unreleased** crate (`version = 0.1.0`, `maintenance = experimental`) providing two convenience macros on top of `tracing`. Workspace-wide build/test/feature/commit conventions live in the root `AGENTS.md`.

## Architecture

- `src/lib.rs` is the entire crate (~51 lines) and now has crate-level docs so it satisfies the workspace `missing_docs` policy. It re-exports `tracing` (as a `#[doc(hidden)]` `pub use`) and defines two `#[macro_export]` macros:
  - `dbg!` — a `tracing`-flavored analogue of `std::dbg!`: evaluates an expression, emits it as a field via `tracing::event!` (default level `DEBUG`, overridable with `level:` / `target:`), and returns the value.
  - `trace_dbg!` — a thin wrapper that forwards to `dbg!`.

## Gotchas

- No `tests/` directory and no `[[bench]]`/`[[test]]` entries — the only executable coverage is `examples/factorial.rs` (run with `cargo run -p tracing-macros --example factorial`). The dev-dependency on `tracing-subscriber` (with `env-filter`) exists for that example.
- Being experimental, the macro API may change or be removed without a semver-meaningful release; don't treat it as stable surface.
