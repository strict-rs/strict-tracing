use std::cell::RefCell;
use std::hash::Hasher as _;
use std::num::NonZeroUsize;
use std::sync::LazyLock;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use ahash::AHasher;
use log::Level;
use log::Metadata;
use lru::LruCache;
use parking_lot::Mutex;
use tracing_core::Callsite;
use tracing_core::Level as TraceLevel;
use tracing_core::Metadata as TraceMetadata;
use tracing_core::callsite;
use tracing_core::field;
use tracing_core::metadata::Kind;
use tracing_core::subscriber::Interest;

/// Mask used to reserve the low hash bit for the cached interest value.
const HASH_MASK: u64 = !1;
/// Mask used to decode the cached interest value.
const INTEREST_MASK: u64 = 1;

/// The interest cache configuration.
#[derive(Copy, Clone, Debug)]
pub struct InterestCacheConfig {
  /// Minimum log verbosity that should use the cache.
  min_verbosity:  Level,
  /// Number of entries retained by each thread-local LRU cache.
  lru_cache_size: usize,
}

impl Default for InterestCacheConfig {
  fn default() -> Self {
    Self {
      min_verbosity:  Level::Debug,
      lru_cache_size: 1024,
    }
  }
}

impl InterestCacheConfig {
  /// Return a configuration that disables interest caching.
  fn disabled() -> Self {
    Self {
      lru_cache_size: 0,
      ..Self::default()
    }
  }
  /// Sets the minimum logging verbosity for which the cache will apply.
  ///
  /// The interest for logs with a lower verbosity than specified here
  /// will not be cached.
  ///
  /// It should be set to the lowest verbosity level for which the majority
  /// of the logs in your application are usually *disabled*.
  ///
  /// In normal circumstances with typical logger usage patterns
  /// you shouldn't ever have to change this.
  ///
  /// By default this is set to `Debug`.
  #[must_use]
  pub const fn with_min_verbosity(mut self, level: Level) -> Self {
    self.min_verbosity = level;
    self
  }

  /// Sets the number of entries in the LRU cache used to cache interests
  /// for `log` records.
  ///
  /// The bigger the cache, the more unlikely it will be for the interest
  /// in a given callsite to be recalculated, at the expense of extra
  /// memory usage per every thread which tries to log events.
  ///
  /// Every unique [level] + [target] pair consumes a single slot
  /// in the cache. Entries will be added to the cache until its size
  /// reaches the value configured here, and from then on it will evict
  /// the least recently seen level + target pair when adding a new entry.
  ///
  /// The ideal value to set here widely depends on how much exactly
  /// you're logging, and how diverse the targets are to which you are logging.
  ///
  /// If your application spends a significant amount of time filtering logs
  /// which are *not* getting printed out then increasing this value will most
  /// likely help.
  ///
  /// Setting this to zero will disable the cache.
  ///
  /// By default this is set to 1024.
  ///
  /// [level]: log::Metadata::level
  /// [target]: log::Metadata::target
  #[must_use]
  pub const fn with_lru_cache_size(mut self, size: usize) -> Self {
    self.lru_cache_size = size;
    self
  }
}

/// Cache key for a log target and level.
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
struct Key {
  /// Address of the target string data.
  target_address:   usize,
  /// Packed log level and target length.
  level_and_length: usize,
}

/// Per-thread interest-cache state.
struct State {
  /// Minimum log verbosity that should use the cache.
  min_verbosity: Level,
  /// Global epoch observed by this thread-local cache.
  epoch:         usize,
  /// LRU cache keyed by log metadata target and level.
  cache:         Option<LruCache<Key, u64, ahash::RandomState>>,
}

impl State {
  /// Create per-thread cache state from a global configuration snapshot.
  fn new(epoch: usize, config: InterestCacheConfig) -> Self {
    Self {
      epoch,
      min_verbosity: config.min_verbosity,
      cache: NonZeroUsize::new(config.lru_cache_size).map(|cap| LruCache::with_hasher(cap, ahash::RandomState::default())),
    }
  }
}

// When the logger's filters are reconfigured the interest cache in core is cleared,
// and we also want to get notified when that happens so that we can clear our cache too.
//
// So what we do here is to register a dummy callsite with the core, just so that we can be
// notified when that happens. It doesn't really matter how exactly our dummy callsite looks
// like and whether subscribers will actually be interested in it, since nothing will actually
// be logged from it.

/// Global epoch bumped whenever log interest must be recomputed.
static INTEREST_CACHE_EPOCH: AtomicUsize = AtomicUsize::new(0);

/// Return the current interest-cache invalidation epoch.
fn interest_cache_epoch() -> usize {
  INTEREST_CACHE_EPOCH.load(Ordering::Relaxed)
}

/// Synthetic callsite used to observe tracing-core interest rebuilds.
struct SentinelCallsite;

impl Callsite for SentinelCallsite {
  fn set_interest(&self, _: Interest) {
    let _previous_epoch = INTEREST_CACHE_EPOCH.fetch_add(1, Ordering::SeqCst);
  }

  fn metadata(&self) -> &TraceMetadata<'_> {
    &SENTINEL_METADATA
  }
}

/// Synthetic callsite registered solely for rebuild notifications.
static SENTINEL_CALLSITE: SentinelCallsite = SentinelCallsite;
/// Metadata for the synthetic interest-cache callsite.
static SENTINEL_METADATA: TraceMetadata<'static> = TraceMetadata::new(
  "log interest cache",
  "log",
  TraceLevel::ERROR,
  None,
  None,
  None,
  &field::FieldSet::new(&[], tracing_core::identify_callsite!(&SENTINEL_CALLSITE)),
  Kind::EVENT,
);

/// Global cache configuration shared by all thread-local cache instances.
static CONFIG: LazyLock<Mutex<InterestCacheConfig>> = LazyLock::new(|| {
  callsite::register(&SENTINEL_CALLSITE);
  Mutex::new(InterestCacheConfig::disabled())
});

thread_local! {
    static STATE: RefCell<State> = {
        let config = CONFIG.lock();
        RefCell::new(State::new(interest_cache_epoch(), *config))
    };
}

/// Configure the per-thread interest cache.
#[allow(
  clippy::single_call_fn,
  reason = "interest-cache reconfiguration remains a narrow crate-local boundary while cache state stays private"
)]
pub(super) fn configure(new_config: Option<InterestCacheConfig>) {
  *CONFIG.lock() = new_config.unwrap_or_else(InterestCacheConfig::disabled);
  let _previous_epoch = INTEREST_CACHE_EPOCH.fetch_add(1, Ordering::SeqCst);
}

/// Return cached interest for a log metadata key, recomputing it with `callback` on miss.
#[allow(
  clippy::single_call_fn,
  reason = "interest-cache lookup remains a narrow crate-local boundary while cache state stays private"
)]
pub(super) fn try_cache(metadata: &Metadata<'_>, callback: impl FnOnce() -> bool) -> bool {
  STATE.with(|state_cell| {
    let Ok(mut state) = state_cell.try_borrow_mut() else {
      return callback();
    };

    // If the interest cache in core was rebuilt we need to reset the cache here too.
    let epoch = interest_cache_epoch();
    if epoch != state.epoch {
      *state = State::new(epoch, *CONFIG.lock());
    }

    let level = metadata.level();
    if level < state.min_verbosity {
      return callback();
    }
    let Some(cache) = state.cache.as_mut() else {
      return callback();
    };

    let target = metadata.target();

    let mut hasher = AHasher::default();
    hasher.write(target.as_bytes());

    // We mask out the least significant bit of the hash since we'll use
    // that space to save the interest.
    //
    // Since we use a good hashing function the loss of only a single bit
    // won't really affect us negatively.
    let target_hash = hasher.finish() & HASH_MASK;

    // Since log targets are usually static strings we just use the address of the pointer
    // as the key for our cache.
    //
    // We want each level to be cached separately so we also use the level as key, and since
    // some linkers at certain optimization levels deduplicate strings if their prefix matches
    // (e.g. "ham" and "hamster" might actually have the same address in memory) we also use the length.
    let key = Key {
      target_address:   target.as_ptr().addr(),
      // For extra efficiency we pack both the level and the length into a single field.
      // The `level` can be between 1 and 5, so it can take at most 3 bits of space.
      level_and_length: level_key(level) | target.len().wrapping_shl(3),
    };

    if let Some(&cached) = cache.get(&key) {
      // And here we make sure that the target actually matches.
      //
      // This is just a hash of the target string, so theoretically we're not guaranteed
      // that it won't collide, however in practice it shouldn't matter as it is quite
      // unlikely that the target string's address and its length and the level and
      // the hash will *all* be equal at the same time.
      //
      // We could of course actually store the whole target string in our cache,
      // but we really want to avoid doing that as the necessary memory allocations
      // would completely tank our performance, especially in cases where the cache's
      // size is too small so it needs to regularly replace entries.
      if cached & HASH_MASK == target_hash {
        return (cached & INTEREST_MASK) != 0;
      }

      // Realistically we should never land here, unless someone is using a non-static
      // target string with the same length and level, or is very lucky and found a hash
      // collision for the cache's key.
    }

    let interest = callback();
    let _previous = cache.put(key, target_hash | u64::from(interest));

    interest
  })
}

/// Encode a `log` level into the low bits of an interest-cache key.
#[allow(
  clippy::single_call_fn,
  reason = "level bit encoding stays separate from pointer and length key assembly"
)]
const fn level_key(level: Level) -> usize {
  match level {
    Level::Error => 1,
    Level::Warn => 2,
    Level::Info => 3,
    Level::Debug => 4,
    Level::Trace => 5,
  }
}

#[cfg(test)]
mod tests {

  use std::str::from_utf8;
  use std::thread;

  use strict_test_support::TestFailure;
  use strict_test_support::ensure;
  use strict_test_support::ensure_eq;
  use strict_test_support::ensure_ok;
  use strict_test_support::ensure_some;

  use super::*;

  /// Increment a test counter without using unchecked arithmetic.
  fn increment(counter: &mut usize) {
    *counter = counter.saturating_add(1);
  }

  fn lock_for_test() -> impl Drop {
    // We need to make sure only one test runs at a time.
    static LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    LOCK.lock()
  }

  fn run_in_worker(callback: impl FnOnce() -> Result<(), TestFailure> + Send + 'static) -> Result<(), TestFailure> {
    thread::spawn(callback).join().map_err(|_panic| TestFailure::Condition {
      context: "worker thread must not panic",
    })?
  }

  fn observe_cache(callback: impl FnOnce() -> bool, metadata: &Metadata<'_>) {
    let _cached = try_cache(metadata, callback);
  }

  #[test]
  fn test_when_disabled_the_callback_is_always_called() -> Result<(), TestFailure> {
    let _lock = lock_for_test();

    *CONFIG.lock() = InterestCacheConfig::disabled();

    run_in_worker(|| {
      let metadata = log::MetadataBuilder::new().level(Level::Trace).target("dummy").build();
      let mut count = 0;
      observe_cache(
        || {
          increment(&mut count);
          true
        },
        &metadata,
      );
      ensure_eq(&count, &1, "disabled cache calls callback once")?;
      observe_cache(
        || {
          increment(&mut count);
          true
        },
        &metadata,
      );
      ensure_eq(&count, &2, "disabled cache calls callback every time")
    })
  }

  #[test]
  fn test_when_enabled_the_callback_is_called_only_once_for_a_high_enough_verbosity() -> Result<(), TestFailure> {
    let _lock = lock_for_test();

    *CONFIG.lock() = InterestCacheConfig::default().with_min_verbosity(Level::Debug);

    run_in_worker(|| {
      let metadata = log::MetadataBuilder::new().level(Level::Debug).target("dummy").build();
      let mut count = 0;
      observe_cache(
        || {
          increment(&mut count);
          true
        },
        &metadata,
      );
      ensure_eq(&count, &1, "enabled cache calls callback before storing hit")?;
      observe_cache(
        || {
          increment(&mut count);
          true
        },
        &metadata,
      );
      ensure_eq(&count, &1, "enabled cache reuses stored interest")
    })
  }

  #[test]
  fn test_when_core_interest_cache_is_rebuilt_this_cache_is_also_flushed() -> Result<(), TestFailure> {
    let _lock = lock_for_test();

    *CONFIG.lock() = InterestCacheConfig::default().with_min_verbosity(Level::Debug);

    run_in_worker(|| {
      let metadata = log::MetadataBuilder::new().level(Level::Debug).target("dummy").build();
      ({
        let mut count = 0;
        observe_cache(
          || {
            increment(&mut count);
            true
          },
          &metadata,
        );
        observe_cache(
          || {
            increment(&mut count);
            true
          },
          &metadata,
        );
        ensure_eq(&count, &1, "cache serves repeated metadata before rebuild")?;
      });
      callsite::rebuild_interest_cache();
      ({
        let mut count = 0;
        observe_cache(
          || {
            increment(&mut count);
            true
          },
          &metadata,
        );
        observe_cache(
          || {
            increment(&mut count);
            true
          },
          &metadata,
        );
        ensure_eq(&count, &1, "cache serves repeated metadata after rebuild")?;
      });
      Ok(())
    })
  }

  #[test]
  fn test_when_enabled_the_callback_is_always_called_for_a_low_enough_verbosity() -> Result<(), TestFailure> {
    let _lock = lock_for_test();

    *CONFIG.lock() = InterestCacheConfig::default().with_min_verbosity(Level::Debug);

    run_in_worker(|| {
      let metadata = log::MetadataBuilder::new().level(Level::Info).target("dummy").build();
      let mut count = 0;
      observe_cache(
        || {
          increment(&mut count);
          true
        },
        &metadata,
      );
      ensure_eq(&count, &1, "below-threshold metadata calls callback once")?;
      observe_cache(
        || {
          increment(&mut count);
          true
        },
        &metadata,
      );
      ensure_eq(&count, &2, "below-threshold metadata is not cached")
    })
  }

  #[test]
  fn test_different_log_levels_are_cached_separately() -> Result<(), TestFailure> {
    let _lock = lock_for_test();

    *CONFIG.lock() = InterestCacheConfig::default().with_min_verbosity(Level::Debug);

    run_in_worker(|| {
      let metadata_debug = log::MetadataBuilder::new().level(Level::Debug).target("dummy").build();
      let metadata_trace = log::MetadataBuilder::new().level(Level::Trace).target("dummy").build();
      let mut count_debug = 0;
      let mut count_trace = 0;
      observe_cache(
        || {
          increment(&mut count_debug);
          true
        },
        &metadata_debug,
      );
      observe_cache(
        || {
          increment(&mut count_trace);
          true
        },
        &metadata_trace,
      );
      observe_cache(
        || {
          increment(&mut count_debug);
          true
        },
        &metadata_debug,
      );
      observe_cache(
        || {
          increment(&mut count_trace);
          true
        },
        &metadata_trace,
      );
      ensure_eq(&count_debug, &1, "debug metadata callback count")?;
      ensure_eq(&count_trace, &1, "trace metadata callback count")
    })
  }

  #[test]
  fn test_different_log_targets_are_cached_separately() -> Result<(), TestFailure> {
    let _lock = lock_for_test();

    *CONFIG.lock() = InterestCacheConfig::default().with_min_verbosity(Level::Debug);

    run_in_worker(|| {
      let metadata_1 = log::MetadataBuilder::new().level(Level::Trace).target("dummy_1").build();
      let metadata_2 = log::MetadataBuilder::new().level(Level::Trace).target("dummy_2").build();
      let mut count_1 = 0;
      let mut count_2 = 0;
      observe_cache(
        || {
          increment(&mut count_1);
          true
        },
        &metadata_1,
      );
      observe_cache(
        || {
          increment(&mut count_2);
          true
        },
        &metadata_2,
      );
      observe_cache(
        || {
          increment(&mut count_1);
          true
        },
        &metadata_1,
      );
      observe_cache(
        || {
          increment(&mut count_2);
          true
        },
        &metadata_2,
      );
      ensure_eq(&count_1, &1, "first target callback count")?;
      ensure_eq(&count_2, &1, "second target callback count")
    })
  }

  #[test]
  fn test_when_cache_runs_out_of_space_the_callback_is_called_again() -> Result<(), TestFailure> {
    let _lock = lock_for_test();

    *CONFIG.lock() = InterestCacheConfig::default()
      .with_min_verbosity(Level::Debug)
      .with_lru_cache_size(1);

    run_in_worker(|| {
      let metadata_1 = log::MetadataBuilder::new().level(Level::Trace).target("dummy_1").build();
      let metadata_2 = log::MetadataBuilder::new().level(Level::Trace).target("dummy_2").build();
      let mut count = 0;
      observe_cache(
        || {
          increment(&mut count);
          true
        },
        &metadata_1,
      );
      observe_cache(
        || {
          increment(&mut count);
          true
        },
        &metadata_1,
      );
      ensure_eq(&count, &1, "first target cache hit count")?;
      observe_cache(|| true, &metadata_2);
      observe_cache(
        || {
          increment(&mut count);
          true
        },
        &metadata_1,
      );
      ensure_eq(&count, &2, "evicted target calls callback again")
    })
  }

  #[test]
  fn test_cache_returns_previously_computed_value() -> Result<(), TestFailure> {
    let _lock = lock_for_test();

    *CONFIG.lock() = InterestCacheConfig::default().with_min_verbosity(Level::Debug);

    run_in_worker(|| {
      let metadata_1 = log::MetadataBuilder::new().level(Level::Trace).target("dummy_1").build();
      let metadata_2 = log::MetadataBuilder::new().level(Level::Trace).target("dummy_2").build();
      observe_cache(|| true, &metadata_1);
      let mut first_unexpected_callback = false;
      let first_cached = try_cache(&metadata_1, || {
        first_unexpected_callback = true;
        false
      });
      ensure(first_cached, "first cached value should be reused")?;
      ensure(
        !first_unexpected_callback,
        "cache hit should not invoke callback for first metadata",
      )?;
      observe_cache(|| false, &metadata_2);
      let mut second_unexpected_callback = false;
      let second_cached = try_cache(&metadata_2, || {
        second_unexpected_callback = true;
        true
      });
      ensure(!second_cached, "second cached value should be reused")?;
      ensure(
        !second_unexpected_callback,
        "cache hit should not invoke callback for second metadata",
      )
    })
  }

  #[test]
  fn test_cache_handles_non_static_target_string() -> Result<(), TestFailure> {
    let _lock = lock_for_test();

    *CONFIG.lock() = InterestCacheConfig::default().with_min_verbosity(Level::Debug);

    run_in_worker(|| {
      let mut target = *b"dummy_1";
      let metadata_1 = log::MetadataBuilder::new()
        .level(Level::Trace)
        .target(ensure_ok(from_utf8(&target), "mutable target remains valid UTF-8")?)
        .build();

      observe_cache(|| true, &metadata_1);
      let mut first_unexpected_callback = false;
      let first_cached = try_cache(&metadata_1, || {
        first_unexpected_callback = true;
        false
      });
      ensure(first_cached, "first cached target should be reused")?;
      ensure(!first_unexpected_callback, "cache hit should not invoke callback for first target")?;

      *ensure_some(target.last_mut(), "target contains a mutable suffix")? = b'2';
      let metadata_2 = log::MetadataBuilder::new()
        .level(Level::Trace)
        .target(ensure_ok(from_utf8(&target), "mutated target remains valid UTF-8")?)
        .build();

      observe_cache(|| false, &metadata_2);
      let mut second_unexpected_callback = false;
      let second_cached = try_cache(&metadata_2, || {
        second_unexpected_callback = true;
        true
      });
      ensure(!second_cached, "second cached target should be reused")?;
      ensure(
        !second_unexpected_callback,
        "cache hit should not invoke callback for second target",
      )
    })
  }
}
