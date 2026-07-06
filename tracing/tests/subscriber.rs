//! Subscriber interaction integration coverage.

#![cfg(feature = "std")]

#[cfg(test)]
mod tests {
  // These tests require the thread-local scoped dispatcher, which only works when
  // we have a standard library. The behaviour being tested should be the same
  // with the standard lib disabled.
  //
  // The alternative would be for each of these tests to be defined in a separate
  // file, which is :(
  use core::num::NonZeroU64;
  use std::sync::Arc;
  use std::sync::atomic::AtomicBool;
  use std::sync::atomic::Ordering;

  use strict_test_support::TestFailure;
  use strict_test_support::ensure;
  use strict_test_support::ensure_ok;
  use tracing::Event;
  use tracing::Level;
  use tracing::Metadata;
  use tracing::field::display;
  use tracing::level_filters::STATIC_MAX_LEVEL;
  use tracing::span::Attributes;
  use tracing::span::Id;
  use tracing::span::Record;
  use tracing::subscriber::Interest;
  use tracing::subscriber::Subscriber;
  use tracing::subscriber::SubscriberResult;
  use tracing::subscriber::with_default;
  use tracing_mock::expect;
  use tracing_mock::subscriber;

  const TEST_SUBSCRIBER_SPAN_ID: Id = match Id::try_from_u64(0xAAAA) {
    Some(id) => id,
    None => Id::from_non_zero_u64(NonZeroU64::MIN),
  };

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn event_macros_dont_infinite_loop() -> Result<(), TestFailure> {
    // This test ensures that an event macro within a subscriber
    // won't cause an infinite loop of events.
    struct TestSubscriber {
      missing_enabled_field: Arc<AtomicBool>,
      missing_event_field:   Arc<AtomicBool>,
    }

    impl Subscriber for TestSubscriber {
      fn register_callsite(&self, _: &'static Metadata<'static>) -> SubscriberResult<Interest> {
        // Always return sometimes so that `enabled` will be called
        // (which can loop).
        Ok(Interest::sometimes())
      }

      fn enabled(&self, meta: &Metadata<'_>) -> SubscriberResult<bool> {
        self
          .missing_enabled_field
          .store(!meta.fields().iter().any(|field| field.name() == "foo"), Ordering::Relaxed);
        tracing::event!(Level::TRACE, bar = false);
        Ok(true)
      }

      fn new_span(&self, _: &Attributes<'_>) -> SubscriberResult<Id> {
        Ok(TEST_SUBSCRIBER_SPAN_ID)
      }

      fn record(&self, _: Id, _: &Record<'_>) -> SubscriberResult {
        Ok(())
      }

      fn record_follows_from(&self, _: Id, _: Id) -> SubscriberResult {
        Ok(())
      }

      fn event(&self, event: &Event<'_>) -> SubscriberResult {
        self.missing_event_field.store(
          !event.metadata().fields().iter().any(|field| field.name() == "foo"),
          Ordering::Relaxed,
        );
        tracing::event!(Level::TRACE, baz = false);
        Ok(())
      }

      fn enter(&self, _: Id) -> SubscriberResult {
        Ok(())
      }

      fn exit(&self, _: Id) -> SubscriberResult {
        Ok(())
      }
    }

    let missing_enabled_field = Arc::new(AtomicBool::new(false));
    let missing_event_field = Arc::new(AtomicBool::new(false));

    with_default(
      TestSubscriber {
        missing_enabled_field: Arc::clone(&missing_enabled_field),
        missing_event_field:   Arc::clone(&missing_event_field),
      },
      || {
        tracing::event!(Level::TRACE, foo = false);
      },
    );

    ensure(
      !missing_enabled_field.load(Ordering::Relaxed),
      "enabled callback should see the original event field",
    )?;
    ensure(
      !missing_event_field.load(Ordering::Relaxed),
      "event callback should see the original event field",
    )
  }

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn boxed_subscriber() -> Result<(), TestFailure> {
    let (mock_subscriber, handle) = subscriber::mock()
      .expect_when(STATIC_MAX_LEVEL.enables(Level::TRACE), |builder| {
        builder
          .new_span(
            expect::span()
              .named("foo")
              .with_fields(expect::field("bar").with_value(&display("hello from my span")).only()),
          )
          .enter(expect::span().named("foo"))
          .exit(expect::span().named("foo"))
          .close_span(expect::span().named("foo"))
      })
      .only()
      .run_with_handle();
    let subscriber: Box<dyn Subscriber + Send + Sync + 'static> = Box::new(mock_subscriber);

    with_default(subscriber, || {
      let from = "my span";
      let span = tracing::span!(Level::TRACE, "foo", bar = format_args!("hello from {from}"));
      span.in_scope(|| {});
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  // A TRACE span wraps the INFO event, so a `max_level_*` cap that disables
  // TRACE removes the span notifications while (some caps) still deliver the
  // event, changing the delivered subset and ancestry; run this only when the
  // whole scenario survives statically.
  #[cfg(not(any(
    feature = "max_level_off",
    feature = "max_level_error",
    feature = "max_level_warn",
    feature = "max_level_info",
    feature = "max_level_debug"
  )))]
  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn arced_subscriber() -> Result<(), TestFailure> {
    let (mock_subscriber, handle) = subscriber::mock()
      .new_span(
        expect::span()
          .named("foo")
          .with_fields(expect::field("bar").with_value(&display("hello from my span")).only()),
      )
      .enter(expect::span().named("foo"))
      .exit(expect::span().named("foo"))
      .close_span(expect::span().named("foo"))
      .event(expect::event().with_fields(expect::field("message").with_value(&display("hello from my event"))))
      .only()
      .run_with_handle();
    let subscriber: Arc<dyn Subscriber + Send + Sync + 'static> = Arc::new(mock_subscriber);

    // Test using a clone of the `Arc`ed subscriber
    with_default(Arc::clone(&subscriber), || {
      let from = "my span";
      let span = tracing::span!(Level::TRACE, "foo", bar = format_args!("hello from {from}"));
      span.in_scope(|| {});
    });

    with_default(subscriber, || {
      tracing::info!("hello from my event");
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn boxed_subscriber_receives_on_register_dispatch() -> Result<(), TestFailure> {
    let (mock_subscriber, handle) = subscriber::mock().on_register_dispatch().only().run_with_handle();
    let subscriber: Box<dyn Subscriber + Send + Sync + 'static> = Box::new(mock_subscriber);

    with_default(subscriber, || {
      // Installing the boxed subscriber as the default dispatch must forward
      // `on_register_dispatch` through the `Box` to the wrapped mock.
    });

    ensure_ok(handle.finished(), "boxed subscriber should observe on_register_dispatch")?;
    Ok(())
  }

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn arced_subscriber_receives_on_register_dispatch() -> Result<(), TestFailure> {
    let (mock_subscriber, handle) = subscriber::mock().on_register_dispatch().only().run_with_handle();
    let subscriber: Arc<dyn Subscriber + Send + Sync + 'static> = Arc::new(mock_subscriber);

    with_default(subscriber, || {
      // Installing the arced subscriber as the default dispatch must forward
      // `on_register_dispatch` through the `Arc` to the wrapped mock.
    });

    ensure_ok(handle.finished(), "arced subscriber should observe on_register_dispatch")?;
    Ok(())
  }

  // The negative assertion depends on the INFO event actually being delivered:
  // the event arriving while `on_register_dispatch` is still the front
  // expectation is what triggers the mismatch whose text this test inspects. A
  // `max_level_*` cap that disables INFO compiles the event out, so gate the
  // test on INFO remaining statically enabled.
  #[cfg(not(any(
    feature = "max_level_off",
    feature = "max_level_error",
    feature = "max_level_warn"
  )))]
  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn non_forwarding_wrapper_drops_on_register_dispatch() -> Result<(), TestFailure> {
    use strict_test_support::ensure_contains;
    use strict_test_support::ensure_some;

    /// A subscriber wrapping another subscriber WITHOUT forwarding
    /// `on_register_dispatch`, unlike the `Box` and `Arc` forwarding impls:
    /// the trait's default no-op drops the notification before the inner
    /// subscriber can observe it.
    struct NonForwardingSubscriber<S> {
      /// The wrapped subscriber that never observes `on_register_dispatch`.
      inner: S,
    }

    impl<S: Subscriber> Subscriber for NonForwardingSubscriber<S> {
      // `on_register_dispatch` is intentionally NOT implemented here; the
      // default no-op implementation swallows the registration callback.

      fn register_callsite(&self, metadata: &'static Metadata<'static>) -> SubscriberResult<Interest> {
        self.inner.register_callsite(metadata)
      }

      fn enabled(&self, metadata: &Metadata<'_>) -> SubscriberResult<bool> {
        self.inner.enabled(metadata)
      }

      fn new_span(&self, span: &Attributes<'_>) -> SubscriberResult<Id> {
        self.inner.new_span(span)
      }

      fn record(&self, span: Id, values: &Record<'_>) -> SubscriberResult {
        self.inner.record(span, values)
      }

      fn record_follows_from(&self, span: Id, follows: Id) -> SubscriberResult {
        self.inner.record_follows_from(span, follows)
      }

      fn event(&self, event: &Event<'_>) -> SubscriberResult {
        self.inner.event(event)
      }

      fn enter(&self, span: Id) -> SubscriberResult {
        self.inner.enter(span)
      }

      fn exit(&self, span: Id) -> SubscriberResult {
        self.inner.exit(span)
      }
    }

    let (mock_subscriber, handle) = subscriber::mock()
      .on_register_dispatch()
      .event(expect::event())
      .run_with_handle();

    with_default(
      NonForwardingSubscriber {
        inner: mock_subscriber
      },
      || {
        tracing::info!("send an event");
      },
    );

    let error = ensure_some(handle.finished().err(), "dropped registration should surface as a mock failure")?;
    ensure_contains(
      &error.to_string(),
      "expected on_register_dispatch to be called",
      "mock expectation error names the missed registration callback",
    )
  }
}
