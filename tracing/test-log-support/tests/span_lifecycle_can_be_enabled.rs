//! Verifies span lifecycle logs are emitted when the lifecycle target is enabled.

#[cfg(test)]
mod tests {
  /// Native failures from logger setup and captured-output comparisons.
  #[derive(Debug, thiserror::Error)]
  enum TestError {
    /// Logger installation failed.
    #[error(transparent)]
    Logger(#[from] strict_test_support::ResultFailure<log::SetLoggerError>),
    /// Captured output differs, including an absent or unexpected line.
    #[error(transparent)]
    Output(#[from] strict_test_support::ComparisonFailure<Option<String>, Option<String>>),
    /// Recording changed the disabled state of a span.
    #[error(transparent)]
    Disabled(#[from] strict_test_support::ComparisonFailure<bool, bool>),
  }

  use strict_test_support::ensure_eq;
  use test_log_support::Test;
  use tracing::Level;
  use tracing::error;
  use tracing::info;
  use tracing::span;
  use tracing::trace;
  use tracing::warn;

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn span_lifecycle_can_be_enabled() -> Result<(), TestError> {
    let test = Test::try_with_filters(&[
      (module_path!(), log::LevelFilter::Trace),
      ("tracing::span", log::LevelFilter::Trace),
    ])?;

    error!(foo = 5);
    test.try_assert_logged("foo=5")?;

    warn!("hello {};", "world");
    test.try_assert_logged("hello world;")?;

    info!(message = "hello world;", thingy = 42, other_thingy = 666);
    test.try_assert_logged("hello world; thingy=42 other_thingy=666")?;

    let lifecycle_span = span!(Level::TRACE, "foo");
    test.try_assert_logged("foo;")?;

    lifecycle_span.in_scope(|| -> Result<(), TestError> {
      // enter should be logged
      test.try_assert_logged("-> foo;")?;

      trace!({foo = 3, bar = 4}, "hello {};", "san francisco");
      test
        .try_assert_logged("hello san francisco; foo=3 bar=4")
        .map_err(TestError::from)
    })?;
    // exit should be logged
    test.try_assert_logged("<- foo;")?;

    drop(lifecycle_span);
    // drop should be logged
    test.try_assert_logged("-- foo;")?;

    trace!(foo = 1, bar = 2, "hello world");
    test.try_assert_logged("hello world foo=1 bar=2")?;

    let field_span = span!(Level::TRACE, "foo", bar = 3, baz = false);
    // creating a span with fields _should_ be logged.
    test.try_assert_logged("foo; bar=3 baz=false")?;

    field_span.in_scope(|| -> Result<(), TestError> {
      // entering the span should be logged
      test.try_assert_logged("-> foo;").map_err(TestError::from)
    })?;
    // exiting the span should be logged
    test.try_assert_logged("<- foo;")?;

    ensure_eq(
      field_span.record("baz", true).is_disabled(),
      field_span.is_disabled(),
      "span record disabled state matches span disabled state",
    )
    .map(drop)?;
    // recording a field should be logged
    test.try_assert_logged("foo; baz=true")?;

    let bar = span!(Level::INFO, "bar");
    // lifecycles for INFO spans should be logged
    test.try_assert_logged("bar;")?;

    bar.in_scope(|| -> Result<(), TestError> {
      // entering the INFO span should be logged
      test.try_assert_logged("-> bar;").map_err(TestError::from)
    })?;
    // exiting the INFO span should be logged
    test.try_assert_logged("<- bar;")?;

    drop(field_span);
    // drop should be logged.
    test.try_assert_logged("-- foo;")?;

    drop(bar);
    // dropping the INFO should be logged.
    test.try_assert_logged("-- bar;")?;

    Ok(())
  }
}
