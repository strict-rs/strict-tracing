//! Abstracts over sync primitive implementations.
//!
//! The rolling appender uses `parking_lot` directly so the lock semantics do
//! not change across dependency graphs.

pub(crate) use parking_lot::{RwLock, RwLockReadGuard};
