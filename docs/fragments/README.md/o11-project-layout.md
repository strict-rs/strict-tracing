## Project layout

The [`tracing`] crate contains the primary _instrumentation_ API, used for
instrumenting libraries and applications to emit trace data. The [`tracing-core`]
crate contains the _core_ API primitives on which the rest of `tracing` is
instrumented. Authors of trace subscribers may depend on `tracing-core`, which
guarantees a higher level of stability.

Additionally, this repository contains several compatibility and utility
libraries built on top of `tracing`. Some of these crates are in a pre-release
state, and are less stable than the `tracing` and `tracing-core` crates.

The crates included as part of Tracing are:

* [`tracing-futures`]: Utilities for instrumenting `futures`.
  ([crates.io][fut-crates]|[docs][fut-docs])

* [`tracing-macros`]: Experimental macros for emitting trace events (unstable).

* [`tracing-attributes`]: Procedural macro attributes for automatically
    instrumenting functions. ([crates.io][attr-crates]|[docs][attr-docs])

* [`tracing-log`]: Compatibility with the `log` crate (unstable).

* [`tracing-serde`]: A compatibility layer for serializing trace data with
    `serde` (unstable).

* [`tracing-subscriber`]: Subscriber implementations, and utilities for
  implementing and composing `Subscriber`s.
  ([crates.io][sub-crates]|[docs][sub-docs])

* [`tracing-tower`]: Compatibility with the `tower` ecosystem (unstable).

* [`tracing-appender`]: Utilities for outputting tracing data, including a file appender
   and non-blocking writer. ([crates.io][app-crates]|[docs][app-docs])

* [`tracing-error`]: Provides `SpanTrace`, a type for instrumenting errors with
  tracing spans. ([crates.io][err-crates]|[docs][err-docs])

* [`tracing-flame`]: Provides a layer for generating flame graphs based on
  tracing span entry / exit events. ([crates.io][flame-crates]|[docs][flame-docs])

* [`tracing-journald`]: Provides a layer for recording events to the
  Linux `journald` service, preserving structured data. ([crates.io][jour-crates]|[docs][jour-docs])

[`tracing`]: tracing
[`tracing-core`]: tracing-core
[`tracing-futures`]: tracing-futures
[`tracing-macros`]: tracing-macros
[`tracing-attributes`]: tracing-attributes
[`tracing-log`]: tracing-log
[`tracing-serde`]: tracing-serde
[`tracing-subscriber`]: tracing-subscriber
[`tracing-tower`]: tracing-tower
[`tracing-appender`]: tracing-appender
[`tracing-error`]: tracing-error
[`tracing-flame`]: tracing-flame
[`tracing-journald`]: tracing-journald

[fut-crates]: https://crates.io/crates/tracing-futures
[fut-docs]: https://docs.rs/tracing-futures

[attr-crates]: https://crates.io/crates/tracing-attributes
[attr-docs]: https://docs.rs/tracing-attributes

[sub-crates]: https://crates.io/crates/tracing-subscriber
[sub-docs]: https://docs.rs/tracing-subscriber

[otel-crates]: https://crates.io/crates/tracing-opentelemetry
[otel-docs]: https://docs.rs/tracing-opentelemetry
[OpenTelemetry]: https://opentelemetry.io/

[app-crates]: https://crates.io/crates/tracing-appender
[app-docs]: https://docs.rs/tracing-appender

[err-crates]: https://crates.io/crates/tracing-error
[err-docs]: https://docs.rs/tracing-error

[flame-crates]: https://crates.io/crates/tracing-flame
[flame-docs]: https://docs.rs/tracing-flame

[jour-crates]: https://crates.io/crates/tracing-journald
[jour-docs]: https://docs.rs/tracing-journald
