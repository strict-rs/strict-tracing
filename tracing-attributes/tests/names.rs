//! Example binary for tracing workspace checks.
#![cfg(test)]

use strict_test_support::TestFailure;
use strict_test_support::ensure_ok;
use tracing::subscriber::with_default;
use tracing_attributes::instrument;
use tracing_mock::*;

#[instrument]
#[allow(
  clippy::single_call_fn,
  reason = "name fixture remains a named function item so default span names can be asserted"
)]
fn default_name() {}

#[instrument(name = "my_name")]
#[allow(
  clippy::single_call_fn,
  reason = "name fixture remains a named function item so explicit span names can be asserted"
)]
fn custom_name() {}

// XXX: it's weird that we support both of these forms, but apparently we
// managed to release a version that accepts both syntax, so now we have to
// support it! yay!
#[instrument("my_other_name")]
#[allow(
  clippy::single_call_fn,
  reason = "legacy name fixture remains a named function item so positional span names can be asserted"
)]
fn custom_name_no_equals() {}

#[test]
fn default_name_test() -> Result<(), TestFailure> {
  let (subscriber, handle) = subscriber::mock()
    .new_span(expect::span().named("default_name"))
    .enter(expect::span().named("default_name"))
    .exit(expect::span().named("default_name"))
    .only()
    .run_with_handle();

  with_default(subscriber, || {
    default_name();
  });

  ensure_ok(handle.finished(), "mock expectations should finish")?;
  Ok(())
}

#[test]
fn custom_name_test() -> Result<(), TestFailure> {
  let (subscriber, handle) = subscriber::mock()
    .new_span(expect::span().named("my_name"))
    .enter(expect::span().named("my_name"))
    .exit(expect::span().named("my_name"))
    .only()
    .run_with_handle();

  with_default(subscriber, || {
    custom_name();
  });

  ensure_ok(handle.finished(), "mock expectations should finish")?;
  Ok(())
}

#[test]
fn custom_name_no_equals_test() -> Result<(), TestFailure> {
  let (subscriber, handle) = subscriber::mock()
    .new_span(expect::span().named("my_other_name"))
    .enter(expect::span().named("my_other_name"))
    .exit(expect::span().named("my_other_name"))
    .only()
    .run_with_handle();

  with_default(subscriber, || {
    custom_name_no_equals();
  });

  ensure_ok(handle.finished(), "mock expectations should finish")?;
  Ok(())
}
