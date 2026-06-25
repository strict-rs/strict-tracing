//! Verifies span lifecycle logs are disabled by default while field updates log.

#[cfg(test)]
mod tests {
    use strict_test_support::{TestFailure, ensure_eq};
    use test_log_support::Test;
    use tracing::{Level, error, info, span, trace, warn};

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[test]
    fn span_lifecycle_defaults_off() -> Result<(), TestFailure> {
        let test = Test::try_with_filters(&[(module_path!(), log::LevelFilter::Trace)])?;

        error!(foo = 5);
        test.try_assert_logged("foo=5")?;

        warn!("hello {};", "world");
        test.try_assert_logged("hello world;")?;

        info!(message = "hello world;", thingy = 42, other_thingy = 666);
        test.try_assert_logged("hello world; thingy=42 other_thingy=666")?;

        let lifecycle_span = span!(Level::TRACE, "foo");
        test.try_assert_not_logged()?;

        lifecycle_span.in_scope(|| -> Result<(), TestFailure> {
            // enter should not be logged
            test.try_assert_not_logged()?;

            trace!({foo = 3, bar = 4}, "hello {};", "san francisco");
            test.try_assert_logged("hello san francisco; foo=3 bar=4")
        })?;
        // exit should not be logged
        test.try_assert_not_logged()?;

        drop(lifecycle_span);
        // drop should not be logged
        test.try_assert_not_logged()?;

        trace!(foo = 1, bar = 2, "hello world");
        test.try_assert_logged("hello world foo=1 bar=2")?;

        let field_span = span!(Level::TRACE, "foo", bar = 3, baz = false);
        // creating a span with fields _should_ be logged.
        test.try_assert_logged("foo; bar=3 baz=false")?;

        field_span.in_scope(|| -> Result<(), TestFailure> {
            // entering the span should not be logged
            test.try_assert_not_logged()
        })?;
        // exiting the span should not be logged
        test.try_assert_not_logged()?;

        ensure_eq(
            &field_span.record("baz", true).is_disabled(),
            &field_span.is_disabled(),
            "span record disabled state matches span disabled state",
        )?;
        // recording a field should be logged
        test.try_assert_logged("foo; baz=true")?;

        drop(field_span);
        // drop should not be logged.
        test.try_assert_not_logged()?;

        Ok(())
    }
}
