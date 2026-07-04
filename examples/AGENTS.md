# AGENTS.md

Scope: `examples/` — the `tracing-examples` workspace member (`version = "0.0.0"`, `publish = false`): a gallery of runnable demos for the whole tracing ecosystem. Workspace-wide build/test/lint, feature conventions, and commit rules live in the root `AGENTS.md`; this member exists only so the examples compile against the in-tree crates.

## Layout

- Every tracing crate the demos exercise is declared as a **dev-dependency**, pinned through `[workspace.dependencies]`, with per-example needs opted in locally (`tracing-subscriber` with `json` + `env-filter`, `tracing-futures` with `futures-01`, `tokio` with `full`, plus `serde_json` and `futures`).
- Example sources live under `examples/` inside this member: ~40 single-file targets plus the directory-based `sloggish/` (`main.rs` + `sloggish_subscriber.rs`). `fmt/yak_shave.rs` is not an example target — it is a shared support module that the `fmt*` examples import via `#[path = "fmt/yak_shave.rs"]`.
- `README.md` in this directory catalogs each example, grouped by the crate it demonstrates — keep it in sync when adding or renaming an example.

## Running

```sh
cargo run -p tracing-examples --example fmt        # or any target name
```

- The `valuable.rs` / `valuable_instrument.rs` / `valuable_json.rs` examples demonstrate the unstable `valuable` support and must be built with `RUSTFLAGS="--cfg tracing_unstable"` (their doc headers say so; see the root guide's feature-flag section).
- Nothing here is asserted by the test suite, but CI compiles and lints every example: the workspace matrix builds `--all-targets`, and `cargo clippy --all --examples --tests --benches -- -D warnings` must stay clean — an example that stops compiling or picks up a warning is a CI failure.
