use alloc::vec::Vec;
use tracing_core::span::Id;

/// A span ID entered on the current thread.
#[derive(Debug)]
struct ContextId {
    /// The entered span ID.
    id: Id,
    /// Whether this ID already existed deeper in the stack when pushed.
    duplicate: bool,
}

/// `SpanStack` tracks what spans are currently executing on a thread-local basis.
///
/// A "separate current span" for each thread is a semantic choice, as each span
/// can be executing in a different thread.
#[derive(Debug, Default)]
pub(super) struct SpanStack {
    /// Entered span IDs, ordered from oldest to newest.
    stack: Vec<ContextId>,
}

impl SpanStack {
    /// Pushes a span ID onto this stack.
    #[inline]
    pub(super) fn push(&mut self, id: Id) {
        let duplicate = self.stack.iter().any(|entry| entry.id == id);
        self.stack.push(ContextId { id, duplicate });
    }

    /// Removes the newest matching span ID from this stack.
    #[inline]
    pub(super) fn pop(&mut self, expected_id: Id) -> bool {
        let mut above_match = Vec::new();
        let mut removed_duplicate = None;

        while let Some(entry) = self.stack.pop() {
            if entry.id == expected_id {
                removed_duplicate = Some(entry.duplicate);
                break;
            }

            above_match.push(entry);
        }

        while let Some(entry) = above_match.pop() {
            self.stack.push(entry);
        }

        removed_duplicate.is_some_and(|duplicate| !duplicate)
    }

    /// Iterates over the unique current span IDs from newest to oldest.
    #[inline]
    pub(super) fn iter(&self) -> impl Iterator<Item = &Id> {
        self.stack
            .iter()
            .rev()
            .filter_map(|entry| (!entry.duplicate).then_some(&entry.id))
    }

    /// Returns the newest unique current span ID.
    #[inline]
    pub(super) fn current(&self) -> Option<&Id> {
        self.iter().next()
    }
}

#[cfg(test)]
mod tests {
    use super::SpanStack;
    use strict_test_support::{TestFailure, ensure};
    use core::num::NonZeroU64;
    use tracing_core::span::Id;

    const FIRST_SPAN_ID: Id = Id::from_non_zero_u64(NonZeroU64::MIN);
    const SECOND_SPAN_ID: Id = match Id::try_from_u64(2) {
        Some(id) => id,
        None => Id::from_non_zero_u64(NonZeroU64::MIN),
    };

    #[test]
    fn pop_last_span() -> Result<(), TestFailure> {
        let mut stack = SpanStack::default();
        let span_id = FIRST_SPAN_ID;
        stack.push(span_id);

        ensure(stack.pop(span_id), "last span can be popped")
    }

    #[test]
    fn pop_first_span() -> Result<(), TestFailure> {
        let mut stack = SpanStack::default();
        stack.push(FIRST_SPAN_ID);
        stack.push(SECOND_SPAN_ID);

        let span_id = FIRST_SPAN_ID;
        ensure(stack.pop(span_id), "first span can be popped")
    }
}
