# AGENTS.md

This file provides guidance to Claude Code (claude.ai/code) and other coding agents when working with code in this repository.

## What this is

`strict-tracing` is a hard fork of [`tokio-rs/tracing`](https://github.com/tokio-rs/tracing) — the `tracing` framework for structured, application-level diagnostics in Rust — maintained under the `strict-rs` org. It is a Cargo workspace of 15 published crates plus an `examples` member. The repo's `README.md` and `CONTRIBUTING.md` document the *upstream* project; this file records the conventions specific to this fork and supersedes them where they conflict.

## Workspace architecture

The crates form a strict dependency layering — read it bottom-up, because almost every change has to respect this stack:

- **`tracing-core`** — the foundation. Defines the primitives every other crate builds on: the `Subscriber` trait, `Dispatch` (the active subscriber for a thread/the process), `Metadata`/`Callsite` (static per-instrumentation-site data), `Field`/`ValueSet`, span `Id`, and `Event`. It is `no_std`-compatible and intentionally the most stable crate (external subscriber authors depend on it directly). Depends on no other workspace crate.
- **`tracing`** — the instrumentation API that applications and libraries call: the `info!`/`span!`/`event!` macros, the `Span` type, and the `Instrument`/`WithSubscriber` future combinators. Re-exports `#[instrument]` from `tracing-attributes` behind the `attributes` feature (on by default — `default = ["std", "attributes"]`). Depends only on `tracing-core` (+ optional `tracing-attributes`).
- **`tracing-attributes`** — the proc-macro crate implementing `#[instrument]`. Its surface is asserted with compile-fail/UI tests (see Testing).
- **`tracing-subscriber`** — the consumer-facing crate where telemetry pipelines are assembled. Provides `Registry` (a `Subscriber` that stores per-span data) and the composable `Layer` trait stacked onto it with `.with(...)`, plus the `fmt`, `EnvFilter`, and `json` building blocks. It has by far the most feature flags and surface area.
- **Compat / output utilities**, each a thin bridge over the layers above: `tracing-log` (interop with the `log` crate), `tracing-serde` (serialize trace data via `serde`), `tracing-futures` & `tracing-tower` (instrument `futures` / `tower` services), `tracing-appender` (non-blocking + rolling file writers), `tracing-error` (`SpanTrace` capture), `tracing-flame` (flamegraph layer), `tracing-journald` (systemd-journal layer), `tracing-macros` (experimental).
- **Test-only crates**: `tracing-mock` (a mock `Subscriber`/`Layer` plus `expect::{...}` matchers, used as a dev-dependency across the workspace to assert exactly which spans/events/fields were emitted) and `tracing-test`.

Mental model: instrumentation in `tracing` emits to the current `Dispatch`; a `Subscriber` — usually `tracing-subscriber`'s `Registry` with a stack of `Layer`s — collects it. When changing behavior, the operative questions are "which layer/subscriber observes this?" and "does `tracing-core`'s contract still hold for external implementors?"

Each member crate has its own `AGENTS.md` with crate-specific architecture, features, testing, and gotchas — consult it (and prefer it) when working inside that crate; this root file owns only the workspace-wide conventions below.

## Build, test, lint

The source of truth for commands is `.github/workflows/CI.yml`. The workspace uses **nextest** for tests and **cargo-hack** for feature-combination checks; install both (`cargo install cargo-nextest cargo-hack`) if missing.

```bash
cargo check --all --tests --benches                              # fast compile gate (CI's first job)
cargo +nightly fmt --all                                         # format — edition 2024 rules need nightly
cargo fmt --all -- --check                                       # CI's format gate
cargo clippy --all --examples --tests --benches -- -D warnings   # lint — warnings are hard errors
cargo nextest run --profile ci --workspace                       # unit + integration tests
cargo test --doc --workspace                                     # doctests (nextest cannot run these)
```

Running a focused test:

```bash
cargo nextest run -p tracing-subscriber <substring>             # filter by test-name substring
cargo nextest run -p tracing-subscriber -E 'test(env_filter)'  # nextest filter expression
cargo test -p tracing-attributes --test instrument             # a single integration-test file (plain cargo)
cargo test --doc -p tracing-subscriber <substring>             # a single doctest
```

Two test crates are **not workspace members** and must be run by entering their directories (CI does exactly this):

```bash
(cd tracing/test_static_max_level_features && cargo test)      # compile-time max-level feature gating
(cd tracing/test-log-support && cargo test)                    # log-crate interop
```

`no_std` / feature-edge steps that CI runs separately:

```bash
cargo test --no-default-features -p tracing-core               # no-std build
cargo test --no-default-features -p tracing
cargo nextest run --profile ci --all-features -p tracing-subscriber
```

Linting is part of the workspace contract, not a per-crate afterthought. Every member manifest inherits `[lints] workspace = true`; the root `Cargo.toml` carries the rustc/rustdoc/Clippy levels, and `clippy.toml` carries thresholds plus disallowed macros, methods, and types. Prefer structural fixes over new `#[allow]`s. When a legacy compatibility hook still needs an allow, keep it local, include a `reason = "..."`, and preserve the existing `TODO(unsafe-forbid)` breadcrumbs instead of broadening the exception.

## Feature-flag conventions

Features are the trickiest part of editing this workspace — internalize these before touching any `Cargo.toml`:

- **`[workspace.dependencies]` owns the "features-off" baseline.** Many entries set `default-features = false`; a member that needs the defaults opts back in locally with `default-features = true`. This is deliberate: Cargo features are *additive*, so once any member turns a default on the whole graph gets it — only the workspace can enforce "off" centrally. Do not "fix" a missing default by flipping the workspace entry; add `default-features = true` (or the specific feature) at the member that actually needs it.
- **Feature combinations are checked exhaustively** via `cargo hack check --feature-powerset --no-dev-deps`, per crate. `tracing` and `tracing-subscriber` have too many features for a full powerset, so CI excludes the `max_level_*` / `release_max_level_*` set for `tracing` and includes only `fmt ansi json registry env-filter` for `tracing-subscriber`. Any feature you add must still build in combination with the others.
- **Unstable APIs are gated behind `--cfg tracing_unstable`**, not a Cargo feature (e.g. `valuable` support). Build or document them with `RUSTFLAGS="--cfg tracing_unstable"`.
- Recognized custom cfgs (declared in `[workspace.lints.rust]`, so they don't trip `unexpected_cfgs`): `tracing_unstable`, `flaky_tests`, `unsound_local_offset`.

## Fork-specific conventions

- **Edition 2024, `rust-version = "1.96"`, `resolver = "3"`** — declared once in the root `[workspace.package]` / `[workspace]` and inherited via `field.workspace = true`. The README's "1.65" and CI's `check-msrv` 1.65.0 matrix predate the migration (edition 2024 itself requires Rust ≥ 1.85); `Cargo.toml`'s `rust-version = 1.96` is authoritative.
- **Dependency versions are pinned centrally** in `[workspace.dependencies]` — both the internal path crates and external deps. Bump a version there, not in member manifests.
- **UI / compile-fail tests use a forked `trybuild`** pulled as a git dependency (`ssh://git@github.com/strict-rs/strict-trybuild.git`, branch `strict`). Editing `tracing-attributes/tests/ui.rs` or any `*.stderr` fixture requires that git dep to resolve.
- **Releases go through `bin/publish <crate> <version>`** (`-d`/`--dry-run` to verify only). It enforces the cargo-hack feature-powerset gate before publishing; see `CONTRIBUTING.md` for the path-dependency release ordering.
- **Docs build is nightly + `--cfg docsrs`**: `RUSTDOCFLAGS="--cfg docsrs" cargo +nightly doc --no-deps` (Netlify additionally sets `--cfg tracing_unstable`).
- **Cargo profiles are explicit in the root manifest**: release uses thin LTO, one codegen unit, `panic = "abort"`, symbol stripping, and overflow checks; dev optimizes third-party deps; test keeps overflow checks on. Do not duplicate those settings in member manifests.
- **`cargo-machete` root metadata exists** with an empty `ignored` list. If a dependency looks unused, prove whether it is feature-gated/generated/test-only before adding it to the ignore list.

## Commit messages

Use Conventional-Commit subjects with a **required scope** and a structured body. This supersedes the upstream per-crate commit style documented in `CONTRIBUTING.md`.

- **Subject:** `type(scope): imperative structural description` — e.g. `refactor(tracing-core): split callsite registration`. The scope is mandatory (a crate name like `tracing-subscriber`, or something broad like `workspace`). Append `!` for breaking changes (`feat(tracing)!: ...`). **Never use `chore`** — pick a descriptive type (`feat`, `fix`, `refactor`, `perf`, `build`, `ci`, `docs`, `test`, `style`, `revert`).
- **Body:** 1–5 sections sized to the change. Each section starts with a **plain-text header line** (no `#`, no bold, no underline), followed by 3–5 imperative bullets describing structural changes (what was introduced, replaced, removed, renamed, rewired). Separate sections with exactly one blank line; don't pad small commits with empty sections.
- Pass the message via HEREDOC to `git commit -m` so blank lines and bullet spacing survive shell quoting.
