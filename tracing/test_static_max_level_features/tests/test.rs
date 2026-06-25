//! Verifies compile-time static max-level filtering for events, spans, and `#[instrument]`.

#[cfg(test)]
mod tests {
    use core::fmt::{self, Display, Formatter};
    use core::future::Future;
    use core::num::NonZeroU64;
    use core::pin::Pin;
    use std::error::Error;
    use std::sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    };
    use tracing::{
        Event, Level, Metadata, debug, error, info, instrument, span,
        span::{Attributes, Id, Record},
        subscriber::{Subscriber, SubscriberResult, with_default},
        trace, warn,
    };
    use tracing_test::block_on_future;

    const LEVEL_NONE: u8 = 0;
    const LEVEL_ERROR: u8 = 1;
    const LEVEL_WARN: u8 = 2;
    const LEVEL_INFO: u8 = 3;
    const LEVEL_DEBUG: u8 = 4;
    const LEVEL_TRACE: u8 = 5;
    const EXPECTED_DEBUG: Option<Level> = if cfg!(debug_assertions) {
        Some(Level::DEBUG)
    } else {
        None
    };

    struct State {
        last_level: AtomicU8,
    }

    impl State {
        const fn new() -> Self {
            Self {
                last_level: AtomicU8::new(LEVEL_NONE),
            }
        }

        fn observe_level(&self, level: Level) {
            let encoded_level = match level {
                Level::ERROR => LEVEL_ERROR,
                Level::WARN => LEVEL_WARN,
                Level::INFO => LEVEL_INFO,
                Level::DEBUG => LEVEL_DEBUG,
                Level::TRACE => LEVEL_TRACE,
            };
            self.last_level.store(encoded_level, Ordering::Relaxed);
        }

        fn take_level(&self) -> Option<Level> {
            match self.last_level.swap(LEVEL_NONE, Ordering::Relaxed) {
                LEVEL_ERROR => Some(Level::ERROR),
                LEVEL_WARN => Some(Level::WARN),
                LEVEL_INFO => Some(Level::INFO),
                LEVEL_DEBUG => Some(Level::DEBUG),
                LEVEL_TRACE => Some(Level::TRACE),
                _ => None,
            }
        }
    }

    struct TestSubscriber(Arc<State>);

    impl Subscriber for TestSubscriber {
        fn enabled(&self, _: &Metadata<'_>) -> SubscriberResult<bool> {
            Ok(true)
        }

        fn new_span(&self, span: &Attributes<'_>) -> SubscriberResult<Id> {
            self.0.observe_level(*span.metadata().level());
            Ok(Id::from_non_zero_u64(NonZeroU64::MIN))
        }

        fn record(&self, _span: Id, _values: &Record<'_>) -> SubscriberResult {
            Ok(())
        }

        fn record_follows_from(&self, _span: Id, _follows: Id) -> SubscriberResult {
            Ok(())
        }

        fn event(&self, event: &Event<'_>) -> SubscriberResult {
            self.0.observe_level(*event.metadata().level());
            Ok(())
        }

        fn enter(&self, _span: Id) -> SubscriberResult {
            Ok(())
        }

        fn exit(&self, _span: Id) -> SubscriberResult {
            Ok(())
        }
    }

    #[derive(Debug)]
    struct TestFailure {
        observed_level: Option<Level>,
        expected_level: Option<Level>,
    }

    impl Display for TestFailure {
        fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
            write!(
                formatter,
                "last observed level mismatch: expected {}, observed {}",
                describe_level(self.expected_level),
                describe_level(self.observed_level),
            )
        }
    }

    impl Error for TestFailure {}

    const fn describe_level(maybe_level: Option<Level>) -> &'static str {
        match maybe_level {
            Some(level) => level.as_str(),
            None => "none",
        }
    }

    #[track_caller]
    fn expect_last_level(state: &State, expected_level: Option<Level>) -> Result<(), TestFailure> {
        let observed_level = state.take_level();
        if observed_level == expected_level {
            Ok(())
        } else {
            Err(TestFailure {
                observed_level,
                expected_level,
            })
        }
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[test]
    fn test_static_max_level_events() -> Result<(), TestFailure> {
        let subscriber_state = Arc::new(State::new());
        let observed_state = Arc::clone(&subscriber_state);
        with_default(
            TestSubscriber(subscriber_state),
            || -> Result<(), TestFailure> {
                error!("");
                expect_last_level(&observed_state, Some(Level::ERROR))?;
                warn!("");
                expect_last_level(&observed_state, Some(Level::WARN))?;
                info!("");
                expect_last_level(&observed_state, Some(Level::INFO))?;
                debug!("");
                expect_last_level(&observed_state, EXPECTED_DEBUG)?;
                trace!("");
                expect_last_level(&observed_state, None)
            },
        )
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[test]
    fn test_static_max_level_spans() -> Result<(), TestFailure> {
        let subscriber_state = Arc::new(State::new());
        let observed_state = Arc::clone(&subscriber_state);

        with_default(
            TestSubscriber(subscriber_state),
            || -> Result<(), TestFailure> {
                span!(Level::ERROR, "");
                expect_last_level(&observed_state, Some(Level::ERROR))?;
                span!(Level::WARN, "");
                expect_last_level(&observed_state, Some(Level::WARN))?;
                span!(Level::INFO, "");
                expect_last_level(&observed_state, Some(Level::INFO))?;
                span!(Level::DEBUG, "");
                expect_last_level(&observed_state, EXPECTED_DEBUG)?;
                span!(Level::TRACE, "");
                expect_last_level(&observed_state, None)
            },
        )
    }

    #[instrument(level = "debug")]
    #[inline(never)] // this makes it a bit easier to look at the asm output
    fn instrumented_fn() {}

    #[instrument(level = "debug")]
    async fn instrumented_async_fn() {}

    #[instrument(level = "debug")]
    fn instrumented_manual_async() -> impl Future<Output = ()> {
        async move {}
    }

    #[instrument(level = "debug")]
    fn instrumented_manual_box_pin() -> Pin<Box<dyn Future<Output = ()>>> {
        Box::pin(async move {})
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[test]
    fn test_static_max_level_instrument() -> Result<(), TestFailure> {
        let subscriber_state = Arc::new(State::new());
        let observed_state = Arc::clone(&subscriber_state);

        with_default(
            TestSubscriber(subscriber_state),
            || -> Result<(), TestFailure> {
                block_on_future(async {
                    instrumented_fn();
                    expect_last_level(&observed_state, EXPECTED_DEBUG)?;
                    instrumented_fn();
                    expect_last_level(&observed_state, EXPECTED_DEBUG)?;

                    instrumented_async_fn().await;
                    expect_last_level(&observed_state, EXPECTED_DEBUG)?;
                    instrumented_async_fn().await;
                    expect_last_level(&observed_state, EXPECTED_DEBUG)?;

                    instrumented_manual_async().await;
                    expect_last_level(&observed_state, EXPECTED_DEBUG)?;
                    instrumented_manual_async().await;
                    expect_last_level(&observed_state, EXPECTED_DEBUG)?;

                    instrumented_manual_box_pin().await;
                    expect_last_level(&observed_state, EXPECTED_DEBUG)?;
                    instrumented_manual_box_pin().await;
                    expect_last_level(&observed_state, EXPECTED_DEBUG)
                })
            },
        )
    }
}
