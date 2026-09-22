## Related Crates

In addition to this repository, here are also several third-party crates which
are not maintained by the `tokio` project. These include:

- [`tracing-timing`] implements inter-event timing metrics on top of `tracing`.
  It provides a subscriber that records the time elapsed between pairs of
  `tracing` events and generates histograms.
- [`tracing-honeycomb`] Provides a layer that reports traces spanning multiple machines to [honeycomb.io]. Backed by [`tracing-distributed`].
- [`tracing-distributed`] Provides a generic implementation of a layer that reports traces spanning multiple machines to some backend.
- [`tracing-actix-web`] provides `tracing` integration for the `actix-web` web framework.
- [`tracing-actix`] provides `tracing` integration for the `actix` actor
  framework.
- [`axum-insights`] provides `tracing` integration and Application insights export for the `axum` web framework.
- [`tracing-gelf`] implements a subscriber for exporting traces in Graylog
  GELF format.
- [`tracing-coz`] provides integration with the [coz] causal profiler
  (Linux-only).
- [`tracing-bunyan-formatter`] provides a layer implementation that reports events and spans in [bunyan] format, enriched with timing information.
- [`tide-tracing`] provides a [tide] middleware to trace all incoming requests and responses.
- [`color-spantrace`] provides a formatter for rendering span traces in the
  style of `color-backtrace`
- [`color-eyre`] provides customized panic and eyre report handlers for
  `eyre::Report` for capturing span traces and backtraces with new errors and
  pretty printing them.
- [`spandoc`] provides a proc macro for constructing spans from doc comments
  _inside_ of functions.
- [`tracing-wasm`] provides a `Subscriber`/`Layer` implementation that reports
  events and spans via browser `console.log` and [User Timing API (`window.performance`)].
- [`tracing-web`] provides a layer implementation of level-aware logging of events
  to web browsers' `console.*` and span events to the [User Timing API (`window.performance`)].
- [`test-log`] takes care of initializing `tracing` for tests, based on
  environment variables with an `env_logger` compatible syntax.
- [`tracing-unwrap`] provides convenience methods to report failed unwraps on `Result` or `Option` types to a `Subscriber`.
- [`diesel-tracing`] provides integration with [`diesel`] database connections.
- [`tracing-tracy`] provides a way to collect [Tracy] profiles in instrumented
  applications.
- [`tracing-elastic-apm`] provides a layer for reporting traces to [Elastic APM].
- [`tracing-etw`] provides a layer for emitting Windows [ETW] events.
- [`sentry-tracing`] provides a layer for reporting events and traces to [Sentry].
- [`tracing-forest`] provides a subscriber that preserves contextual coherence by
  grouping together logs from the same spans during writing.
- [`tracing-loki`] provides a layer for shipping logs to [Grafana Loki].
- [`tracing-logfmt`] provides a layer that formats events and spans into the logfmt format.
- [`tracing-chrome`] provides a layer that exports trace data that can be viewed in `chrome://tracing`.
- [`reqwest-tracing`] provides a middleware to trace [`reqwest`] HTTP requests.
- [`tracing-cloudwatch`] provides a layer that sends events to AWS CloudWatch Logs.
- [`tracing-subscriber-reload-arcswap`] provides a lock-free alternative to `tracing_subscriber::reload::Layer` using `arc-swap`.
- [`clippy-tracing`] provides a tool to add, remove and check for `tracing::instrument`.

(if you're the maintainer of a `tracing` ecosystem crate not in this list,
please let us know!)

[`tracing-timing`]: https://crates.io/crates/tracing-timing
[`tracing-honeycomb`]: https://crates.io/crates/tracing-honeycomb
[`tracing-distributed`]: https://crates.io/crates/tracing-distributed
[honeycomb.io]: https://www.honeycomb.io/
[`tracing-actix`]: https://crates.io/crates/tracing-actix
[`tracing-actix-web`]: https://crates.io/crates/tracing-actix-web
[`axum-insights`]: https://crates.io/crates/axum-insights
[`tracing-gelf`]: https://crates.io/crates/tracing-gelf
[`tracing-coz`]: https://crates.io/crates/tracing-coz
[coz]: https://github.com/plasma-umass/coz
[`tracing-bunyan-formatter`]: https://crates.io/crates/tracing-bunyan-formatter
[`tide-tracing`]: https://crates.io/crates/tide-tracing
[tide]: https://crates.io/crates/tide
[bunyan]: https://github.com/trentm/node-bunyan
[`color-spantrace`]: https://docs.rs/color-spantrace
[`color-eyre`]: https://docs.rs/color-eyre
[`spandoc`]: https://docs.rs/spandoc
[`tracing-wasm`]: https://docs.rs/tracing-wasm
[`tracing-web`]: https://crates.io/crates/tracing-web
[`test-log`]: https://crates.io/crates/test-log
[User Timing API (`window.performance`)]: https://developer.mozilla.org/en-US/docs/Web/API/User_Timing_API
[`tracing-unwrap`]: https://docs.rs/tracing-unwrap
[`diesel`]: https://crates.io/crates/diesel
[`diesel-tracing`]: https://crates.io/crates/diesel-tracing
[`tracing-tracy`]: https://crates.io/crates/tracing-tracy
[Tracy]: https://github.com/wolfpld/tracy
[`tracing-elastic-apm`]: https://crates.io/crates/tracing-elastic-apm
[Elastic APM]: https://www.elastic.co/apm
[`tracing-etw`]: https://github.com/microsoft/rust_win_etw/tree/main/win_etw_tracing
[ETW]: https://docs.microsoft.com/en-us/windows/win32/etw/about-event-tracing
[`sentry-tracing`]: https://crates.io/crates/sentry-tracing
[Sentry]: https://sentry.io/welcome/
[`tracing-forest`]: https://crates.io/crates/tracing-forest
[`tracing-loki`]: https://crates.io/crates/tracing-loki
[Grafana Loki]: https://grafana.com/oss/loki/
[`tracing-logfmt`]: https://crates.io/crates/tracing-logfmt
[`tracing-chrome`]: https://crates.io/crates/tracing-chrome
[`reqwest-tracing`]: https://crates.io/crates/reqwest-tracing
[`reqwest`]: https://crates.io/crates/reqwest
[`tracing-cloudwatch`]: https://crates.io/crates/tracing-cloudwatch
[`tracing-subscriber-reload-arcswap`]: https://crates.io/crates/tracing-subscriber-reload-arcswap
[`clippy-tracing`]: https://crates.io/crates/clippy-tracing

**Note:** that some of the ecosystem crates are currently unreleased and
undergoing active development. They may be less stable than `tracing` and
`tracing-core`.
