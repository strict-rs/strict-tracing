//! Tests layer filter interest caching.
#![cfg(feature = "registry")]

#[cfg(test)]
mod tests {
    use parking_lot::Mutex;
    use std::{collections::HashMap, sync::Arc};
    use strict_test_support::{TestFailure, ensure, ensure_ok};
    use tracing::subscriber::set_default;
    use tracing::{Level, Subscriber as _};
    use tracing_mock::{expect, layer};
    use tracing_subscriber::{filter, prelude::*};

    fn events() {
        tracing::trace!("hello trace");
        tracing::debug!("hello debug");
        tracing::info!("hello info");
        tracing::warn!("hello warn");
        tracing::error!("hello error");
    }

    #[test]
    fn layer_filter_interests_are_cached() -> Result<(), TestFailure> {
        let seen = Arc::new(Mutex::new(HashMap::new()));
        let seen_filter = Arc::clone(&seen);
        let filter = filter::filter_fn(move |meta| {
            *seen_filter.lock().entry(meta.callsite()).or_insert(0_usize) += 1;
            meta.level() == &Level::INFO
        });

        let (expect, handle) = layer::mock()
            .event(expect::event().at_level(Level::INFO))
            .event(expect::event().at_level(Level::INFO))
            .only()
            .run_with_handle();

        let subscriber = tracing_subscriber::registry().with(expect.with_filter(filter));
        ensure(
            subscriber.max_level_hint().is_none(),
            "dynamic filter does not provide a max level hint",
        )?;

        let _subscriber = set_default(subscriber);

        events();
        let first_counts_cached = {
            let seen_counts = seen.lock();
            seen_counts.values().all(|&count| count == 1)
        };
        ensure(
            first_counts_cached,
            "each callsite is seen once after the first event set",
        )?;

        events();
        let second_counts_cached = {
            let seen_counts = seen.lock();
            seen_counts.values().all(|&count| count == 1)
        };
        ensure(
            second_counts_cached,
            "each callsite is still seen once after the second event set",
        )?;

        ensure_ok(handle.finished(), "mock expectations should finish")?;
        Ok(())
    }
}
