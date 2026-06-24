//! Thread-local dispatcher integration coverage.
#![cfg(feature = "std")]

#[cfg(test)]
mod tests {
    mod common {
        include!("common/mod.rs");
    }

    use common::*;
    use strict_test_support::{TestFailure, ensure, ensure_ok};
    use tracing_core::dispatcher::*;

    #[test]
    fn set_default_dispatch() -> Result<(), TestFailure> {
        ensure_ok(
            set_global_default(Dispatch::new(TestSubscriberA)),
            "global dispatch set failed",
        )?;
        get_default(|current| {
            ensure(
                current.is::<TestSubscriberA>(),
                "global dispatch get failed",
            )
        })?;

        let guard = set_default(&Dispatch::new(TestSubscriberB));
        get_default(|current| ensure(current.is::<TestSubscriberB>(), "set_default get failed"))?;

        // Drop the guard, setting the dispatch back to the global dispatch
        drop(guard);

        get_default(|current| {
            ensure(
                current.is::<TestSubscriberA>(),
                "global dispatch get failed",
            )
        })
    }

    #[test]
    fn nested_set_default() -> Result<(), TestFailure> {
        let _guard = set_default(&Dispatch::new(TestSubscriberA));
        get_default(|current| {
            ensure(
                current.is::<TestSubscriberA>(),
                "set_default for outer subscriber failed",
            )
        })?;

        let inner_guard = set_default(&Dispatch::new(TestSubscriberB));
        get_default(|current| {
            ensure(
                current.is::<TestSubscriberB>(),
                "set_default inner subscriber failed",
            )
        })?;

        drop(inner_guard);
        get_default(|current| {
            ensure(
                current.is::<TestSubscriberA>(),
                "set_default outer subscriber failed",
            )
        })
    }
}
