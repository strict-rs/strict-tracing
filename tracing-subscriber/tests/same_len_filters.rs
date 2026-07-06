//! Tests filters with equal-length directive strings.
// These tests include field filters with no targets, so they have to go in a
// separate file.
#![cfg(feature = "env-filter")]

use strict_test_support::TestFailure;
use strict_test_support::ensure_ok;
use tracing::Level;
use tracing::subscriber::with_default;
use tracing_mock::*;
use tracing_subscriber::filter::EnvFilter;
use tracing_subscriber::prelude::*;

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn same_length_targets() -> Result<(), TestFailure> {
    let filter: EnvFilter = ensure_ok("foo=trace,bar=trace".parse(), "same-length target filter parses")?;
    let (mock_subscriber, mock_handle) = subscriber::mock()
      .event(expect::event().at_level(Level::TRACE))
      .event(expect::event().at_level(Level::TRACE))
      .only()
      .run_with_handle();
    let subscriber = mock_subscriber.with(filter);

    with_default(subscriber, || {
      tracing::trace!(target: "foo", "foo");
      tracing::trace!(target: "bar", "bar");
    });

    ensure_ok(mock_handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[test]
  fn same_num_fields_event() -> Result<(), TestFailure> {
    let filter: EnvFilter = ensure_ok("[{foo}]=trace,[{bar}]=trace".parse(), "same-number field filter parses")?;
    let (mock_subscriber, mock_handle) = subscriber::mock()
      .event(expect::event().at_level(Level::TRACE).with_fields(expect::field("foo")))
      .event(expect::event().at_level(Level::TRACE).with_fields(expect::field("bar")))
      .only()
      .run_with_handle();
    let subscriber = mock_subscriber.with(filter);
    with_default(subscriber, || {
      tracing::trace!(foo = 1);
      tracing::trace!(bar = 3);
    });

    ensure_ok(mock_handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[test]
  fn same_num_fields_and_name_len() -> Result<(), TestFailure> {
    let filter: EnvFilter = ensure_ok(
      "[foo{bar=1}]=trace,[baz{boz=1}]=trace".parse(),
      "same-length field-and-name filter parses",
    )?;
    let (mock_subscriber, mock_handle) = subscriber::mock()
      .new_span(
        expect::span()
          .named("foo")
          .at_level(Level::TRACE)
          .with_fields(expect::field("bar")),
      )
      .new_span(
        expect::span()
          .named("baz")
          .at_level(Level::TRACE)
          .with_fields(expect::field("boz")),
      )
      .only()
      .run_with_handle();
    let subscriber = mock_subscriber.with(filter);
    with_default(subscriber, || {
      tracing::trace_span!("foo", bar = 1);
      tracing::trace_span!("baz", boz = 1);
    });

    ensure_ok(mock_handle.finished(), "mock expectations should finish")?;
    Ok(())
  }
}
