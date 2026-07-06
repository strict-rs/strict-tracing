//! `Instrument` and `WithSubscriber` integration coverage.

#![cfg(feature = "std")]

#[cfg(test)]
mod tests {
  // These tests require the thread-local scoped dispatcher, which only works when
  // we have a standard library. The behaviour being tested should be the same
  // with the standard lib disabled.

  use std::future;
  use std::future::Future;
  use std::pin::Pin;
  use std::task;

  use futures::FutureExt as _;
  use strict_test_support::TestFailure;
  use strict_test_support::ensure;
  use strict_test_support::ensure_eq;
  use strict_test_support::ensure_ok;
  use strict_test_support::ensure_some;
  use tracing::Instrument as _;
  use tracing::Level;
  use tracing::instrument::WithSubscriber as _;
  use tracing::level_filters::STATIC_MAX_LEVEL;
  use tracing::subscriber::with_default;
  use tracing_mock::*;

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn span_on_drop() -> Result<(), TestFailure> {
    #[derive(Clone, Debug)]
    struct AssertSpanOnDrop;

    impl Drop for AssertSpanOnDrop {
      fn drop(&mut self) {
        tracing::info!("Drop");
      }
    }

    struct Fut {
      span_on_drop: Option<AssertSpanOnDrop>,
    }

    impl Future for Fut {
      type Output = ();

      fn poll(mut self: Pin<&mut Self>, _: &mut task::Context<'_>) -> task::Poll<Self::Output> {
        drop(self.span_on_drop.take());
        task::Poll::Ready(())
      }
    }

    let subscriber = subscriber::mock()
      .enter(expect::span().named("foo"))
      .event(
        expect::event()
          .with_ancestry(expect::has_contextual_parent("foo"))
          .at_level(Level::INFO),
      )
      .exit(expect::span().named("foo"))
      .enter(expect::span().named("foo"))
      .exit(expect::span().named("foo"))
      .close_span(expect::span().named("foo"))
      .enter(expect::span().named("bar"))
      .event(
        expect::event()
          .with_ancestry(expect::has_contextual_parent("bar"))
          .at_level(Level::INFO),
      )
      .exit(expect::span().named("bar"))
      .close_span(expect::span().named("bar"))
      .only()
      .run();

    with_default(subscriber, || -> Result<(), TestFailure> {
      // polled once
      let poll_result = Fut {
        span_on_drop: Some(AssertSpanOnDrop),
      }
      .instrument(tracing::span!(Level::TRACE, "foo"))
      .now_or_never();
      ensure(poll_result.is_some(), "instrumented future should complete when polled once")?;

      // never polled
      drop(
        Fut {
          span_on_drop: Some(AssertSpanOnDrop),
        }
        .instrument(tracing::span!(Level::TRACE, "bar")),
      );
      Ok(())
    })
  }

  #[test]
  fn instrumented_accessors_reflect_inner_state_before_into_inner() -> Result<(), TestFailure> {
    let span = tracing::info_span!("accessor_span");
    let mut instrumented = future::ready(13_u64).instrument(span);

    ensure(instrumented.inner().is_some(), "inner is available before consumption")?;
    let inner_mut = ensure_some(instrumented.inner_mut(), "mutable inner is available before consumption")?;
    *inner_mut = future::ready(21_u64);
    ensure(
      instrumented.span().metadata().is_some(),
      "span accessor returns the instrumenting span",
    )?;
    *instrumented.span_mut() = tracing::Span::none();
    ensure(
      instrumented.span().metadata().is_none(),
      "mutable span accessor replaces the instrumenting span",
    )?;
    ensure(
      Pin::new(&instrumented).inner_pin_ref().is_some(),
      "pinned shared inner is available before consumption",
    )?;
    ensure(
      Pin::new(&mut instrumented).inner_pin_mut().is_some(),
      "pinned mutable inner is available before consumption",
    )?;

    let inner = ensure_some(instrumented.into_inner(), "into_inner returns the wrapped future")?;
    let output = ensure_some(inner.now_or_never(), "returned future completes")?;

    ensure_eq(&output, &21_u64, "inner_mut updates the wrapped future")
  }

  #[test]
  fn instrumented_future_enters_span_while_polled() -> Result<(), TestFailure> {
    let subscriber = subscriber::mock()
      .enter(expect::span().named("poll_span"))
      .event(
        expect::event()
          .with_ancestry(expect::has_contextual_parent("poll_span"))
          .at_level(Level::INFO),
      )
      .exit(expect::span().named("poll_span"))
      .close_span(expect::span().named("poll_span"))
      .only()
      .run();

    with_default(subscriber, || {
      let poll_output = async {
        tracing::info!("inside poll");
        7_u64
      }
      .instrument(tracing::info_span!("poll_span"))
      .now_or_never();

      let ready_output = ensure_some(poll_output, "instrumented future completes")?;
      ensure_eq(&ready_output, &7_u64, "instrumented future output is preserved")
    })
  }

  #[test]
  fn in_current_span_uses_the_current_span_for_later_polls() -> Result<(), TestFailure> {
    let subscriber = subscriber::mock()
      .enter(expect::span().named("current_span"))
      .enter(expect::span().named("current_span"))
      .event(
        expect::event()
          .with_ancestry(expect::has_contextual_parent("current_span"))
          .at_level(Level::INFO),
      )
      .exit(expect::span().named("current_span"))
      .exit(expect::span().named("current_span"))
      .close_span(expect::span().named("current_span"))
      .only()
      .run();

    with_default(subscriber, || {
      let span = tracing::info_span!("current_span");
      let guard = span.enter();
      let instrumented = async {
        tracing::info!("inside captured current span");
      }
      .in_current_span();
      drop(guard);

      ensure(instrumented.now_or_never().is_some(), "future with current span completes")
    })
  }

  #[test]
  fn with_dispatch_accessors_expose_dispatch_and_inner_future() -> Result<(), TestFailure> {
    let subscriber = subscriber::mock().run();
    let mut with_dispatch = 5_u64.with_subscriber(subscriber);

    ensure_eq(with_dispatch.inner(), &5_u64, "shared inner accessor exposes the value")?;
    *with_dispatch.inner_mut() = 8_u64;
    ensure_eq(
      &*Pin::new(&with_dispatch).inner_pin_ref(),
      &8_u64,
      "pinned shared inner exposes the value",
    )?;
    *Pin::new(&mut with_dispatch).inner_pin_mut() = 13_u64;
    let _dispatch = with_dispatch.dispatcher();
    let inner = with_dispatch.into_inner();

    ensure_eq(&inner, &13_u64, "with_dispatch preserves mutations to the wrapped value")
  }

  #[test]
  fn with_current_subscriber_polls_with_the_captured_dispatch() -> Result<(), TestFailure> {
    let (captured_subscriber, captured_handle) = subscriber::mock()
      .expect_when(STATIC_MAX_LEVEL.enables(Level::ERROR), |builder| {
        builder.event(
          expect::event()
            .with_fields(expect::msg("captured dispatch event"))
            .at_level(Level::ERROR),
        )
      })
      .only()
      .run_with_handle();
    let other_subscriber = subscriber::mock().only().run();

    let with_dispatch = with_default(captured_subscriber, || {
      async {
        tracing::error!("captured dispatch event");
        144_u64
      }
      .with_current_subscriber()
    });

    let maybe_output = with_default(other_subscriber, || with_dispatch.now_or_never());
    let output = ensure_some(maybe_output, "with_current_subscriber future completes")?;
    ensure_eq(&output, &144_u64, "with_current_subscriber preserves future output")?;
    ensure_ok(captured_handle.finished(), "captured subscriber should receive the event")
  }
}
