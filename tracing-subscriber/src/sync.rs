// Abstracts over sync primitive implementations.
//
// Optionally, we allow the Rust standard library's `RwLock` to be replaced
// with the `parking_lot` crate's implementation. This may provide improved
// performance in some cases. However, the `parking_lot` dependency is an
// opt-in feature flag. Because `parking_lot::RwLock` has a slightly different
// API than `std::sync::RwLock` (it does not support poisoning on panics), we
// wrap it with a type that provides the same method signatures. This allows us
// to transparently swap `parking_lot` in without changing code at the callsite.
#[cfg(feature = "parking_lot")]
use parking_lot::RwLock as InnerRwLock;
#[cfg(not(feature = "parking_lot"))]
use std::sync::RwLock as InnerRwLock;

#[cfg(feature = "parking_lot")]
pub(crate) use parking_lot::{RwLockReadGuard, RwLockWriteGuard};
#[cfg(not(feature = "parking_lot"))]
pub(crate) use std::sync::{RwLockReadGuard, RwLockWriteGuard};

#[cfg(not(feature = "parking_lot"))]
use std::sync::PoisonError;
#[cfg(not(feature = "parking_lot"))]
use tracing_core::subscriber::{SubscriberError, SubscriberResult};

/// A lock-acquisition result normalized across lock implementations.
pub(crate) enum LockResult<T> {
    /// The lock was acquired successfully.
    Acquired(T),
    /// The standard-library lock was poisoned by a panic while locked.
    #[cfg(not(feature = "parking_lot"))]
    Poisoned,
}

impl<T> LockResult<T> {
    /// Creates a successful lock-acquisition result.
    #[cfg(feature = "parking_lot")]
    const fn acquired(value: T) -> Self {
        Self::Acquired(value)
    }
}

/// Converts normalized lock acquisition into the core subscriber result channel.
#[cfg(not(feature = "parking_lot"))]
pub(crate) fn lock_result_into_subscriber_result<T>(result: LockResult<T>) -> SubscriberResult<T> {
    match result {
        LockResult::Acquired(lock) => Ok(lock),
        LockResult::Poisoned => Err(SubscriberError::lock_poisoned()),
    }
}

#[cfg(not(feature = "parking_lot"))]
impl<T> From<Result<T, PoisonError<T>>> for LockResult<T> {
    fn from(result: Result<T, PoisonError<T>>) -> Self {
        result.map_or(Self::Poisoned, Self::Acquired)
    }
}

/// A read-write lock backed by the selected synchronization implementation.
#[derive(Debug)]
pub(crate) struct RwLock<T> {
    /// The selected backing lock implementation.
    inner: InnerRwLock<T>,
}

impl<T> RwLock<T> {
    /// Creates a new read-write lock containing the provided value.
    pub(crate) const fn new(value: T) -> Self {
        Self {
            inner: InnerRwLock::new(value),
        }
    }

    /// Acquires a shared read guard.
    #[inline]
    #[cfg(feature = "parking_lot")]
    pub(crate) fn read(&self) -> LockResult<RwLockReadGuard<'_, T>> {
        LockResult::acquired(self.inner.read())
    }

    /// Acquires a shared read guard.
    #[inline]
    #[cfg(not(feature = "parking_lot"))]
    pub(crate) fn read(&self) -> LockResult<RwLockReadGuard<'_, T>> {
        self.inner.read().into()
    }

    /// Acquires an exclusive write guard.
    #[inline]
    #[cfg(feature = "parking_lot")]
    pub(crate) fn write(&self) -> LockResult<RwLockWriteGuard<'_, T>> {
        LockResult::acquired(self.inner.write())
    }

    /// Acquires an exclusive write guard.
    #[inline]
    #[cfg(not(feature = "parking_lot"))]
    pub(crate) fn write(&self) -> LockResult<RwLockWriteGuard<'_, T>> {
        self.inner.write().into()
    }
}

impl<T: Default> Default for RwLock<T> {
    fn default() -> Self {
        Self {
            inner: InnerRwLock::default(),
        }
    }
}
