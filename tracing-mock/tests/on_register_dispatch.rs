//! Tests for `on_register_dispatch` expectations in `MockSubscriber` and `MockLayer`.

#[cfg(test)]
mod tests {
    use strict_test_support::{TestFailure, ensure_contains, ensure_ok, ensure_some};
    use tracing::subscriber::{set_default, with_default};
    use tracing_mock::{expect, subscriber};

    #[test]
    fn subscriber_on_register_dispatch() -> Result<(), TestFailure> {
        let (subscriber, handle) = subscriber::mock().on_register_dispatch().run_with_handle();

        with_default(subscriber, || {
            // The subscriber's on_register_dispatch is called when set as default
        });

        ensure_ok(handle.finished(), "mock expectations should finish")?;
        Ok(())
    }

    #[cfg(feature = "tracing-subscriber")]
    #[test]
    fn layer_on_register_dispatch() -> Result<(), TestFailure> {
        use tracing_mock::layer;
        use tracing_subscriber::layer::SubscriberExt as _;

        let (layer, handle) = layer::mock().on_register_dispatch().run_with_handle();

        let subscriber = tracing_subscriber::registry().with(layer);
        let subscriber_guard = set_default(subscriber);

        // The layer's on_register_dispatch is called when the subscriber is set as default
        drop(subscriber_guard);

        ensure_ok(handle.finished(), "mock expectations should finish")?;
        Ok(())
    }

    #[test]
    fn subscriber_multiple_expectations() -> Result<(), TestFailure> {
        let (subscriber, handle) = subscriber::mock()
            .on_register_dispatch()
            .event(expect::event())
            .run_with_handle();

        with_default(subscriber, || {
            tracing::info!("test event");
        });

        ensure_ok(handle.finished(), "mock expectations should finish")?;
        Ok(())
    }

    #[cfg(feature = "tracing-subscriber")]
    #[test]
    fn layer_multiple_expectations() -> Result<(), TestFailure> {
        use tracing_mock::layer;
        use tracing_subscriber::layer::SubscriberExt as _;

        let (layer, handle) = layer::mock()
            .on_register_dispatch()
            .event(expect::event())
            .run_with_handle();

        let subscriber = tracing_subscriber::registry().with(layer);
        let subscriber_guard = set_default(subscriber);

        tracing::info!("test event");

        drop(subscriber_guard);
        ensure_ok(handle.finished(), "mock expectations should finish")?;
        Ok(())
    }

    #[cfg(feature = "tracing-subscriber")]
    #[test]
    fn layer_on_register_dispatch_not_propagated() -> Result<(), TestFailure> {
        use tracing::error;
        use tracing_core::{Event, subscriber::SubscriberResult};
        use tracing_mock::layer;
        use tracing_subscriber::{
            Layer,
            layer::{Context, SubscriberExt as _},
        };

        /// A layer that wraps another layer but does NOT propagate `on_register_dispatch`
        struct BadLayer<L> {
            inner: L,
        }

        impl<S, L> Layer<S> for BadLayer<L>
        where
            S: tracing_core::Subscriber,
            L: Layer<S>,
        {
            // Intentionally NOT implementing on_register_dispatch to test the failure case
            // The default implementation does nothing, so the inner layer won't receive the call

            fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) -> SubscriberResult {
                self.inner.on_event(event, ctx)
            }
        }

        let (mock_layer, handle) = layer::named("inner")
            .on_register_dispatch()
            .run_with_handle();

        let bad_layer = BadLayer { inner: mock_layer };

        let subscriber = tracing_subscriber::registry().with(bad_layer);
        let subscriber_guard = set_default(subscriber);

        // This event will be sent to the mock layer, which expects on_register_dispatch first
        error!("send an event");

        drop(subscriber_guard);

        let error = ensure_some(
            handle.finished().err(),
            "mock expectations should return a registration mismatch",
        )?;
        ensure_contains(
            &error.to_string(),
            "expected on_register_dispatch to be called",
            "mock expectation error includes registration mismatch",
        )
    }
}
