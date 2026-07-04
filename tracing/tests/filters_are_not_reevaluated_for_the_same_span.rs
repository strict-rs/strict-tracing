//! Filter caching coverage for repeated entries of the same span.

#[cfg(test)]
mod tests {
  // Tests that depend on a count of the number of times their filter is evaluated
  // cant exist in the same file with other tests that add subscribers to the
  // registry. The registry was changed so that each time a new dispatcher is
  // added all filters are re-evaluated. The tests being run only in separate
  // threads with shared global state lets them interfere with each other
  #[cfg(not(feature = "std"))]
  extern crate std;

  use std::sync::Arc;
  use std::sync::atomic::AtomicUsize;
  use std::sync::atomic::Ordering;

  use strict_test_support::TestFailure;
  use strict_test_support::ensure_eq;
  use strict_test_support::ensure_ok;
  use tracing::Level;
  use tracing::span;
  use tracing::subscriber::set_global_default;
  use tracing_mock::*;

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn filters_are_not_reevaluated_for_the_same_span() -> Result<(), TestFailure> {
    // Asserts that the `span!` macro caches the result of calling
    // `Subscriber::enabled` for each span.
    let alice_count = Arc::new(AtomicUsize::new(0));
    let bob_count = Arc::new(AtomicUsize::new(0));
    let alice_count2 = Arc::clone(&alice_count);
    let bob_count2 = Arc::clone(&bob_count);

    let (subscriber, handle) = subscriber::mock()
      .with_filter(move |meta| match meta.name() {
        "alice" => {
          let _previous = alice_count2.fetch_add(1, Ordering::Relaxed);
          false
        }
        "bob" => {
          let _previous = bob_count2.fetch_add(1, Ordering::Relaxed);
          true
        }
        _ => false,
      })
      .run_with_handle();

    // Since this test is in its own file anyway, we can do this. Thus, this
    // test will work even with no-std.
    ensure_ok(set_global_default(subscriber), "global subscriber should install")?;

    // Enter "alice" and then "bob". The dispatcher expects to see "bob" but
    // not "alice."
    let alice = span!(Level::TRACE, "alice");
    let bob = alice.in_scope(|| {
      let bob = span!(Level::TRACE, "bob");
      bob.in_scope(|| ());
      bob
    });

    // The filter should have seen each span a single time.
    ensure_eq(&alice_count.load(Ordering::Relaxed), &1, "alice filter runs once after first span")?;
    ensure_eq(&bob_count.load(Ordering::Relaxed), &1, "bob filter runs once after first span")?;

    alice.in_scope(|| bob.in_scope(|| {}));

    // The subscriber should see "bob" again, but the filter should not have
    // been called.
    ensure_eq(
      &alice_count.load(Ordering::Relaxed),
      &1,
      "alice filter remains cached after nested enter",
    )?;
    ensure_eq(
      &bob_count.load(Ordering::Relaxed),
      &1,
      "bob filter remains cached after nested enter",
    )?;

    bob.in_scope(|| {});
    ensure_eq(
      &alice_count.load(Ordering::Relaxed),
      &1,
      "alice filter remains cached after bob re-enter",
    )?;
    ensure_eq(
      &bob_count.load(Ordering::Relaxed),
      &1,
      "bob filter remains cached after bob re-enter",
    )?;

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
  }
}
