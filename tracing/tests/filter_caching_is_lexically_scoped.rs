//! Filter caching coverage for repeated calls from the same lexical callsite.

#[cfg(test)]
mod tests {
  // Tests that depend on a count of the number of times their filter is evaluated
  // can't exist in the same file with other tests that add subscribers to the
  // registry. The registry was changed so that each time a new dispatcher is
  // added all filters are re-evaluated. The tests being run only in separate
  // threads with shared global state lets them interfere with each other

  #[cfg(not(feature = "std"))]
  extern crate std;

  use std::sync::Arc;
  use std::sync::atomic::AtomicUsize;
  use std::sync::atomic::Ordering;

  use strict_test_support::TestFailure;
  use strict_test_support::ensure;
  use strict_test_support::ensure_eq;
  use strict_test_support::ensure_ok;
  use tracing::Level;
  use tracing::span;
  use tracing::subscriber::set_global_default;
  use tracing_mock::*;

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn filter_caching_is_lexically_scoped() -> Result<(), TestFailure> {
    fn my_great_function() -> bool {
      span!(Level::TRACE, "emily").in_scope(|| true)
    }

    fn my_other_function() -> bool {
      span!(Level::TRACE, "frank").in_scope(|| true)
    }

    let count = Arc::new(AtomicUsize::new(0));
    let count2 = Arc::clone(&count);

    let subscriber = subscriber::mock()
      .with_filter(move |meta| match meta.name() {
        "emily" | "frank" => {
          let _previous = count2.fetch_add(1, Ordering::Relaxed);
          true
        }
        _ => false,
      })
      .run();

    // Since this test is in its own file anyway, we can do this. Thus, this
    // test will work even with no-std.
    ensure_ok(set_global_default(subscriber), "global subscriber should install")?;

    // Call the function once. The filter should be re-evaluated.
    ensure(my_great_function(), "first emily call enters span")?;
    ensure_eq(&count.load(Ordering::Relaxed), &1, "first emily call evaluates filter once")?;

    // Call the function again. The cached result should be used.
    ensure(my_great_function(), "second emily call enters span")?;
    ensure_eq(&count.load(Ordering::Relaxed), &1, "second emily call reuses cached filter")?;

    ensure(my_other_function(), "first frank call enters span")?;
    ensure_eq(&count.load(Ordering::Relaxed), &2, "first frank call evaluates its filter")?;

    ensure(my_great_function(), "third emily call enters span")?;
    ensure_eq(&count.load(Ordering::Relaxed), &2, "third emily call reuses cached filter")?;

    ensure(my_other_function(), "second frank call enters span")?;
    ensure_eq(&count.load(Ordering::Relaxed), &2, "second frank call reuses cached filter")?;

    ensure(my_great_function(), "fourth emily call enters span")?;
    ensure_eq(&count.load(Ordering::Relaxed), &2, "fourth emily call reuses cached filter")
  }
}
