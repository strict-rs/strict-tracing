//! Tests for `on_register_dispatch` expectations in `MockSubscriber` and `MockLayer`.

#[cfg(test)]
mod subscriber_registration {
  use strict_test_support::TestFailure;
  use strict_test_support::ensure_contains;
  use strict_test_support::ensure_ok;
  use strict_test_support::ensure_some;
  use tracing::subscriber::with_default;
  use tracing_mock::expect;
  use tracing_mock::subscriber;

  #[test]
  fn subscriber_on_register_dispatch() -> Result<(), TestFailure> {
    let (subscriber, handle) = subscriber::mock().on_register_dispatch().run_with_handle();

    with_default(subscriber, || {
      // The subscriber's on_register_dispatch is called when set as default
    });

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

  #[test]
  fn subscriber_on_register_dispatch_missing_registration() -> Result<(), TestFailure> {
    let (_subscriber, handle) = subscriber::mock().on_register_dispatch().run_with_handle();

    let error = ensure_some(
      handle.finished().err(),
      "unregistered subscriber should leave the registration expectation pending",
    )?;
    let rendered = error.to_string();
    ensure_contains(
      &rendered,
      "more notifications expected",
      "mock expectation error reports an unmet pending notification",
    )?;
    ensure_contains(
      &rendered,
      "on_register_dispatch",
      "mock expectation error names the missed registration callback",
    )
  }
}

#[cfg(test)]
#[cfg(feature = "tracing-subscriber")]
mod layer_registration {
  use strict_test_support::TestFailure;
  use strict_test_support::ensure_contains;
  use strict_test_support::ensure_ok;
  use strict_test_support::ensure_some;
  use tracing::subscriber::set_default;
  use tracing_core::Event;
  use tracing_core::subscriber::SubscriberResult;
  use tracing_mock::expect;
  use tracing_mock::layer;
  use tracing_subscriber::Layer;
  use tracing_subscriber::layer::Context;
  use tracing_subscriber::layer::SubscriberExt as _;

  #[test]
  fn layer_on_register_dispatch() -> Result<(), TestFailure> {
    let (layer, handle) = layer::mock().on_register_dispatch().run_with_handle();

    let subscriber = tracing_subscriber::registry().with(layer);
    let subscriber_guard = set_default(subscriber);

    // The layer's on_register_dispatch is called when the subscriber is set as default
    drop(subscriber_guard);

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[test]
  fn layer_multiple_expectations() -> Result<(), TestFailure> {
    let (layer, handle) = layer::mock().on_register_dispatch().event(expect::event()).run_with_handle();

    let subscriber = tracing_subscriber::registry().with(layer);
    let subscriber_guard = set_default(subscriber);

    tracing::info!("test event");

    drop(subscriber_guard);
    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[test]
  fn layer_on_register_dispatch_not_propagated() -> Result<(), TestFailure> {
    use tracing::error;

    /// A consumer layer wrapper that forwards events but drops registration callbacks.
    struct NonForwardingLayer<L> {
      inner: L,
    }

    impl<S, L> Layer<S> for NonForwardingLayer<L>
    where
      S: tracing_core::Subscriber,
      L: Layer<S>,
    {
      fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) -> SubscriberResult {
        self.inner.on_event(event, ctx)
      }
    }

    let (mock_layer, handle) = layer::named("inner").on_register_dispatch().run_with_handle();

    let non_forwarding_layer = NonForwardingLayer {
      inner: mock_layer
    };

    let subscriber = tracing_subscriber::registry().with(non_forwarding_layer);
    let subscriber_guard = set_default(subscriber);

    // This event will be sent to the mock layer, which expects on_register_dispatch first
    error!("send an event");

    drop(subscriber_guard);

    let error = ensure_some(handle.finished().err(), "mock expectations should return a registration mismatch")?;
    ensure_contains(
      &error.to_string(),
      "expected on_register_dispatch to be called",
      "mock expectation error includes registration mismatch",
    )
  }
}
