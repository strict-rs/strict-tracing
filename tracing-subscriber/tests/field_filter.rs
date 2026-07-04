//! Tests filtering by field values.
#![cfg(feature = "env-filter")]

#[cfg(test)]
mod tests {
  use strict_test_support::TestFailure;
  use strict_test_support::ensure_ok;
  use tracing::Level;
  use tracing::subscriber::with_default;
  use tracing::{
    self,
  };
  use tracing_mock::*;
  use tracing_subscriber::filter::EnvFilter;
  use tracing_subscriber::prelude::*;

  #[test]
  #[cfg_attr(
    not(flaky_tests),
    ignore = "field-filter expectations are flaky without the explicit flaky_tests cfg"
  )]
  fn field_filter_events() -> Result<(), TestFailure> {
    let filter: EnvFilter = ensure_ok("[{thing}]=debug".parse(), "field event filter parses")?;
    let (mock_subscriber, mock_handle) = subscriber::mock()
      .event(expect::event().at_level(Level::INFO).with_fields(expect::field("thing")))
      .event(expect::event().at_level(Level::DEBUG).with_fields(expect::field("thing")))
      .only()
      .run_with_handle();
    let subscriber = mock_subscriber.with(filter);

    with_default(subscriber, || {
      tracing::trace!(disabled = true);
      tracing::info!("also disabled");
      tracing::info!(thing = 1);
      tracing::debug!(thing = 2);
      tracing::trace!(thing = 3);
    });

    ensure_ok(mock_handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[test]
  #[cfg_attr(
    not(flaky_tests),
    ignore = "field-filter expectations are flaky without the explicit flaky_tests cfg"
  )]
  fn field_filter_spans() -> Result<(), TestFailure> {
    let filter: EnvFilter = ensure_ok("[{enabled=true}]=debug".parse(), "field span filter parses")?;
    let (mock_subscriber, mock_handle) = subscriber::mock()
      .enter(expect::span().named("span1"))
      .event(expect::event().at_level(Level::INFO).with_fields(expect::field("something")))
      .exit(expect::span().named("span1"))
      .enter(expect::span().named("span2"))
      .exit(expect::span().named("span2"))
      .enter(expect::span().named("span3"))
      .event(expect::event().at_level(Level::DEBUG).with_fields(expect::field("something")))
      .exit(expect::span().named("span3"))
      .only()
      .run_with_handle();
    let subscriber = mock_subscriber.with(filter);

    with_default(subscriber, || {
      tracing::trace!("disabled");
      tracing::info!("also disabled");
      tracing::info_span!("span1", enabled = true).in_scope(|| {
        tracing::info!(something = 1);
      });
      tracing::debug_span!("span2", enabled = false, foo = "hi").in_scope(|| {
        tracing::warn!(something = 2);
      });
      tracing::trace_span!("span3", enabled = true, answer = 42).in_scope(|| {
        tracing::debug!(something = 2);
      });
    });

    ensure_ok(mock_handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[test]
  fn record_after_created() -> Result<(), TestFailure> {
    let filter: EnvFilter = ensure_ok("[{enabled=true}]=debug".parse(), "record-after-create filter parses")?;
    let (mock_subscriber, mock_handle) = subscriber::mock()
      .enter(expect::span().named("span"))
      .exit(expect::span().named("span"))
      .record(expect::span().named("span"), expect::field("enabled").with_value(&true))
      .enter(expect::span().named("span"))
      .event(expect::event().at_level(Level::DEBUG))
      .exit(expect::span().named("span"))
      .only()
      .run_with_handle();
    let subscriber = mock_subscriber.with(filter);

    with_default(subscriber, || {
      let span = tracing::info_span!("span", enabled = false);
      span.in_scope(|| {
        tracing::debug!("i'm disabled!");
      });

      let _span = span.record("enabled", true);
      span.in_scope(|| {
        tracing::debug!("i'm enabled!");
      });

      tracing::debug!("i'm also disabled");
    });

    ensure_ok(mock_handle.finished(), "mock expectations should finish")?;
    Ok(())
  }
}
