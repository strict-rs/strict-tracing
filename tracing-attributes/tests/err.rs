//! Example binary for tracing workspace checks.
#![cfg(test)]

use std::convert::TryFrom as _;
use std::num::TryFromIntError;

use strict_test_support::TestFailure;
use strict_test_support::ensure;
use strict_test_support::ensure_ok;
use tracing::Level;
use tracing::field::debug;
use tracing::field::display;
use tracing::subscriber::with_default;
use tracing_attributes::instrument;
use tracing_mock::*;
use tracing_subscriber::filter::EnvFilter;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_test::PollN;
use tracing_test::block_on_future;

#[instrument(err)]
fn err() -> Result<u8, TryFromIntError> {
  u8::try_from(1234)
}

#[instrument(err)]
#[allow(
  clippy::single_call_fn,
  reason = "suspicious-else err fixture remains a named instrumented function so formatting-safe expansion is asserted"
)]
fn err_suspicious_else() -> Result<u8, TryFromIntError> {
  {}
  u8::try_from(1234)
}

#[test]
fn test_suspicious_else() -> Result<(), TestFailure> {
  let span = expect::span().named("err_suspicious_else");
  let (subscriber, handle) = subscriber::mock()
    .new_span(span.clone())
    .enter(span.clone())
    .event(expect::event().at_level(Level::ERROR))
    .exit(span.clone())
    .close_span(span)
    .only()
    .run_with_handle();
  let _result = with_default(subscriber, || err_suspicious_else().ok());
  ensure_ok(handle.finished(), "mock expectations should finish")?;
  Ok(())
}

#[test]
fn test() -> Result<(), TestFailure> {
  let span = expect::span().named("err");
  let (subscriber, handle) = subscriber::mock()
    .new_span(span.clone())
    .enter(span.clone())
    .event(expect::event().at_level(Level::ERROR))
    .exit(span.clone())
    .close_span(span)
    .only()
    .run_with_handle();
  let _result = with_default(subscriber, || err().ok());
  ensure_ok(handle.finished(), "mock expectations should finish")?;
  Ok(())
}

#[instrument(err)]
#[allow(
  clippy::single_call_fn,
  reason = "early-return err fixture remains a named instrumented function so question-mark errors are asserted"
)]
fn err_early_return() -> Result<u8, TryFromIntError> {
  let _value = u8::try_from(1234)?;
  Ok(5)
}

#[test]
fn test_early_return() -> Result<(), TestFailure> {
  let span = expect::span().named("err_early_return");
  let (subscriber, handle) = subscriber::mock()
    .new_span(span.clone())
    .enter(span.clone())
    .event(expect::event().at_level(Level::ERROR))
    .exit(span.clone())
    .close_span(span)
    .only()
    .run_with_handle();
  let _result = with_default(subscriber, || err_early_return().ok());
  ensure_ok(handle.finished(), "mock expectations should finish")?;
  Ok(())
}

#[instrument(err)]
#[allow(
  clippy::single_call_fn,
  reason = "async err fixture remains a named instrumented function so awaited error events can be asserted"
)]
async fn err_async(polls: usize) -> Result<u8, TryFromIntError> {
  let future = PollN::new_ok(polls);
  tracing::trace!(awaiting = true);
  let _result = future.await.ok();
  u8::try_from(1234)
}

#[test]
fn test_async() -> Result<(), TestFailure> {
  let span = expect::span().named("err_async");
  let (subscriber, handle) = subscriber::mock()
    .new_span(span.clone())
    .enter(span.clone())
    .event(
      expect::event()
        .with_fields(expect::field("awaiting").with_value(&true))
        .at_level(Level::TRACE),
    )
    .exit(span.clone())
    .enter(span.clone())
    .event(expect::event().at_level(Level::ERROR))
    .exit(span.clone())
    .enter(span.clone())
    .exit(span.clone())
    .close_span(span)
    .only()
    .run_with_handle();
  with_default(subscriber, || {
    let _result = block_on_future(async { err_async(2).await }).ok();
  });
  ensure_ok(handle.finished(), "mock expectations should finish")?;
  Ok(())
}

#[instrument(err)]
#[allow(
  clippy::single_call_fn,
  reason = "mutable err fixture remains a named instrumented function so error paths preserve mutation behavior"
)]
fn err_mut(out: &mut u8) -> Result<(), TryFromIntError> {
  *out = u8::try_from(1234)?;
  Ok(())
}

#[test]
fn test_mut() -> Result<(), TestFailure> {
  let span = expect::span().named("err_mut");
  let (subscriber, handle) = subscriber::mock()
    .new_span(span.clone())
    .enter(span.clone())
    .event(expect::event().at_level(Level::ERROR))
    .exit(span.clone())
    .close_span(span)
    .only()
    .run_with_handle();
  let _result = with_default(subscriber, || err_mut(&mut 0).ok());
  ensure_ok(handle.finished(), "mock expectations should finish")?;
  Ok(())
}

#[instrument(err)]
#[allow(
  clippy::single_call_fn,
  reason = "async mutable err fixture remains a named instrumented function so awaited mutation errors are asserted"
)]
async fn err_mut_async(polls: usize, out: &mut u8) -> Result<(), TryFromIntError> {
  let future = PollN::new_ok(polls);
  tracing::trace!(awaiting = true);
  let _result = future.await.ok();
  *out = u8::try_from(1234)?;
  Ok(())
}

#[test]
fn test_mut_async() -> Result<(), TestFailure> {
  let span = expect::span().named("err_mut_async");
  let (subscriber, handle) = subscriber::mock()
    .new_span(span.clone())
    .enter(span.clone())
    .event(
      expect::event()
        .with_fields(expect::field("awaiting").with_value(&true))
        .at_level(Level::TRACE),
    )
    .exit(span.clone())
    .enter(span.clone())
    .event(expect::event().at_level(Level::ERROR))
    .exit(span.clone())
    .enter(span.clone())
    .exit(span.clone())
    .close_span(span)
    .only()
    .run_with_handle();
  with_default(subscriber, || {
    let _result = block_on_future(async { err_mut_async(2, &mut 0).await }).ok();
  });
  ensure_ok(handle.finished(), "mock expectations should finish")?;
  Ok(())
}

#[test]
fn impl_trait_return_type() -> Result<(), TestFailure> {
  // Reproduces https://github.com/tokio-rs/tracing/issues/1227

  #[instrument(err)]
  fn returns_impl_trait(x: usize) -> Result<impl Iterator<Item = usize>, String> {
    Ok(0..x)
  }

  let span = expect::span().named("returns_impl_trait");

  let (subscriber, handle) = subscriber::mock()
    .new_span(span.clone().with_fields(expect::field("x").with_value(&10_usize).only()))
    .enter(span.clone())
    .exit(span.clone())
    .close_span(span)
    .only()
    .run_with_handle();

  with_default(subscriber, || {
    let Ok(values) = returns_impl_trait(10) else {
      return ensure(false, "instrumented impl Trait result should be Ok");
    };
    for _ in values {
      // nop
    }
    Ok(())
  })?;

  ensure_ok(handle.finished(), "mock expectations should finish")?;
  Ok(())
}

#[instrument(err(Debug))]
#[allow(
  clippy::single_call_fn,
  reason = "debug err fixture remains a named instrumented function so debug error formatting is asserted"
)]
fn err_dbg() -> Result<u8, TryFromIntError> {
  u8::try_from(1234)
}

#[test]
fn test_err_dbg() -> Result<(), TestFailure> {
  let Err(expected_error) = u8::try_from(1234) else {
    return ensure(false, "1234 should not fit in u8");
  };
  let span = expect::span().named("err_dbg");
  let (subscriber, handle) = subscriber::mock()
    .new_span(span.clone())
    .enter(span.clone())
    .event(expect::event().at_level(Level::ERROR).with_fields(expect::field("error")
                    // use the actual error value that will be emitted, so
                    // that this test doesn't break if the standard library
                    // changes the `fmt::Debug` output from the error type
                    // in the future.
                    .with_value(&debug(expected_error))))
    .exit(span.clone())
    .close_span(span)
    .only()
    .run_with_handle();
  let _result = with_default(subscriber, || err_dbg().ok());
  ensure_ok(handle.finished(), "mock expectations should finish")?;
  Ok(())
}

#[test]
fn test_err_display_default() -> Result<(), TestFailure> {
  let Err(expected_error) = u8::try_from(1234) else {
    return ensure(false, "1234 should not fit in u8");
  };
  let span = expect::span().named("err");
  let (subscriber, handle) = subscriber::mock()
    .new_span(span.clone())
    .enter(span.clone())
    .event(expect::event().at_level(Level::ERROR).with_fields(expect::field("error")
                    // by default, errors will be emitted with their display values
                    .with_value(&display(expected_error))))
    .exit(span.clone())
    .close_span(span)
    .only()
    .run_with_handle();
  let _result = with_default(subscriber, || err().ok());
  ensure_ok(handle.finished(), "mock expectations should finish")?;
  Ok(())
}

#[test]
fn test_err_custom_target() -> Result<(), TestFailure> {
  let filter: EnvFilter = ensure_ok("my_target=error".parse(), "filter should parse")?;
  let span = expect::span().named("error_span").with_target("my_target");

  let (subscriber, handle) = subscriber::mock()
    .new_span(span.clone())
    .enter(span.clone())
    .event(expect::event().at_level(Level::ERROR).with_target("my_target"))
    .exit(span.clone())
    .close_span(span)
    .only()
    .run_with_handle();

  let filtered_subscriber = subscriber.with(filter);

  with_default(filtered_subscriber, || {
    let error_span = tracing::error_span!(target: "my_target", "error_span");

    {
      let _enter = error_span.enter();
      tracing::error!(target: "my_target", "This should display");
    }
  });
  ensure_ok(handle.finished(), "mock expectations should finish")?;
  Ok(())
}

#[instrument(err(level = "info"))]
#[allow(
  clippy::single_call_fn,
  reason = "info-level err fixture remains a named instrumented function so error level overrides are asserted"
)]
fn err_info() -> Result<u8, TryFromIntError> {
  u8::try_from(1234)
}

#[test]
fn test_err_info() -> Result<(), TestFailure> {
  let span = expect::span().named("err_info");
  let (subscriber, handle) = subscriber::mock()
    .new_span(span.clone())
    .enter(span.clone())
    .event(expect::event().at_level(Level::INFO))
    .exit(span.clone())
    .close_span(span)
    .only()
    .run_with_handle();
  let _result = with_default(subscriber, || err_info().ok());
  ensure_ok(handle.finished(), "mock expectations should finish")?;
  Ok(())
}

#[instrument(err(Debug, level = "info"))]
#[allow(
  clippy::single_call_fn,
  reason = "debug info err fixture remains a named instrumented function so format and level overrides are asserted"
)]
fn err_dbg_info() -> Result<u8, TryFromIntError> {
  u8::try_from(1234)
}

#[test]
fn test_err_dbg_info() -> Result<(), TestFailure> {
  let Err(expected_error) = u8::try_from(1234) else {
    return ensure(false, "1234 should not fit in u8");
  };
  let span = expect::span().named("err_dbg_info");
  let (subscriber, handle) = subscriber::mock()
    .new_span(span.clone())
    .enter(span.clone())
    .event(expect::event().at_level(Level::INFO).with_fields(expect::field("error")
                    // use the actual error value that will be emitted, so
                    // that this test doesn't break if the standard library
                    // changes the `fmt::Debug` output from the error type
                    // in the future.
                    .with_value(&debug(expected_error))))
    .exit(span.clone())
    .close_span(span)
    .only()
    .run_with_handle();
  let _result = with_default(subscriber, || err_dbg_info().ok());
  ensure_ok(handle.finished(), "mock expectations should finish")?;
  Ok(())
}

#[instrument(level = "warn", err(level = "info"))]
#[allow(
  clippy::single_call_fn,
  reason = "warn-span err fixture remains a named instrumented function so err level overrides span level"
)]
fn err_warn_info() -> Result<u8, TryFromIntError> {
  u8::try_from(1234)
}

#[test]
fn test_err_warn_info() -> Result<(), TestFailure> {
  let span = expect::span().named("err_warn_info").at_level(Level::WARN);
  let (subscriber, handle) = subscriber::mock()
    .new_span(span.clone())
    .enter(span.clone())
    .event(expect::event().at_level(Level::INFO))
    .exit(span.clone())
    .close_span(span)
    .only()
    .run_with_handle();
  let _result = with_default(subscriber, || err_warn_info().ok());
  ensure_ok(handle.finished(), "mock expectations should finish")?;
  Ok(())
}
