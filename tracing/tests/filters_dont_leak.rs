#![cfg(feature = "std")]
//! Filter leakage regression coverage.

#[cfg(test)]
mod tests {
  use strict_test_support::TestFailure;
  use strict_test_support::ensure_ok;
  use tracing::subscriber::set_default;
  use tracing::subscriber::with_default;
  use tracing_mock::*;

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn spans_dont_leak() -> Result<(), TestFailure> {
    fn do_span() {
      let span = tracing::debug_span!("alice");
      let _entered = span.enter();
    }

    let (subscriber, handle) = subscriber::mock()
      .named("spans/subscriber1")
      .with_filter(|_| false)
      .only()
      .run_with_handle();

    let _guard = set_default(subscriber);

    do_span();

    let alice = expect::span().named("alice");
    let (subscriber2, handle2) = subscriber::mock()
      .named("spans/subscriber2")
      .with_filter(|_| true)
      .new_span(alice.clone())
      .enter(alice.clone())
      .exit(alice.clone())
      .close_span(alice)
      .only()
      .run_with_handle();

    with_default(subscriber2, do_span);

    do_span();

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    ensure_ok(handle2.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn events_dont_leak() -> Result<(), TestFailure> {
    fn do_event() {
      tracing::debug!("alice");
    }

    let (subscriber, handle) = subscriber::mock()
      .named("events/subscriber1")
      .with_filter(|_| false)
      .only()
      .run_with_handle();

    let _guard = set_default(subscriber);

    do_event();

    let (subscriber2, handle2) = subscriber::mock()
      .named("events/subscriber2")
      .with_filter(|_| true)
      .event(expect::event())
      .only()
      .run_with_handle();

    with_default(subscriber2, do_event);

    do_event();

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    ensure_ok(handle2.finished(), "mock expectations should finish")?;
    Ok(())
  }
}
