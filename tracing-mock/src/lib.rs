#![doc = "Testing utilities for `tracing` diagnostics."]
#![cfg_attr(docsrs, feature(doc_cfg), deny(rustdoc::broken_intra_doc_links))]
pub mod ancestry;
pub mod event;
pub mod expect;
#[doc(hidden)]
pub mod failure;
pub mod field;
/// Shared metadata expectation helpers.
#[doc(hidden)]
pub mod metadata;
pub mod span;
pub mod subscriber;

/// Layer-based mocks for validating traces inside a `tracing-subscriber` stack.
#[cfg(feature = "tracing-subscriber")]
pub mod layer;
