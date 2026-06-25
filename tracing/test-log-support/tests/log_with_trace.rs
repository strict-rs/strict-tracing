//! Verifies `log-always` output when a tracing subscriber is installed.

#[cfg(test)]
mod tests {
    use std::num::{NonZeroU16, NonZeroU64, Wrapping};
    use strict_test_support::{TestFailure, ensure_ok};
    use test_log_support::Test;
    use tracing::span::{Attributes, Id, Record};
    use tracing::subscriber::{SubscriberResult, set_global_default};
    use tracing::{Event, Metadata};
    use tracing::{Level, error, info, span, trace, warn};

    struct NopSubscriber;

    impl tracing::Subscriber for NopSubscriber {
        fn enabled(&self, _: &Metadata<'_>) -> SubscriberResult<bool> {
            Ok(true)
        }
        fn new_span(&self, _: &Attributes<'_>) -> SubscriberResult<Id> {
            use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
            static NEXT: AtomicU64 = AtomicU64::new(0);

            let span_id = NonZeroU64::MIN.saturating_add(NEXT.fetch_add(1, Relaxed));
            Ok(Id::from_non_zero_u64(span_id))
        }
        fn record(&self, _: Id, _: &Record<'_>) -> SubscriberResult {
            Ok(())
        }
        fn record_follows_from(&self, _: Id, _: Id) -> SubscriberResult {
            Ok(())
        }
        fn event(&self, _: &Event<'_>) -> SubscriberResult {
            Ok(())
        }
        fn enter(&self, _: Id) -> SubscriberResult {
            Ok(())
        }
        fn exit(&self, _: Id) -> SubscriberResult {
            Ok(())
        }
        fn try_close(&self, _: Id) -> SubscriberResult<bool> {
            Ok(true)
        }
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[test]
    fn log_with_trace() -> Result<(), TestFailure> {
        ensure_ok(
            set_global_default(NopSubscriber),
            "global tracing subscriber installs",
        )?;

        let test = Test::try_start()?;

        error!(foo = 5);
        test.try_assert_logged("foo=5")?;

        error!(foo = NonZeroU16::MIN.saturating_add(41));
        test.try_assert_logged("foo=42")?;

        error!(foo = Wrapping(39));
        test.try_assert_logged("foo=39")?;

        warn!("hello {};", "world");
        test.try_assert_logged("hello world;")?;

        info!(message = "hello world;", thingy = 42, other_thingy = 666);
        test.try_assert_logged("hello world; thingy=42 other_thingy=666")?;

        let trace_span = span!(Level::TRACE, "foo");
        test.try_assert_logged("++ foo; span=1")?;

        trace_span.in_scope(|| -> Result<(), TestFailure> {
            test.try_assert_logged("-> foo; span=1")?;

            trace!({foo = 3, bar = 4}, "hello {};", "san francisco");
            test.try_assert_logged("hello san francisco; foo=3 bar=4")
        })?;
        test.try_assert_logged("<- foo; span=1")?;

        drop(trace_span);
        test.try_assert_logged("-- foo; span=1")?;

        let field_span = span!(Level::TRACE, "foo", bar = 3, baz = false);
        test.try_assert_logged("++ foo; bar=3 baz=false span=2")?;

        drop(field_span);
        test.try_assert_logged("-- foo; span=2")?;

        trace!(foo = 1, bar = 2, "hello world");
        test.try_assert_logged("hello world foo=1 bar=2")?;

        // TODO(#1138): determine a new syntax for uninitialized span fields, and
        // re-enable these.
        // let span = span!(Level::TRACE, "foo", bar = _, baz = _);
        // span.record("bar", &3);
        // test.try_assert_logged("foo; bar=3")?;
        // span.record("baz", &"a string");
        // test.try_assert_logged("foo; baz=\"a string\"")?;

        Ok(())
    }
}
