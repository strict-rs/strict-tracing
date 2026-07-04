//! Tests for `std::future::Future` instrumentation.

#[cfg(test)]
mod tests {
  use std::future::Future;
  use std::pin::Pin;
  use std::task;

  use futures::FutureExt as _;
  use strict_test_support::TestFailure;
  use strict_test_support::ensure;
  use strict_test_support::ensure_ok;
  use strict_test_support::ensure_some;
  use tracing::Instrument as _;
  use tracing::Level;
  use tracing::subscriber::with_default;
  use tracing_mock::expect;
  use tracing_mock::subscriber;
  use tracing_test::PollN;
  use tracing_test::block_on_future;

  #[test]
  fn enter_exit_is_reasonable() -> Result<(), TestFailure> {
    let (subscriber, handle) = subscriber::mock()
      .enter(expect::span().named("foo"))
      .exit(expect::span().named("foo"))
      .enter(expect::span().named("foo"))
      .exit(expect::span().named("foo"))
      .enter(expect::span().named("foo"))
      .exit(expect::span().named("foo"))
      .close_span(expect::span().named("foo"))
      .only()
      .run_with_handle();
    with_default(subscriber, || {
      let future = PollN::new_ok(2).instrument(tracing::span!(Level::TRACE, "foo"));
      ensure(block_on_future(future).is_ok(), "instrumented std future resolves")
    })?;
    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[test]
  fn error_ends_span() -> Result<(), TestFailure> {
    let (subscriber, handle) = subscriber::mock()
      .enter(expect::span().named("foo"))
      .exit(expect::span().named("foo"))
      .enter(expect::span().named("foo"))
      .exit(expect::span().named("foo"))
      .enter(expect::span().named("foo"))
      .exit(expect::span().named("foo"))
      .close_span(expect::span().named("foo"))
      .only()
      .run_with_handle();
    with_default(subscriber, || {
      let future = PollN::new_err(2).instrument(tracing::span!(Level::TRACE, "foo"));
      ensure(block_on_future(future).is_err(), "instrumented std future returns its error")
    })?;
    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

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

    with_default(subscriber, || {
      // polled once
      ensure_some(
        Fut {
          span_on_drop: Some(AssertSpanOnDrop),
        }
        .instrument(tracing::span!(Level::TRACE, "foo"))
        .now_or_never(),
        "instrumented std future resolves synchronously",
      )?;

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
}
