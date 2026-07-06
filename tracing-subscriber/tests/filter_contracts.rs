//! Public filter contract coverage for `EnvFilter`.

#![cfg(feature = "env-filter")]

#[cfg(test)]
mod tests {
  use strict_test_support::TestFailure;
  use strict_test_support::ensure;
  use strict_test_support::ensure_contains;
  use strict_test_support::ensure_ok;
  use tracing::Level;
  use tracing::subscriber::with_default;
  use tracing_mock::expect;
  use tracing_mock::subscriber;
  use tracing_subscriber::filter::EnvFilter;
  use tracing_subscriber::filter::LevelFilter;
  use tracing_subscriber::prelude::*;

  #[test]
  fn valid_directive_accepts_matching_target_and_rejects_other_targets() -> Result<(), TestFailure> {
    let filter = ensure_ok(EnvFilter::try_new("filter_contract_target=info"), "target directive parses")?;
    let (mock_subscriber, handle) = subscriber::mock()
      .event(expect::event().at_level(Level::INFO).with_target("filter_contract_target"))
      .only()
      .run_with_handle();
    let subscriber = mock_subscriber.with(filter);

    with_default(subscriber, || {
      tracing::info!(target: "other_target", "disabled by target");
      tracing::info!(target: "filter_contract_target", "enabled by target");
      tracing::debug!(target: "filter_contract_target", "disabled by level");
    });

    ensure_ok(handle.finished(), "target directive enables only matching events")
  }

  #[test]
  fn invalid_directive_reports_the_bad_directive_text() -> Result<(), TestFailure> {
    let invalid = EnvFilter::try_new("filter_contract_target[broken");

    ensure(invalid.is_err(), "invalid directive is rejected")?;
    let parse_error = invalid.err().map(|error| error.to_string()).unwrap_or_default();
    ensure_contains(
      &parse_error,
      "invalid filter directive",
      "parse error identifies the invalid directive category",
    )
  }

  #[test]
  fn env_builder_uses_default_directive_only_for_empty_inputs() -> Result<(), TestFailure> {
    let builder = EnvFilter::builder().with_default_directive(LevelFilter::INFO.into());

    let defaulted = ensure_ok(builder.parse(""), "empty filter parses through default directive")?;
    let explicit = ensure_ok(
      builder.parse("filter_contract_target=error"),
      "explicit filter parses without using default directive",
    )?;

    ensure(defaulted.to_string() == "info", "empty input uses the default directive")?;
    ensure(
      explicit.to_string() == "filter_contract_target=error",
      "explicit input replaces the default directive",
    )
  }

  #[test]
  fn more_specific_directive_wins_over_less_specific_default() -> Result<(), TestFailure> {
    let filter = ensure_ok(
      EnvFilter::try_new("info,filter_contract_target=trace"),
      "specific trace directive parses",
    )?;
    let (mock_subscriber, handle) = subscriber::mock()
      .event(expect::event().at_level(Level::INFO).with_target("other_target"))
      .event(expect::event().at_level(Level::TRACE).with_target("filter_contract_target"))
      .only()
      .run_with_handle();
    let subscriber = mock_subscriber.with(filter);

    with_default(subscriber, || {
      tracing::info!(target: "other_target", "enabled by less-specific info");
      tracing::debug!(target: "other_target", "disabled by less-specific info");
      tracing::trace!(target: "filter_contract_target", "enabled by specific trace");
    });

    ensure_ok(handle.finished(), "more-specific directive overrides the default level")
  }

  #[test]
  fn field_matchers_accept_primitive_span_field_values() -> Result<(), TestFailure> {
    let filter = ensure_ok(
      EnvFilter::builder()
        .with_regex(false)
        .parse("[bool_span{enabled=true}]=debug,[i64_span{signed=-2}]=debug,[u64_span{unsigned=2}]=debug,[f64_span{float=1.5}]=debug"),
      "primitive field directives parse",
    )?;
    let (mock_subscriber, handle) = subscriber::mock()
      .enter("bool_span")
      .event(expect::event().at_level(Level::DEBUG).with_target("bool_target"))
      .exit("bool_span")
      .enter("i64_span")
      .event(expect::event().at_level(Level::DEBUG).with_target("i64_target"))
      .exit("i64_span")
      .enter("u64_span")
      .event(expect::event().at_level(Level::DEBUG).with_target("u64_target"))
      .exit("u64_span")
      .enter("f64_span")
      .event(expect::event().at_level(Level::DEBUG).with_target("f64_target"))
      .exit("f64_span")
      .only()
      .run_with_handle();
    let subscriber = mock_subscriber.with(filter);

    with_default(subscriber, || {
      let bool_guard = tracing::info_span!("bool_span", enabled = true).entered();
      tracing::debug!(target: "bool_target", "enabled by bool");
      drop(bool_guard);

      let i64_guard = tracing::info_span!("i64_span", signed = -2_i64).entered();
      tracing::debug!(target: "i64_target", "enabled by i64");
      drop(i64_guard);

      let u64_guard = tracing::info_span!("u64_span", unsigned = 2_u64).entered();
      tracing::debug!(target: "u64_target", "enabled by u64");
      drop(u64_guard);

      let f64_guard = tracing::info_span!("f64_span", float = 1.5_f64).entered();
      tracing::debug!(target: "f64_target", "enabled by f64");
      drop(f64_guard);
    });

    ensure_ok(handle.finished(), "primitive field filters enable matching spans")
  }

  #[test]
  fn field_matchers_reject_wrong_type_and_value() -> Result<(), TestFailure> {
    let filter = ensure_ok(
      EnvFilter::builder()
        .with_regex(false)
        .parse("[bool_span{enabled=true}]=debug,[name_span{name=\"alice\"}]=debug"),
      "field directives parse",
    )?;
    let (mock_subscriber, handle) = subscriber::mock()
      .enter("bool_span")
      .exit("bool_span")
      .enter("name_span")
      .exit("name_span")
      .enter("name_span")
      .event(expect::event().at_level(Level::DEBUG).with_target("name_target"))
      .exit("name_span")
      .only()
      .run_with_handle();
    let subscriber = mock_subscriber.with(filter);

    with_default(subscriber, || {
      let wrong_type_guard = tracing::info_span!("bool_span", enabled = "true").entered();
      tracing::debug!(target: "bool_target", "disabled by wrong type");
      drop(wrong_type_guard);

      let wrong_value_guard = tracing::info_span!("name_span", name = "alice-bob").entered();
      tracing::debug!(target: "name_target", "disabled by wrong exact value");
      drop(wrong_value_guard);

      let exact_guard = tracing::info_span!("name_span", name = "alice").entered();
      tracing::debug!(target: "name_target", "enabled by exact value");
      drop(exact_guard);
    });

    ensure_ok(handle.finished(), "field filters reject mismatched values and accept exact matches")
  }
}
