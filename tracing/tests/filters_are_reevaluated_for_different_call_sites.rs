//! Filter re-evaluation coverage for distinct callsites with matching names.

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
  use tracing::level_filters::STATIC_MAX_LEVEL;
  use tracing::span;
  use tracing::subscriber::set_global_default;
  use tracing_mock::*;

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn filters_are_reevaluated_for_different_call_sites() -> Result<(), TestFailure> {
    // Asserts that the `span!` macro caches the result of calling
    // `Subscriber::enabled` for each span.
    let charlie_count = Arc::new(AtomicUsize::new(0));
    let dave_count = Arc::new(AtomicUsize::new(0));
    let charlie_count2 = Arc::clone(&charlie_count);
    let dave_count2 = Arc::clone(&dave_count);

    let subscriber = subscriber::mock()
      .with_filter(move |meta| match meta.name() {
        "charlie" => {
          let _previous = charlie_count2.fetch_add(1, Ordering::Relaxed);
          false
        }
        "dave" => {
          let _previous = dave_count2.fetch_add(1, Ordering::Relaxed);
          true
        }
        _ => false,
      })
      .run();

    // Since this test is in its own file anyway, we can do this. Thus, this
    // test will work even with no-std.
    ensure_ok(set_global_default(subscriber), "global subscriber should install")?;

    // Under a `max_level_*` cap that statically disables TRACE, the spans are
    // compiled out before the subscriber's filter can run, so each callsite is
    // evaluated zero times instead of climbing to one and then two.
    let expected_once = usize::from(STATIC_MAX_LEVEL.enables(Level::TRACE));
    let expected_twice = expected_once.saturating_mul(2);

    // Enter "charlie" and then "dave". The dispatcher expects to see "dave" but
    // not "charlie."
    let charlie = span!(Level::TRACE, "charlie");
    let dave = charlie.in_scope(|| {
      let dave = span!(Level::TRACE, "dave");
      dave.in_scope(|| {});
      dave
    });

    // The filter should have seen each span a single time.
    ensure_eq(
      &charlie_count.load(Ordering::Relaxed),
      &expected_once,
      "charlie filter runs once after first span",
    )?;
    ensure_eq(
      &dave_count.load(Ordering::Relaxed),
      &expected_once,
      "dave filter runs once after first span",
    )?;

    charlie.in_scope(|| dave.in_scope(|| {}));

    // The subscriber should see "dave" again, but the filter should not have
    // been called.
    ensure_eq(
      &charlie_count.load(Ordering::Relaxed),
      &expected_once,
      "charlie filter remains cached after nested enter",
    )?;
    ensure_eq(
      &dave_count.load(Ordering::Relaxed),
      &expected_once,
      "dave filter remains cached after nested enter",
    )?;

    // A different span with the same name has a different call site, so it
    // should cause the filter to be reapplied.
    let charlie2 = span!(Level::TRACE, "charlie");
    charlie.in_scope(|| {});
    ensure_eq(
      &charlie_count.load(Ordering::Relaxed),
      &expected_twice,
      "new charlie callsite evaluates filter",
    )?;
    ensure_eq(
      &dave_count.load(Ordering::Relaxed),
      &expected_once,
      "dave filter stays cached before second dave callsite",
    )?;

    // But, the filter should not be re-evaluated for the new "charlie" span
    // when it is re-entered.
    charlie2.in_scope(|| span!(Level::TRACE, "dave").in_scope(|| {}));
    ensure_eq(
      &charlie_count.load(Ordering::Relaxed),
      &expected_twice,
      "second charlie span stays cached",
    )?;
    ensure_eq(
      &dave_count.load(Ordering::Relaxed),
      &expected_twice,
      "new dave callsite evaluates filter",
    )
  }
}
