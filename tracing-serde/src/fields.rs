//! Support for serializing fields as `serde` structs or maps.
use serde::{Serialize, ser::Serializer};
use tracing_core::{
    event::Event,
    span::{Attributes, Record},
};

use super::{SerdeMapVisitor, sealed};

/// A `serde::Serialize` adapter that records tracing fields as a map.
#[derive(Debug)]
pub struct SerializeFieldMap<'a, T>(&'a T);

/// Converts tracing values with fields into map-shaped serialization adapters.
pub trait AsMap: Sized + sealed::Sealed {
    /// Returns a map-shaped serialization adapter for the value's fields.
    fn field_map(&self) -> SerializeFieldMap<'_, Self> {
        SerializeFieldMap(self)
    }
}

impl AsMap for Event<'_> {}
impl AsMap for Attributes<'_> {}
impl AsMap for Record<'_> {}

// === impl SerializeFieldMap ===

impl Serialize for SerializeFieldMap<'_, Event<'_>> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let len = self.0.fields().count();
        let map = serializer.serialize_map(Some(len))?;
        let mut visitor = SerdeMapVisitor::new(map);
        self.0.record(&mut visitor);
        visitor.finish()
    }
}

impl Serialize for SerializeFieldMap<'_, Attributes<'_>> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let len = self.0.metadata().fields().len();
        let map = serializer.serialize_map(Some(len))?;
        let mut visitor = SerdeMapVisitor::new(map);
        self.0.record(&mut visitor);
        visitor.finish()
    }
}

impl Serialize for SerializeFieldMap<'_, Record<'_>> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let map = serializer.serialize_map(None)?;
        let mut visitor = SerdeMapVisitor::new(map);
        self.0.record(&mut visitor);
        visitor.finish()
    }
}
