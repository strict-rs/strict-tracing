//! Internal synchronization facade for the callsite registry.

#[cfg(feature = "std")]
pub use parking_lot::{Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};

#[cfg(not(feature = "std"))]
pub type MutexGuard<'a, T> = spin::mutex::MutexGuard<'a, T, spin::Spin>;

/// Builds a mutex for statics and const initializers.
#[cfg(feature = "std")]
pub const fn mutex<T>(data: T) -> Mutex<T> {
    parking_lot::const_mutex(data)
}

/// Builds a read/write lock for statics and const initializers.
#[cfg(feature = "std")]
pub const fn rwlock<T>(data: T) -> RwLock<T> {
    parking_lot::const_rwlock(data)
}

/// Non-poisoning mutex used in `no_std` builds.
#[cfg(not(feature = "std"))]
#[derive(Debug, Default)]
pub struct Mutex<T> {
    /// Spin-backed lock storage.
    inner: spin::Mutex<T>,
}

#[cfg(not(feature = "std"))]
impl<T> Mutex<T> {
    /// Returns a new spin-backed mutex.
    pub const fn new(data: T) -> Self {
        Self {
            inner: spin::Mutex::new(data),
        }
    }

    /// Acquires the lock.
    pub fn lock(&self) -> MutexGuard<'_, T> {
        self.inner.lock()
    }
}

/// Builds a mutex for statics and const initializers.
#[cfg(not(feature = "std"))]
pub const fn mutex<T>(data: T) -> Mutex<T> {
    Mutex::new(data)
}
