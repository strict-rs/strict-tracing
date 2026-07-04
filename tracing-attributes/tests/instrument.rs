//! Example binary for tracing workspace checks.
#![cfg(test)]

use std::fmt::Debug;
use std::hint::black_box;

use strict_test_support::TestFailure;
use strict_test_support::ensure_eq;
use strict_test_support::ensure_ok;
use tracing::Level;
use tracing::subscriber::with_default;
use tracing_attributes::instrument;
use tracing_mock::*;

// Reproduces a compile error when an instrumented function body contains inner
// attributes (https://github.com/tokio-rs/tracing/issues/2294).
#[deny(unused_variables)]
#[instrument]
#[allow(
  clippy::single_call_fn,
  reason = "inner-attribute regression fixture must remain a named instrumented function item"
)]
fn repro_2294() {
  let observed_value = 42;
  let _observed = black_box(observed_value);
}

#[test]
fn repro_2294_runs() {
  repro_2294();
}

#[test]
fn override_everything() -> Result<(), TestFailure> {
  #[instrument(target = "my_target", level = "debug")]
  fn my_fn() {}

  #[instrument(level = Level::DEBUG, target = "my_target")]
  fn my_other_fn() {}

  let span = expect::span().named("my_fn").at_level(Level::DEBUG).with_target("my_target");
  let span2 = expect::span()
    .named("my_other_fn")
    .at_level(Level::DEBUG)
    .with_target("my_target");
  let (subscriber, handle) = subscriber::mock()
    .new_span(span.clone())
    .enter(span.clone())
    .exit(span.clone())
    .close_span(span)
    .new_span(span2.clone())
    .enter(span2.clone())
    .exit(span2.clone())
    .close_span(span2)
    .only()
    .run_with_handle();

  with_default(subscriber, || {
    my_fn();
    my_other_fn();
  });

  ensure_ok(handle.finished(), "mock expectations should finish")?;
  Ok(())
}

#[test]
fn fields() -> Result<(), TestFailure> {
  #[instrument(target = "my_target", level = "debug")]
  fn my_fn(arg1: usize, arg2: bool, arg3: String) {
    drop(arg3);
  }

  let span = expect::span().named("my_fn").at_level(Level::DEBUG).with_target("my_target");

  let span2 = expect::span().named("my_fn").at_level(Level::DEBUG).with_target("my_target");
  let (subscriber, handle) = subscriber::mock()
    .new_span(
      span.clone().with_fields(
        expect::field("arg1")
          .with_value(&2_usize)
          .and(expect::field("arg2").with_value(&false))
          .and(expect::field("arg3").with_value(&"Cool".to_owned()))
          .only(),
      ),
    )
    .enter(span.clone())
    .exit(span.clone())
    .close_span(span)
    .new_span(
      span2.clone().with_fields(
        expect::field("arg1")
          .with_value(&3_usize)
          .and(expect::field("arg2").with_value(&true))
          .and(expect::field("arg3").with_value(&"Still Cool".to_owned()))
          .only(),
      ),
    )
    .enter(span2.clone())
    .exit(span2.clone())
    .close_span(span2)
    .only()
    .run_with_handle();

  with_default(subscriber, || {
    my_fn(2, false, "Cool".to_owned());
    my_fn(3, true, "Still Cool".to_owned());
  });

  ensure_ok(handle.finished(), "mock expectations should finish")?;
  Ok(())
}

#[test]
fn skip() -> Result<(), TestFailure> {
  struct UnDebug;

  #[instrument(target = "my_target", level = "debug", skip(_arg2, _arg3))]
  fn my_fn(arg1: usize, _arg2: UnDebug, _arg3: UnDebug) {}

  #[instrument(target = "my_target", level = "debug", skip_all)]
  fn my_fn2(_arg1: usize, _arg2: UnDebug, _arg3: UnDebug) {}

  let span = expect::span().named("my_fn").at_level(Level::DEBUG).with_target("my_target");

  let span2 = expect::span().named("my_fn").at_level(Level::DEBUG).with_target("my_target");

  let span3 = expect::span().named("my_fn2").at_level(Level::DEBUG).with_target("my_target");

  let (subscriber, handle) = subscriber::mock()
    .new_span(span.clone().with_fields(expect::field("arg1").with_value(&2_usize).only()))
    .enter(span.clone())
    .exit(span.clone())
    .close_span(span)
    .new_span(span2.clone().with_fields(expect::field("arg1").with_value(&3_usize).only()))
    .enter(span2.clone())
    .exit(span2.clone())
    .close_span(span2)
    .new_span(span3.clone())
    .enter(span3.clone())
    .exit(span3.clone())
    .close_span(span3)
    .only()
    .run_with_handle();

  with_default(subscriber, || {
    my_fn(2, UnDebug, UnDebug);
    my_fn(3, UnDebug, UnDebug);
    my_fn2(2, UnDebug, UnDebug);
  });

  ensure_ok(handle.finished(), "mock expectations should finish")?;
  Ok(())
}

#[test]
fn generics() -> Result<(), TestFailure> {
  #[derive(Debug)]
  struct Foo;

  #[instrument]
  fn my_fn<S, T: Debug>(arg1: S, arg2: T)
  where
    S: Debug,
  {
  }

  let span = expect::span().named("my_fn");

  let (subscriber, handle) = subscriber::mock()
    .new_span(
      span.clone().with_fields(
        expect::field("arg1")
          .with_value(&format_args!("Foo"))
          .and(expect::field("arg2").with_value(&format_args!("false"))),
      ),
    )
    .enter(span.clone())
    .exit(span.clone())
    .close_span(span)
    .only()
    .run_with_handle();

  with_default(subscriber, || {
    my_fn(Foo, false);
  });

  ensure_ok(handle.finished(), "mock expectations should finish")?;
  Ok(())
}

#[test]
fn methods() -> Result<(), TestFailure> {
  #[derive(Debug)]
  struct Foo;

  impl Foo {
    #[instrument]
    fn my_fn(&self, arg1: usize) {}
  }

  let span = expect::span().named("my_fn");

  let (subscriber, handle) = subscriber::mock()
    .new_span(
      span.clone().with_fields(
        expect::field("self")
          .with_value(&format_args!("Foo"))
          .and(expect::field("arg1").with_value(&42_usize)),
      ),
    )
    .enter(span.clone())
    .exit(span.clone())
    .close_span(span)
    .only()
    .run_with_handle();

  with_default(subscriber, || {
    let foo = Foo;
    foo.my_fn(42);
  });

  ensure_ok(handle.finished(), "mock expectations should finish")?;
  Ok(())
}

#[test]
fn impl_trait_return_type() -> Result<(), TestFailure> {
  #[instrument]
  fn returns_impl_trait(x: usize) -> impl Iterator<Item = usize> {
    0..x
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
    for _ in returns_impl_trait(10) {
      // nop
    }
  });

  ensure_ok(handle.finished(), "mock expectations should finish")?;
  Ok(())
}

#[test]
fn name_ident() -> Result<(), TestFailure> {
  const MY_NAME: &str = "my_name";
  #[instrument(name = MY_NAME)]
  fn name() {}

  let span_name = expect::span().named(MY_NAME);

  let (subscriber, handle) = subscriber::mock()
    .new_span(span_name.clone())
    .enter(span_name.clone())
    .exit(span_name.clone())
    .close_span(span_name)
    .only()
    .run_with_handle();

  with_default(subscriber, || {
    name();
  });

  ensure_ok(handle.finished(), "mock expectations should finish")?;
  Ok(())
}

#[test]
fn target_ident() -> Result<(), TestFailure> {
  const MY_TARGET: &str = "my_target";

  #[instrument(target = MY_TARGET)]
  fn target() {}

  let span_target = expect::span().named("target").with_target(MY_TARGET);

  let (subscriber, handle) = subscriber::mock()
    .new_span(span_target.clone())
    .enter(span_target.clone())
    .exit(span_target.clone())
    .close_span(span_target)
    .only()
    .run_with_handle();

  with_default(subscriber, || {
    target();
  });

  ensure_ok(handle.finished(), "mock expectations should finish")?;
  Ok(())
}

#[test]
fn target_name_ident() -> Result<(), TestFailure> {
  const MY_NAME: &str = "my_name";
  const MY_TARGET: &str = "my_target";

  #[instrument(target = MY_TARGET, name = MY_NAME)]
  fn name_target() {}

  let span_name_target = expect::span().named(MY_NAME).with_target(MY_TARGET);

  let (subscriber, handle) = subscriber::mock()
    .new_span(span_name_target.clone())
    .enter(span_name_target.clone())
    .exit(span_name_target.clone())
    .close_span(span_name_target)
    .only()
    .run_with_handle();

  with_default(subscriber, || {
    name_target();
  });

  ensure_ok(handle.finished(), "mock expectations should finish")?;
  Ok(())
}

/// Fixture for user code that defines its own module named `tracing`.
pub mod user_tracing_module_regression {
  use tracing_attributes::instrument;

  use super::TestFailure;
  use super::ensure_eq;

  /// User-defined module whose name intentionally shadows the external crate.
  pub mod tracing {
    /// Return a visible sentinel from the user-defined `tracing` module.
    #[must_use]
    #[allow(
      clippy::single_call_fn,
      reason = "fake tracing module function remains callable to prove user module shadowing survives expansion"
    )]
    pub fn my_other_fn() -> &'static str {
      "test"
    }
  }

  #[test]
  fn user_tracing_module() -> Result<(), TestFailure> {
    use ::tracing::field::Empty;

    // Reproduces https://github.com/tokio-rs/tracing/issues/3119
    #[instrument(fields(f = Empty))]
    fn my_fn() -> Result<(), TestFailure> {
      ensure_eq(
        &tracing::my_other_fn(),
        &"test",
        "user-defined tracing module should remain visible",
      )
    }

    my_fn()
  }
}
