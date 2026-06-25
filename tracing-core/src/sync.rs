//! Internal synchronization facade for the callsite registry.

#[cfg(feature = "std")]
pub(super) use parking_lot::{Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};

#[cfg(not(feature = "std"))]
pub(super) type MutexGuard<'a, T> = spin::mutex::MutexGuard<'a, T, spin::Spin>;

/// Non-poisoning mutex used in `no_std` builds.
#[cfg(not(feature = "std"))]
#[derive(Debug, Default)]
pub(super) struct Mutex<T> {
    /// Spin-backed lock storage.
    inner: spin::Mutex<T>,
}

#[cfg(not(feature = "std"))]
impl<T> Mutex<T> {
    /// Returns a new spin-backed mutex.
    const fn new(data: T) -> Self {
        Self {
            inner: spin::Mutex::new(data),
        }
    }

    /// Acquires the lock.
    pub(super) fn lock(&self) -> MutexGuard<'_, T> {
        self.inner.lock()
    }
}

/// Builds a mutex for statics and const initializers.
#[cfg(not(feature = "std"))]
pub(super) const fn mutex<T>(data: T) -> Mutex<T> {
    Mutex::new(data)
}
