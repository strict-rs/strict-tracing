// taken from https://github.com/hyperium/http/blob/master/src/extensions.rs.

use crate::{RwLockReadGuard, RwLockWriteGuard};
use alloc::{boxed::Box, fmt};
use core::{
    any::{Any, TypeId},
    hash::{BuildHasherDefault, Hasher},
};
use std::collections::HashMap;

/// A type map keyed by concrete value type.
type AnyMap = HashMap<TypeId, Box<dyn Any + Send + Sync>, BuildHasherDefault<IdHasher>>;

/// Hashes [`TypeId`] keys using their compiler-provided hash value.
///
/// With [`TypeId`]s as keys, there's no need to hash them. They are already
/// hashes themselves, coming from the compiler. The [`IdHasher`] holds the
/// `u64` of the [`TypeId`], and then returns it, instead of doing any bit
/// fiddling.
#[derive(Default, Debug)]
struct IdHasher(u64);

impl Hasher for IdHasher {
    fn write(&mut self, bytes: &[u8]) {
        let mut hash = 0xcbf2_9ce4_8422_2325;
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        self.0 = hash;
    }

    #[inline]
    fn write_u64(&mut self, id: u64) {
        self.0 = id;
    }

    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }
}

/// An immutable, read-only reference to a Span's extensions.
#[derive(Debug)]
#[cfg_attr(docsrs, doc(cfg(feature = "std")))]
pub struct Extensions<'a> {
    /// Read guard for the span's extension map.
    inner: RwLockReadGuard<'a, ExtensionsInner>,
}

#[cfg(feature = "registry")]
impl<'a> Extensions<'a> {
    /// Creates an immutable extension view from a registry read guard.
    #[allow(
        clippy::single_call_fn,
        reason = "constructor keeps extension guard wrapping local to the registry module"
    )]
    pub(super) const fn new(inner: RwLockReadGuard<'a, ExtensionsInner>) -> Self {
        Self { inner }
    }
}

impl Extensions<'_> {
    /// Immutably borrows a type previously inserted into this `Extensions`.
    #[must_use]
    pub fn get<T: 'static>(&self) -> Option<&T> {
        self.inner.get::<T>()
    }
}

/// A mutable reference to a Span's extensions.
#[derive(Debug)]
#[cfg_attr(docsrs, doc(cfg(feature = "std")))]
pub struct ExtensionsMut<'a> {
    /// Write guard for the span's extension map.
    inner: RwLockWriteGuard<'a, ExtensionsInner>,
}

#[cfg(feature = "registry")]
impl<'a> ExtensionsMut<'a> {
    /// Creates a mutable extension view from a registry write guard.
    #[allow(
        clippy::single_call_fn,
        reason = "constructor keeps mutable extension guard wrapping local to the registry module"
    )]
    pub(super) const fn new(inner: RwLockWriteGuard<'a, ExtensionsInner>) -> Self {
        Self { inner }
    }
}

impl ExtensionsMut<'_> {
    /// Insert a type into this `Extensions`.
    ///
    /// Note that extensions are _not_
    /// `Layer`-specific—they are _span_-specific. This means that
    /// other layers can access and mutate extensions that
    /// a different Layer recorded. For example, an application might
    /// have a layer that records execution timings, alongside a layer
    /// that reports spans and events to a distributed
    /// tracing system that requires timestamps for spans.
    /// Ideally, if one layer records a timestamp _x_, the other layer
    /// should be able to reuse timestamp _x_.
    ///
    /// Therefore, extensions should generally be newtypes, rather than common
    /// types like [`String`](std::string::String), to avoid accidental
    /// cross-`Layer` clobbering.
    ///
    /// Returns `None` when the value was inserted. If `T` is already present in
    /// `Extensions`, the provided value is returned as `Some(T)` and the
    /// existing extension is left unchanged.
    pub fn insert<T: Send + Sync + 'static>(&mut self, extension: T) -> Option<T> {
        if self.inner.contains::<T>() {
            return Some(extension);
        }

        let _previous = self.inner.insert(extension);
        None
    }

    /// Replaces an existing `T` into this extensions.
    ///
    /// If `T` is not present, `Option::None` will be returned.
    pub fn replace<T: Send + Sync + 'static>(&mut self, extension: T) -> Option<T> {
        self.inner.insert(extension)
    }

    /// Get a mutable reference to a type previously inserted on this `ExtensionsMut`.
    pub fn get_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.inner.get_mut::<T>()
    }

    /// Remove a type from this `Extensions`.
    ///
    /// If a extension of this type existed, it will be returned.
    pub fn remove<T: Send + Sync + 'static>(&mut self) -> Option<T> {
        self.inner.remove::<T>()
    }
}

/// A type map of span extensions.
///
/// [`ExtensionsInner`] is used by [`SpanData`](super::SpanData) to store
/// span-specific data. A given `Layer` can read and write
/// data that it is interested in recording and emitting.
#[derive(Default)]
pub(super) struct ExtensionsInner {
    /// Stored extension values keyed by concrete type.
    map: AnyMap,
}

impl ExtensionsInner {
    /// Create an empty `Extensions`.
    #[cfg(any(test, feature = "registry"))]
    #[inline]
    #[allow(
        clippy::single_call_fn,
        reason = "constructor centralizes the concrete extension map representation"
    )]
    pub(super) fn new() -> Self {
        Self {
            map: AnyMap::default(),
        }
    }

    /// Insert a type into this `Extensions`.
    ///
    /// If a extension of this type already existed, it will
    /// be returned.
    pub(super) fn insert<T: Send + Sync + 'static>(&mut self, extension: T) -> Option<T> {
        self.map
            .insert(TypeId::of::<T>(), Box::new(extension))
            .and_then(|boxed_value| boxed_value.downcast().ok().map(|typed| *typed))
    }

    /// Get a reference to a type previously inserted on this `Extensions`.
    pub(super) fn get<T: 'static>(&self) -> Option<&T> {
        self.map
            .get(&TypeId::of::<T>())
            .and_then(|boxed| boxed.downcast_ref())
    }

    /// Returns `true` if this map contains a value of type `T`.
    pub(super) fn contains<T: 'static>(&self) -> bool {
        self.map.contains_key(&TypeId::of::<T>())
    }

    /// Get a mutable reference to a type previously inserted on this `Extensions`.
    pub(super) fn get_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.map
            .get_mut(&TypeId::of::<T>())
            .and_then(|boxed| boxed.downcast_mut())
    }

    /// Remove a type from this `Extensions`.
    ///
    /// If a extension of this type existed, it will be returned.
    pub(super) fn remove<T: Send + Sync + 'static>(&mut self) -> Option<T> {
        self.map.remove(&TypeId::of::<T>()).and_then(|boxed| {
            boxed.downcast().ok().map(|typed| *typed)
        })
    }

    /// Clear the `ExtensionsInner` in-place, dropping any elements in the map but
    /// retaining allocated capacity.
    ///
    /// This permits the hash map allocation to be pooled by the registry so
    /// that future spans will not need to allocate new hashmaps.
    #[cfg(any(test, feature = "registry"))]
    pub(super) fn clear(&mut self) {
        self.map.clear();
    }
}

impl fmt::Debug for ExtensionsInner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Extensions")
            .field("len", &self.map.len())
            .field("capacity", &self.map.capacity())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use strict_test_support::{TestFailure, ensure};

    #[derive(Debug, PartialEq)]
    struct MyType(i32);

    #[test]
    fn test_extensions() -> Result<(), TestFailure> {
        let mut extensions = ExtensionsInner::new();

        let _first_previous = extensions.insert(5_i32);
        let _second_previous = extensions.insert(MyType(10));

        ensure(extensions.get() == Some(&5_i32), "inserted i32 is readable")?;
        ensure(
            extensions.get_mut() == Some(&mut 5_i32),
            "inserted i32 is mutably readable",
        )?;

        ensure(
            extensions.remove::<i32>() == Some(5_i32),
            "inserted i32 is removable",
        )?;
        ensure(
            extensions.get::<i32>().is_none(),
            "removed i32 is no longer present",
        )?;

        ensure(
            extensions.get::<bool>().is_none(),
            "missing bool is not present",
        )?;
        ensure(
            extensions.get() == Some(&MyType(10)),
            "inserted custom type remains present",
        )
    }

    #[test]
    fn clear_retains_capacity() -> Result<(), TestFailure> {
        let mut extensions = ExtensionsInner::new();
        let _first_previous = extensions.insert(5_i32);
        let _second_previous = extensions.insert(MyType(10));
        let _third_previous = extensions.insert(true);

        ensure(extensions.map.len() == 3, "extensions store three entries")?;
        let prev_capacity = extensions.map.capacity();
        extensions.clear();

        ensure(
            extensions.map.is_empty(),
            "after clear(), extensions map should have length 0",
        )?;
        ensure(
            extensions.map.capacity() == prev_capacity,
            "after clear(), extensions map should retain prior capacity",
        )
    }

    #[test]
    fn clear_drops_elements() -> Result<(), TestFailure> {
        use std::sync::Arc;
        struct DropMePlease(Arc<()>);
        struct DropMeTooPlease(Arc<()>);

        let mut extensions = ExtensionsInner::new();
        let val1 = DropMePlease(Arc::new(()));
        let val2 = DropMeTooPlease(Arc::new(()));

        let val1_dropped = Arc::downgrade(&val1.0);
        let val2_dropped = Arc::downgrade(&val2.0);
        let _first_previous = extensions.insert(val1);
        let _second_previous = extensions.insert(val2);

        ensure(
            val1_dropped.upgrade().is_some(),
            "first value is live before clear",
        )?;
        ensure(
            val2_dropped.upgrade().is_some(),
            "second value is live before clear",
        )?;

        extensions.clear();
        ensure(
            val1_dropped.upgrade().is_none(),
            "after clear(), val1 should be dropped",
        )?;
        ensure(
            val2_dropped.upgrade().is_none(),
            "after clear(), val2 should be dropped",
        )
    }
}
