#[cfg(feature = "std")]
#[cfg(feature = "parking_lot")]
/// Acquires a lock and normalizes the selected backend's failure behavior.
macro_rules! try_lock {
  ($lock:expr) => {{
    let crate::LockResult::Acquired(lock) = $lock;
    lock
  }};
  ($lock:expr,else $els:expr) => {{
    let crate::LockResult::Acquired(lock) = $lock;
    lock
  }};
}

#[cfg(feature = "std")]
#[cfg(not(feature = "parking_lot"))]
/// Acquires a lock and preserves the caller's fallback behavior on poisoning.
macro_rules! try_lock {
    ($lock:expr) => {
        try_lock!($lock, else return)
    };
    ($lock:expr, else $els:expr) => {
        match $lock {
            crate::LockResult::Acquired(lock) => lock,
            crate::LockResult::Poisoned => $els,
        }
    };
}

#[cfg(feature = "std")]
#[cfg(feature = "parking_lot")]
/// Acquires a lock for a subscriber callback.
macro_rules! try_lock_subscriber {
  ($lock:expr) => {{
    let crate::LockResult::Acquired(lock) = $lock;
    lock
  }};
}

#[cfg(feature = "std")]
#[cfg(not(feature = "parking_lot"))]
/// Acquires a lock for a subscriber callback, propagating poisoning as a typed
/// `tracing-core` subscriber error.
macro_rules! try_lock_subscriber {
  ($lock:expr) => {
    crate::lock_result_into_subscriber_result($lock)?
  };
}

/// Expands items with crate feature cfgs and matching `docsrs` cfg docs.
macro_rules! feature {
    (
        #![$meta:meta]
        $($item:item)*
    ) => {
        $(
            #[cfg($meta)]
            #[cfg_attr(docsrs, doc(cfg($meta)))]
            $item
        )*
    }
}
