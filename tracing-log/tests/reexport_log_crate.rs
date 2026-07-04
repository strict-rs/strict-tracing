//! Tests the public re-export of the `log` crate.

#[cfg(test)]
mod tests {
  use strict_test_support::TestFailure;
  use strict_test_support::ensure_ok;
  use tracing_log::LogTracer;
  use tracing_log::log::LevelFilter;

  /// This test makes sure we can access `log::LevelFilter` through the
  /// `tracing_log` crate and don't have to depend on `log` separately.
  ///
  /// See <https://github.com/tokio-rs/tracing/issues/552>.
  #[test]
  fn can_initialize_log_tracer_with_level() -> Result<(), TestFailure> {
    ensure_ok(
      LogTracer::init_with_filter(LevelFilter::Error),
      "`LogTracer` should initialize with a re-exported `LevelFilter`",
    )
  }
}
