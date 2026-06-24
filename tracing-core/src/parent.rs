use crate::span::Id;

/// Parent relationship requested for a new span or event.
#[derive(Debug)]
pub enum Parent {
    /// The new span will be a root span.
    Root,
    /// The new span will be rooted in the current span.
    Current,
    /// The new span has an explicitly-specified parent.
    Explicit(Id),
}
