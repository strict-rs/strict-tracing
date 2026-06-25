//! Verifies log output emitted without an installed tracing subscriber.

#[cfg(test)]
mod tests {
    use strict_test_support::TestFailure;
    use test_log_support::Test;
    use tracing::{Level, error, info, span, trace, warn};

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[test]
    fn test_always_log() -> Result<(), TestFailure> {
        let test = Test::try_start()?;

        error!(foo = 5);
        test.try_assert_logged("foo=5")?;

        warn!("hello {};", "world");
        test.try_assert_logged("hello world;")?;

        info!(
            message = "hello world;",
            thingy = display(42),
            other_thingy = debug(666)
        );
        test.try_assert_logged("hello world; thingy=42 other_thingy=666")?;

        let trace_span = span!(Level::TRACE, "foo");
        test.try_assert_logged("foo;")?;

        trace_span.in_scope(|| -> Result<(), TestFailure> {
            test.try_assert_logged("-> foo;")?;

            trace!({foo = 3, bar = 4}, "hello {};", "san francisco");
            test.try_assert_logged("hello san francisco; foo=3 bar=4")
        })?;
        test.try_assert_logged("<- foo;")?;

        drop(trace_span);
        test.try_assert_logged("-- foo;")?;

        trace!(foo = 1, bar = 2, "hello world");
        test.try_assert_logged("hello world foo=1 bar=2")?;

        let field_span = span!(Level::TRACE, "foo", bar = 3, baz = false);
        test.try_assert_logged("foo; bar=3 baz=false")?;

        drop(field_span);

        Ok(())
    }
}
