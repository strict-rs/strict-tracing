//! Span API integration coverage.

#![cfg(feature = "std")]

#[cfg(test)]
mod tests {
  // These tests require the thread-local scoped dispatcher, which only works when
  // we have a standard library. The behaviour being tested should be the same
  // with the standard lib disabled.

  use std::convert::identity;
  use std::thread;

  use strict_test_support::TestFailure;
  use strict_test_support::ensure;
  use strict_test_support::ensure_ok;
  use tracing::Level;
  use tracing::Span;
  use tracing::error_span;
  use tracing::field::Empty;
  use tracing::field::debug;
  use tracing::field::display;
  use tracing::record_all;
  use tracing::subscriber::with_default;
  use tracing_mock::*;

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn handles_to_the_same_span_are_equal() -> Result<(), TestFailure> {
    // Create a mock subscriber that will return `true` on calls to
    // `Subscriber::enabled`, so that the spans will be constructed. We
    // won't enter any spans in this test, so the subscriber won't actually
    // expect to see any spans.
    with_default(subscriber::mock().run(), || {
      let foo1 = tracing::span!(Level::TRACE, "foo");

      // The purpose of this test is to assert that two clones of the same
      // span are equal, so the clone here is kind of the whole point :)
      let foo2 = foo1.clone();

      // Two handles that point to the same span are equal.
      ensure(foo1 == foo2, "two handles to the same span are equal")
    })
  }

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn handles_to_different_spans_are_not_equal() -> Result<(), TestFailure> {
    with_default(subscriber::mock().run(), || {
      // Even though these spans have the same name and fields, they will have
      // differing metadata, since they were created on different lines.
      let foo1 = tracing::span!(Level::TRACE, "foo", bar = 1_u64, baz = false);
      let foo2 = tracing::span!(Level::TRACE, "foo", bar = 1_u64, baz = false);

      ensure(foo1 != foo2, "different spans are not equal")
    })
  }

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn handles_to_different_spans_with_the_same_metadata_are_not_equal() -> Result<(), TestFailure> {
    // Every time time this function is called, it will return a _new
    // instance_ of a span with the same metadata, name, and fields.
    fn make_span() -> Span {
      tracing::span!(Level::TRACE, "foo", bar = 1_u64, baz = false)
    }

    with_default(subscriber::mock().run(), || {
      let foo1 = make_span();
      let foo2 = make_span();

      ensure(foo1 != foo2, "different span instances with identical metadata are not equal")
    })
  }

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn spans_always_go_to_the_subscriber_that_tagged_them() {
    let subscriber1 = subscriber::mock()
      .enter(expect::span().named("foo"))
      .exit(expect::span().named("foo"))
      .enter(expect::span().named("foo"))
      .exit(expect::span().named("foo"))
      .close_span(expect::span().named("foo"))
      .only()
      .run();
    let subscriber2 = subscriber::mock().run();

    let foo = with_default(subscriber1, || {
      let foo = tracing::span!(Level::TRACE, "foo");
      foo.in_scope(|| {});
      foo
    });
    // Even though we enter subscriber 2's context, the subscriber that
    // tagged the span should see the enter/exit.
    with_default(subscriber2, move || foo.in_scope(|| {}));
  }

  // This gets exempt from testing in wasm because of: `thread::spawn` which is
  // not yet possible to do in WASM. There is work going on see:
  // <https://rustwasm.github.io/2018/10/24/multithreading-rust-and-wasm.html>
  //
  // But for now since it's not possible we don't need to test for it :)
  #[test]
  fn spans_always_go_to_the_subscriber_that_tagged_them_even_across_threads() -> Result<(), TestFailure> {
    let subscriber1 = subscriber::mock()
      .enter(expect::span().named("foo"))
      .exit(expect::span().named("foo"))
      .enter(expect::span().named("foo"))
      .exit(expect::span().named("foo"))
      .close_span(expect::span().named("foo"))
      .only()
      .run();
    let foo = with_default(subscriber1, || {
      let foo = tracing::span!(Level::TRACE, "foo");
      foo.in_scope(|| {});
      foo
    });

    // Even though we enter subscriber 2's context, the subscriber that
    // tagged the span should see the enter/exit.
    thread::spawn(move || {
      with_default(subscriber::mock().run(), || {
        foo.in_scope(|| {});
      });
    })
    .join()
    .map_or_else(|_panic| ensure(false, "subscriber thread should join"), |()| Ok(()))
  }

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn dropping_a_span_closes_span() -> Result<(), TestFailure> {
    let (subscriber, handle) = subscriber::mock()
      .enter(expect::span().named("foo"))
      .exit(expect::span().named("foo"))
      .close_span(expect::span().named("foo"))
      .only()
      .run_with_handle();
    with_default(subscriber, || {
      let span = tracing::span!(Level::TRACE, "foo");
      span.in_scope(|| {});
      drop(span);
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn span_closes_after_event() -> Result<(), TestFailure> {
    let (subscriber, handle) = subscriber::mock()
      .enter(expect::span().named("foo"))
      .event(expect::event())
      .exit(expect::span().named("foo"))
      .close_span(expect::span().named("foo"))
      .only()
      .run_with_handle();
    with_default(subscriber, || {
      tracing::span!(Level::TRACE, "foo").in_scope(|| {
        tracing::event!(Level::DEBUG, {}, "my tracing::event!");
      });
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn new_span_after_event() -> Result<(), TestFailure> {
    let (subscriber, handle) = subscriber::mock()
      .enter(expect::span().named("foo"))
      .event(expect::event())
      .exit(expect::span().named("foo"))
      .close_span(expect::span().named("foo"))
      .enter(expect::span().named("bar"))
      .exit(expect::span().named("bar"))
      .close_span(expect::span().named("bar"))
      .only()
      .run_with_handle();
    with_default(subscriber, || {
      tracing::span!(Level::TRACE, "foo").in_scope(|| {
        tracing::event!(Level::DEBUG, {}, "my tracing::event!");
      });
      tracing::span!(Level::TRACE, "bar").in_scope(|| {});
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn event_outside_of_span() -> Result<(), TestFailure> {
    let (subscriber, handle) = subscriber::mock()
      .event(expect::event())
      .enter(expect::span().named("foo"))
      .exit(expect::span().named("foo"))
      .close_span(expect::span().named("foo"))
      .only()
      .run_with_handle();
    with_default(subscriber, || {
      tracing::debug!("my tracing::event!");
      tracing::span!(Level::TRACE, "foo").in_scope(|| {});
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn cloning_a_span_calls_clone_span() -> Result<(), TestFailure> {
    let (subscriber, handle) = subscriber::mock().clone_span(expect::span().named("foo")).run_with_handle();
    with_default(subscriber, || {
      let span = tracing::span!(Level::TRACE, "foo");
      // Allow the "redundant" `.clone` since it is used to call into the `.clone_span` hook.
      let cloned_span = Clone::clone(&span);
      drop(cloned_span);
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn close_span_when_exiting_dispatchers_context() -> Result<(), TestFailure> {
    let (subscriber, handle) = subscriber::mock()
      .clone_span(expect::span().named("foo"))
      .close_span(expect::span().named("foo"))
      .event(expect::event())
      .run_with_handle();
    with_default(subscriber, || {
      let span = tracing::span!(Level::TRACE, "foo");
      let span2 = span.clone();
      drop(span);
      drop(span2);
      tracing::debug!("after final close");
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn clone_and_close_span_always_go_to_the_subscriber_that_tagged_the_span() -> Result<(), TestFailure> {
    let (subscriber1, handle1) = subscriber::mock()
      .enter(expect::span().named("foo"))
      .exit(expect::span().named("foo"))
      .clone_span(expect::span().named("foo"))
      .enter(expect::span().named("foo"))
      .exit(expect::span().named("foo"))
      .close_span(expect::span().named("foo"))
      .run_with_handle();
    let subscriber2 = subscriber::mock().only().run();

    let foo = with_default(subscriber1, || {
      let foo = tracing::span!(Level::TRACE, "foo");
      foo.in_scope(|| {});
      foo
    });
    // Even though we enter subscriber 2's context, the subscriber that
    // tagged the span should see the enter/exit.
    with_default(subscriber2, move || {
      let foo2 = foo.clone();
      foo.in_scope(|| {});
      drop(foo);
      drop(foo2);
    });

    ensure_ok(handle1.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn span_closes_when_exited() -> Result<(), TestFailure> {
    let (subscriber, handle) = subscriber::mock()
      .enter(expect::span().named("foo"))
      .exit(expect::span().named("foo"))
      .close_span(expect::span().named("foo"))
      .only()
      .run_with_handle();
    with_default(subscriber, || {
      let foo = tracing::span!(Level::TRACE, "foo");

      foo.in_scope(|| {});

      drop(foo);
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn enter() -> Result<(), TestFailure> {
    let (subscriber, handle) = subscriber::mock()
      .enter(expect::span().named("foo"))
      .event(expect::event())
      .exit(expect::span().named("foo"))
      .close_span(expect::span().named("foo"))
      .only()
      .run_with_handle();
    with_default(subscriber, || {
      let foo = tracing::span!(Level::TRACE, "foo");
      let _enter = foo.enter();
      tracing::debug!("dropping guard...");
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn entered() -> Result<(), TestFailure> {
    let (subscriber, handle) = subscriber::mock()
      .enter(expect::span().named("foo"))
      .event(expect::event())
      .exit(expect::span().named("foo"))
      .close_span(expect::span().named("foo"))
      .only()
      .run_with_handle();
    with_default(subscriber, || {
      let _span = tracing::span!(Level::TRACE, "foo").entered();
      tracing::debug!("dropping guard...");
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn entered_api() -> Result<(), TestFailure> {
    let (subscriber, handle) = subscriber::mock()
      .enter(expect::span().named("foo"))
      .event(expect::event())
      .exit(expect::span().named("foo"))
      .close_span(expect::span().named("foo"))
      .only()
      .run_with_handle();
    with_default(subscriber, || {
      let span = tracing::span!(Level::TRACE, "foo").entered();
      let _derefs_to_span = span.id();
      tracing::debug!("exiting span...");
      let _span = span.exit();
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn moved_field() -> Result<(), TestFailure> {
    let (subscriber, handle) = subscriber::mock()
      .new_span(
        expect::span()
          .named("foo")
          .with_fields(expect::field("bar").with_value(&display("hello from my span")).only()),
      )
      .enter(expect::span().named("foo"))
      .exit(expect::span().named("foo"))
      .close_span(expect::span().named("foo"))
      .only()
      .run_with_handle();
    with_default(subscriber, || {
      let from = "my span";
      let span = tracing::span!(Level::TRACE, "foo", bar = display(format!("hello from {from}")));
      span.in_scope(|| {});
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn dotted_field_name() -> Result<(), TestFailure> {
    let (subscriber, handle) = subscriber::mock()
      .new_span(
        expect::span()
          .named("foo")
          .with_fields(expect::field("fields.bar").with_value(&true).only()),
      )
      .only()
      .run_with_handle();
    with_default(subscriber, || {
      tracing::span!(Level::TRACE, "foo", fields.bar = true);
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn borrowed_field() -> Result<(), TestFailure> {
    let (subscriber, handle) = subscriber::mock()
      .new_span(
        expect::span()
          .named("foo")
          .with_fields(expect::field("bar").with_value(&display("hello from my span")).only()),
      )
      .enter(expect::span().named("foo"))
      .exit(expect::span().named("foo"))
      .close_span(expect::span().named("foo"))
      .only()
      .run_with_handle();

    with_default(subscriber, || {
      let from = "my span";
      let mut message = format!("hello from {from}");
      let span = tracing::span!(Level::TRACE, "foo", bar = display(&message));
      span.in_scope(|| {
        message.push_str(" inside");
      });
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  // If emitting log instrumentation, this gets moved anyway, breaking the test.
  #[cfg(not(feature = "log"))]
  fn move_field_out_of_struct() -> Result<(), TestFailure> {
    use tracing::field::debug;

    #[derive(Debug)]
    struct Position {
      x: f32,
      y: f32,
    }

    let expected_pos = Position {
      x: 3.234, y: -1.223
    };
    let (subscriber, handle) = subscriber::mock()
      .new_span(
        expect::span().named("foo").with_fields(
          expect::field("x")
            .with_value(&debug(3.234))
            .and(expect::field("y").with_value(&debug(-1.223)))
            .only(),
        ),
      )
      .new_span(
        expect::span()
          .named("bar")
          .with_fields(expect::field("position").with_value(&debug(&expected_pos)).only()),
      )
      .run_with_handle();

    with_default(subscriber, || {
      let pos = Position {
        x: 3.234, y: -1.223
      };
      let foo = tracing::span!(Level::TRACE, "foo", x = debug(pos.x), y = debug(pos.y));
      let bar = tracing::span!(Level::TRACE, "bar", position = debug(pos));
      foo.in_scope(|| {});
      bar.in_scope(|| {});
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn float_values() -> Result<(), TestFailure> {
    let (subscriber, handle) = subscriber::mock()
      .new_span(
        expect::span().named("foo").with_fields(
          expect::field("x")
            .with_value(&3.234)
            .and(expect::field("y").with_value(&-1.223))
            .only(),
        ),
      )
      .run_with_handle();

    with_default(subscriber, || {
      let foo = tracing::span!(Level::TRACE, "foo", x = 3.234, y = -1.223);
      foo.in_scope(|| {});
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  // TODO(#1138): determine a new syntax for uninitialized span fields, and
  // re-enable these.
  // #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  // #[test]
  // fn add_field_after_new_span() {
  // let (subscriber, handle) = subscriber::mock()
  // .new_span(
  // expect::span()
  // .named("foo")
  // .with_fields(expect::field("bar").with_value(&5)
  // .and(expect::field("baz").with_value).only()),
  // )
  // .record(
  // expect::span().named("foo"),
  // field::expect("baz").with_value(&true).only(),
  // )
  // .enter(expect::span().named("foo"))
  // .exit(expect::span().named("foo"))
  // .close_span(expect::span().named("foo"))
  // .only()
  // .run_with_handle();
  //
  // with_default(subscriber, || {
  // let span = tracing::span!(Level::TRACE, "foo", bar = 5, baz = false);
  // span.record("baz", &true);
  // span.in_scope(|| {});
  // });
  //
  // ensure_ok(handle.finished(), "mock expectations should finish")?;
  // }
  //
  // #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  // #[test]
  // fn add_fields_only_after_new_span() {
  // let (subscriber, handle) = subscriber::mock()
  // .new_span(expect::span().named("foo"))
  // .record(
  // expect::span().named("foo"),
  // field::expect("bar").with_value(&5).only(),
  // )
  // .record(
  // expect::span().named("foo"),
  // field::expect("baz").with_value(&true).only(),
  // )
  // .enter(expect::span().named("foo"))
  // .exit(expect::span().named("foo"))
  // .close_span(expect::span().named("foo"))
  // .only()
  // .run_with_handle();
  //
  // with_default(subscriber, || {
  // let span = tracing::span!(Level::TRACE, "foo", bar = _, baz = _);
  // span.record("bar", &5);
  // span.record("baz", &true);
  // span.in_scope(|| {});
  // });
  //
  // ensure_ok(handle.finished(), "mock expectations should finish")?;
  // }

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn record_new_value_for_field() -> Result<(), TestFailure> {
    let (subscriber, handle) = subscriber::mock()
      .new_span(
        expect::span().named("foo").with_fields(
          expect::field("bar")
            .with_value(&5)
            .and(expect::field("baz").with_value(&false))
            .only(),
        ),
      )
      .record(expect::span().named("foo"), expect::field("baz").with_value(&true).only())
      .enter(expect::span().named("foo"))
      .exit(expect::span().named("foo"))
      .close_span(expect::span().named("foo"))
      .only()
      .run_with_handle();

    with_default(subscriber, || {
      let span = tracing::span!(Level::TRACE, "foo", bar = 5, baz = false);
      let _baz_recorded_span = span.record("baz", true);
      span.in_scope(|| {});
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn record_new_values_for_fields() -> Result<(), TestFailure> {
    let (subscriber, handle) = subscriber::mock()
      .new_span(
        expect::span().named("foo").with_fields(
          expect::field("bar")
            .with_value(&4)
            .and(expect::field("baz").with_value(&false))
            .only(),
        ),
      )
      .record(expect::span().named("foo"), expect::field("bar").with_value(&5).only())
      .record(expect::span().named("foo"), expect::field("baz").with_value(&true).only())
      .enter(expect::span().named("foo"))
      .exit(expect::span().named("foo"))
      .close_span(expect::span().named("foo"))
      .only()
      .run_with_handle();

    with_default(subscriber, || {
      let span = tracing::span!(Level::TRACE, "foo", bar = 4, baz = false);
      let _bar_recorded_span = span.record("bar", 5);
      let _baz_recorded_span = span.record("baz", true);
      span.in_scope(|| {});
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  // Tests the `record_all!` macro, which is a wrapper for `Span::record_all`.
  // Placed here instead of `tests/macros.rs`, because it uses `tracing_mock`,
  // which requires the standard library. Other macro tests exclude the standard
  // library to verify the macros do not depend on it.
  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn record_all_macro_records_new_values_for_fields() -> Result<(), TestFailure> {
    let (subscriber, handle) = subscriber::mock()
      .new_span(expect::span().named("foo").with_fields(expect::field("bar")))
      .record(
        expect::span().named("foo"),
        expect::field("bar")
          .with_value(&5)
          .and(expect::field("qux").with_value(&display("qux")))
          .and(expect::field("quux").with_value(&debug("QuuX")))
          .only(),
      )
      .enter(expect::span().named("foo"))
      .exit(expect::span().named("foo"))
      .close_span(expect::span().named("foo"))
      .only()
      .run_with_handle();

    with_default(subscriber, || {
      let span = tracing::span!(Level::TRACE, "foo", bar = 1, baz = 2, qux = Empty, quux = Empty);
      record_all!(span, bar = 5, qux = %"qux", quux = ?"QuuX", unknown = "unknown");
      span.in_scope(|| {});
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn record_all_macro_records_all_fields() -> Result<(), TestFailure> {
    let (subscriber, handle) = subscriber::mock()
      .new_span(expect::span().named("foo").with_fields(expect::field("bar")))
      .record(
        expect::span().named("foo"),
        expect::field("bar")
          .with_value(&5)
          .and(expect::field("baz").with_value(&6))
          .and(expect::field("qux").with_value(&display("qux")))
          .and(expect::field("quux").with_value(&debug("QuuX")))
          .only(),
      )
      .enter(expect::span().named("foo"))
      .exit(expect::span().named("foo"))
      .close_span(expect::span().named("foo"))
      .only()
      .run_with_handle();

    with_default(subscriber, || {
      let span = tracing::span!(Level::TRACE, "foo", bar = 1, baz = 2, qux = Empty, quux = Empty);
      record_all!(span, bar = 5, baz = 6, qux = %"qux", quux = ?"QuuX");
      span.in_scope(|| {});
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn record_all_macro_records_all_fields_different_order() -> Result<(), TestFailure> {
    let (subscriber, handle) = subscriber::mock()
      .new_span(expect::span().named("foo").with_fields(expect::field("bar")))
      .record(
        expect::span().named("foo"),
        expect::field("bar")
          .with_value(&5)
          .and(expect::field("baz").with_value(&6))
          .and(expect::field("qux").with_value(&display("qux")))
          .and(expect::field("quux").with_value(&debug("QuuX")))
          .only(),
      )
      .enter(expect::span().named("foo"))
      .exit(expect::span().named("foo"))
      .close_span(expect::span().named("foo"))
      .only()
      .run_with_handle();

    with_default(subscriber, || {
      let span = tracing::span!(Level::TRACE, "foo", bar = 1, baz = 2, qux = Empty, quux = Empty);
      record_all!(span, qux = %"qux", baz = 6, bar = 5, quux = ?"QuuX");
      span.in_scope(|| {});
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn record_all_macro_unknown_field() -> Result<(), TestFailure> {
    let (subscriber, handle) = subscriber::mock()
      .new_span(expect::span().named("foo").with_fields(expect::field("bar")))
      .record(expect::span().named("foo"), field::ExpectedFields::default().only())
      .enter(expect::span().named("foo"))
      .exit(expect::span().named("foo"))
      .close_span(expect::span().named("foo"))
      .only()
      .run_with_handle();

    with_default(subscriber, || {
      let span = tracing::span!(Level::TRACE, "foo", bar = 1, baz = 2, qux = Empty, quux = Empty);
      record_all!(span, unknown = "unknown");
      span.in_scope(|| {});
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn new_span_with_target_and_log_level() -> Result<(), TestFailure> {
    let (subscriber, handle) = subscriber::mock()
      .new_span(expect::span().named("foo").with_target("app_span").at_level(Level::DEBUG))
      .only()
      .run_with_handle();

    with_default(subscriber, || {
      tracing::span!(target: "app_span", Level::DEBUG, "foo");
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn explicit_root_span_is_root() -> Result<(), TestFailure> {
    let (subscriber, handle) = subscriber::mock()
      .new_span(expect::span().named("foo").with_ancestry(expect::is_explicit_root()))
      .only()
      .run_with_handle();

    with_default(subscriber, || {
      tracing::span!(parent: None, Level::TRACE, "foo");
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn explicit_root_span_is_root_regardless_of_ctx() -> Result<(), TestFailure> {
    let (subscriber, handle) = subscriber::mock()
      .new_span(expect::span().named("foo"))
      .enter(expect::span().named("foo"))
      .new_span(expect::span().named("bar").with_ancestry(expect::is_explicit_root()))
      .exit(expect::span().named("foo"))
      .only()
      .run_with_handle();

    with_default(subscriber, || {
      tracing::span!(Level::TRACE, "foo").in_scope(|| {
        tracing::span!(parent: None, Level::TRACE, "bar");
      });
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn explicit_child() -> Result<(), TestFailure> {
    let (subscriber, handle) = subscriber::mock()
      .new_span(expect::span().named("foo"))
      .new_span(expect::span().named("bar").with_ancestry(expect::has_explicit_parent("foo")))
      .only()
      .run_with_handle();

    with_default(subscriber, || {
      let foo = tracing::span!(Level::TRACE, "foo");
      tracing::span!(parent: foo.id(), Level::TRACE, "bar");
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn explicit_child_at_levels() -> Result<(), TestFailure> {
    let (subscriber, handle) = subscriber::mock()
      .new_span(expect::span().named("foo"))
      .new_span(expect::span().named("a").with_ancestry(expect::has_explicit_parent("foo")))
      .new_span(expect::span().named("b").with_ancestry(expect::has_explicit_parent("foo")))
      .new_span(expect::span().named("c").with_ancestry(expect::has_explicit_parent("foo")))
      .new_span(expect::span().named("d").with_ancestry(expect::has_explicit_parent("foo")))
      .new_span(expect::span().named("e").with_ancestry(expect::has_explicit_parent("foo")))
      .only()
      .run_with_handle();

    with_default(subscriber, || {
      let foo = tracing::span!(Level::TRACE, "foo");
      tracing::trace_span!(parent: foo.id(), "a");
      tracing::debug_span!(parent: foo.id(), "b");
      tracing::info_span!(parent: foo.id(), "c");
      tracing::warn_span!(parent: foo.id(), "d");
      tracing::error_span!(parent: foo.id(), "e");
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn explicit_child_regardless_of_ctx() -> Result<(), TestFailure> {
    let (subscriber, handle) = subscriber::mock()
      .new_span(expect::span().named("foo"))
      .new_span(expect::span().named("bar"))
      .enter(expect::span().named("bar"))
      .new_span(expect::span().named("baz").with_ancestry(expect::has_explicit_parent("foo")))
      .exit(expect::span().named("bar"))
      .only()
      .run_with_handle();

    with_default(subscriber, || {
      let foo = tracing::span!(Level::TRACE, "foo");
      let _span = tracing::span!(Level::TRACE, "bar").in_scope(|| tracing::span!(parent: foo.id(), Level::TRACE, "baz"));
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn contextual_root() -> Result<(), TestFailure> {
    let (subscriber, handle) = subscriber::mock()
      .new_span(expect::span().named("foo").with_ancestry(expect::is_contextual_root()))
      .only()
      .run_with_handle();

    with_default(subscriber, || {
      tracing::span!(Level::TRACE, "foo");
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn contextual_child() -> Result<(), TestFailure> {
    let (subscriber, handle) = subscriber::mock()
      .new_span(expect::span().named("foo"))
      .enter(expect::span().named("foo"))
      .new_span(expect::span().named("bar").with_ancestry(expect::has_contextual_parent("foo")))
      .exit(expect::span().named("foo"))
      .only()
      .run_with_handle();

    with_default(subscriber, || {
      tracing::span!(Level::TRACE, "foo").in_scope(|| {
        tracing::span!(Level::TRACE, "bar");
      });
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn display_shorthand() -> Result<(), TestFailure> {
    let (subscriber, handle) = subscriber::mock()
      .new_span(
        expect::span()
          .named("my_span")
          .with_fields(expect::field("my_field").with_value(&display("hello world")).only()),
      )
      .only()
      .run_with_handle();
    with_default(subscriber, || {
      tracing::span!(Level::TRACE, "my_span", my_field = %"hello world");
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn debug_shorthand() -> Result<(), TestFailure> {
    let (subscriber, handle) = subscriber::mock()
      .new_span(
        expect::span()
          .named("my_span")
          .with_fields(expect::field("my_field").with_value(&debug("hello world")).only()),
      )
      .only()
      .run_with_handle();
    with_default(subscriber, || {
      tracing::span!(Level::TRACE, "my_span", my_field = ?"hello world");
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn both_shorthands() -> Result<(), TestFailure> {
    let (subscriber, handle) = subscriber::mock()
      .new_span(
        expect::span().named("my_span").with_fields(
          expect::field("display_field")
            .with_value(&display("hello world"))
            .and(expect::field("debug_field").with_value(&debug("hello world")))
            .only(),
        ),
      )
      .only()
      .run_with_handle();
    with_default(subscriber, || {
      tracing::span!(Level::TRACE, "my_span", display_field = %"hello world", debug_field = ?"hello world");
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn constant_field_name() -> Result<(), TestFailure> {
    let (subscriber, handle) = subscriber::mock()
      .new_span(
        expect::span().named("my_span").with_fields(
          expect::field("foo")
            .with_value(&"bar")
            .and(expect::field("constant string").with_value(&"also works"))
            .and(expect::field("foo.bar").with_value(&"baz"))
            .only(),
        ),
      )
      .only()
      .run_with_handle();

    with_default(subscriber, || {
      const FOO: &str = "foo";
      tracing::span!(
        Level::TRACE,
        "my_span",
        { identity(FOO) } = "bar",
        { "constant string" } = "also works",
        foo.bar = "baz",
      );
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn keyword_ident_in_field_name_span_macro() -> Result<(), TestFailure> {
    #[derive(Debug)]
    struct Foo;

    let (subscriber, handle) = subscriber::mock()
      .new_span(expect::span().with_fields(expect::field("self").with_value(&debug(Foo)).only()))
      .only()
      .run_with_handle();

    with_default(subscriber, || {
      error_span!("span", self = ?Foo);
    });
    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
  }
}
