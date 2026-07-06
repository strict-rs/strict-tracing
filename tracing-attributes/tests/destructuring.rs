//! Example binary for tracing workspace checks.
#![cfg(test)]

use strict_test_support::TestFailure;
use strict_test_support::ensure_ok;
use tracing::subscriber::with_default;
use tracing_attributes::instrument;
use tracing_mock::*;

/// Pair destructured by instrumentation tests.
type Pair = (usize, usize);

/// Nested pair destructured by instrumentation tests.
type NestedPair = (Pair, Pair);

#[test]
fn destructure_tuples() -> Result<(), TestFailure> {
  #[instrument]
  fn my_fn((arg1, arg2): (usize, usize)) {}

  let span = expect::span().named("my_fn");

  let (subscriber, handle) = subscriber::mock()
    .new_span(
      span.clone().with_fields(
        expect::field("arg1")
          .with_value(&format_args!("1"))
          .and(expect::field("arg2").with_value(&format_args!("2")))
          .only(),
      ),
    )
    .enter(span.clone())
    .exit(span.clone())
    .close_span(span)
    .only()
    .run_with_handle();

  with_default(subscriber, || {
    my_fn((1, 2));
  });

  ensure_ok(handle.finished(), "mock expectations should finish")?;
  Ok(())
}

#[test]
fn destructure_nested_tuples() -> Result<(), TestFailure> {
  #[instrument]
  fn my_fn(((arg1, arg2), (arg3, arg4)): NestedPair) {}

  let span = expect::span().named("my_fn");

  let (subscriber, handle) = subscriber::mock()
    .new_span(
      span.clone().with_fields(
        expect::field("arg1")
          .with_value(&format_args!("1"))
          .and(expect::field("arg2").with_value(&format_args!("2")))
          .and(expect::field("arg3").with_value(&format_args!("3")))
          .and(expect::field("arg4").with_value(&format_args!("4")))
          .only(),
      ),
    )
    .enter(span.clone())
    .exit(span.clone())
    .close_span(span)
    .only()
    .run_with_handle();

  with_default(subscriber, || {
    my_fn(((1, 2), (3, 4)));
  });

  ensure_ok(handle.finished(), "mock expectations should finish")?;
  Ok(())
}

#[test]
fn destructure_refs() -> Result<(), TestFailure> {
  #[instrument]
  fn my_fn(&arg1: &[usize; 3]) {}

  let span = expect::span().named("my_fn");

  let (subscriber, handle) = subscriber::mock()
    .new_span(
      span
        .clone()
        .with_fields(expect::field("arg1").with_value(&format_args!("[1, 2, 3]")).only()),
    )
    .enter(span.clone())
    .exit(span.clone())
    .close_span(span)
    .only()
    .run_with_handle();

  with_default(subscriber, || {
    my_fn(&[1, 2, 3]);
  });

  ensure_ok(handle.finished(), "mock expectations should finish")?;
  Ok(())
}

#[test]
fn destructure_tuple_structs() -> Result<(), TestFailure> {
  struct Foo(usize, usize);

  #[instrument]
  fn my_fn(Foo(arg1, arg2): Foo) {}

  let span = expect::span().named("my_fn");

  let (subscriber, handle) = subscriber::mock()
    .new_span(
      span.clone().with_fields(
        expect::field("arg1")
          .with_value(&format_args!("1"))
          .and(expect::field("arg2").with_value(&format_args!("2")))
          .only(),
      ),
    )
    .enter(span.clone())
    .exit(span.clone())
    .close_span(span)
    .only()
    .run_with_handle();

  with_default(subscriber, || {
    my_fn(Foo(1, 2));
  });

  ensure_ok(handle.finished(), "mock expectations should finish")?;
  Ok(())
}

#[test]
fn destructure_structs() -> Result<(), TestFailure> {
  struct Foo {
    bar: usize,
    baz: usize,
  }

  #[instrument]
  fn my_fn(
    Foo {
      bar: arg1,
      baz: arg2,
    }: Foo,
  ) {
    let observed_args = format!("{arg1}{arg2}");
    drop(observed_args);
  }

  let span = expect::span().named("my_fn");

  let (subscriber, handle) = subscriber::mock()
    .new_span(
      span.clone().with_fields(
        expect::field("arg1")
          .with_value(&format_args!("1"))
          .and(expect::field("arg2").with_value(&format_args!("2")))
          .only(),
      ),
    )
    .enter(span.clone())
    .exit(span.clone())
    .close_span(span)
    .only()
    .run_with_handle();

  with_default(subscriber, || {
    my_fn(Foo {
      bar: 1, baz: 2
    });
  });

  ensure_ok(handle.finished(), "mock expectations should finish")?;
  Ok(())
}

#[test]
fn destructure_everything() -> Result<(), TestFailure> {
  struct Foo {
    bar: Bar,
    baz: (usize, usize),
    qux: NoDebug,
  }
  struct Bar((usize, usize));
  struct NoDebug;

  #[instrument]
  fn my_fn(
    &Foo {
      bar: Bar((arg1, arg2)),
      baz: (arg3, arg4),
      ..
    }: &Foo,
  ) {
    let observed_args = format!("{arg1}{arg2}{arg3}{arg4}");
    drop(observed_args);
  }

  let span = expect::span().named("my_fn");

  let (subscriber, handle) = subscriber::mock()
    .new_span(
      span.clone().with_fields(
        expect::field("arg1")
          .with_value(&format_args!("1"))
          .and(expect::field("arg2").with_value(&format_args!("2")))
          .and(expect::field("arg3").with_value(&format_args!("3")))
          .and(expect::field("arg4").with_value(&format_args!("4")))
          .only(),
      ),
    )
    .enter(span.clone())
    .exit(span.clone())
    .close_span(span)
    .only()
    .run_with_handle();

  with_default(subscriber, || {
    let foo = Foo {
      bar: Bar((1, 2)),
      baz: (3, 4),
      qux: NoDebug,
    };
    let observed_qux = format!("{:p}", &foo.qux); // to eliminate unused field warning
    drop(observed_qux);
    my_fn(&foo);
  });

  ensure_ok(handle.finished(), "mock expectations should finish")?;
  Ok(())
}
