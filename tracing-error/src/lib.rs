//! Utilities for enriching error handling with [`tracing`] diagnostic
//! information.
//!
//! # Overview
//!
//! [`tracing`] is a framework for instrumenting Rust programs to collect
//! scoped, structured, and async-aware diagnostics. This crate provides
//! integrations between [`tracing`] instrumentation and Rust error handling. It
//! enables enriching error types with diagnostic information from `tracing`
//! [span] contexts, formatting those contexts when errors are displayed, and
//! automatically generate `tracing` [events] when errors occur.
//!
//! The crate provides the following:
//!
//! * [`SpanTrace`], a captured trace of the current `tracing` [span] context
//!
//! * [`ErrorLayer`], a [subscriber layer] which enables capturing `SpanTrace`s
//!
//! **Note**: This crate is currently experimental.
//!
//! *Compiler support: [requires `rustc` 1.96+][msrv]*
//!
//! [msrv]: #supported-rust-versions
//!
//! ## Feature Flags
//!
//! - `traced-error` - Enables the [`TracedError`] type and related Traits
//!     - [`InstrumentResult`] and [`InstrumentError`] extension traits, which provide an
//!       [`in_current_span()`] method for bundling errors with a [`SpanTrace`].
//!     - [`ExtractSpanTrace`] extension trait, for extracting `SpanTrace`s from behind `dyn Error`
//!       trait objects.
//!
//! ## Usage
//!
//! `tracing-error` provides the [`SpanTrace`] type, which captures the current
//! `tracing` span context when it is constructed and allows it to be displayed
//! at a later time.
//!
//! For example:
//!
//! ```rust
//! use std::error::Error;
//! use std::fmt;
//!
//! use tracing_error::SpanTrace;
//!
//! #[derive(Debug)]
//! pub struct MyError {
//!   context: SpanTrace,
//!   // ...
//! }
//!
//! impl fmt::Display for MyError {
//!   fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
//!     // ... format other parts of the error ...
//!
//!     self.context.fmt(f)?;
//!
//!     // ... format other error context information, cause chain, etc ...
//!         # Ok(())
//!   }
//! }
//!
//! impl Error for MyError {}
//!
//! impl MyError {
//!   pub fn new() -> Self {
//!     Self {
//!       context: SpanTrace::capture(),
//!       // ... other error information ...
//!     }
//!   }
//! }
//! ```
//!
//! This crate also provides [`TracedError`], for attaching a [`SpanTrace`] to
//! an existing error. The easiest way to wrap errors in `TracedError` is to
//! either use the [`InstrumentResult`] and [`InstrumentError`] traits or the
//! `From`/`Into` traits.
//!
//! ```rust
//! # use std::error::Error;
//! use tracing_error::prelude::*;
//!
//! # fn fake_main() -> Result<(), Box<dyn Error>> {
//! std::fs::read_to_string("myfile.txt").in_current_span()?;
//! # Ok(())
//! # }
//! ```
//!
//! Once an error has been wrapped with with a [`TracedError`] the [`SpanTrace`]
//! can be extracted one of 3 ways: either via [`TracedError`]'s
//! `Display`/`Debug` implementations, or via the [`ExtractSpanTrace`] trait.
//!
//! For example, here is how one might print the errors but specialize the
//! printing when the error is a placeholder for a wrapping [`SpanTrace`]:
//!
//! ```rust
//! use std::error::Error;
//!
//! use tracing_error::ExtractSpanTrace as _;
//!
//! fn print_extracted_spantraces(error: &(dyn Error + 'static)) {
//!   let mut error = Some(error);
//!   let mut ind = 0;
//!
//!   eprintln!("Error:");
//!
//!   while let Some(err) = error {
//!     if let Some(spantrace) = err.span_trace() {
//!       eprintln!("found a spantrace:\n{}", spantrace);
//!     } else {
//!       eprintln!("{:>4}: {}", ind, err);
//!     }
//!
//!     error = err.source();
//!     ind += 1;
//!   }
//! }
//! ```
//!
//! Whereas here, we can still display the content of the `SpanTraces` without
//! any special casing by simply printing all errors in our error chain.
//!
//! ```rust
//! use std::error::Error;
//!
//! fn print_naive_spantraces(error: &(dyn Error + 'static)) {
//!   let mut error = Some(error);
//!   let mut ind = 0;
//!
//!   eprintln!("Error:");
//!
//!   while let Some(err) = error {
//!     eprintln!("{:>4}: {}", ind, err);
//!     error = err.source();
//!     ind += 1;
//!   }
//! }
//! ```
//!
//! Applications that wish to use `tracing-error`-enabled errors should
//! construct an [`ErrorLayer`] and add it to their [`Subscriber`] in order to
//! enable capturing [`SpanTrace`]s. For example:
//!
//! ```rust
//! use tracing_error::ErrorLayer;
//! use tracing_subscriber::prelude::*;
//!
//! fn main() {
//!   let subscriber = tracing_subscriber::Registry::default()
//!         // any number of other subscriber layers may be added before or
//!         // after the `ErrorLayer`...
//!         .with(ErrorLayer::default());
//!
//!   // set the subscriber as the default for the application
//!   tracing::subscriber::set_global_default(subscriber);
//! }
//! ```
//!
//! [`in_current_span()`]: InstrumentResult#tymethod.in_current_span
//! [span]: mod@tracing::span
//! [events]: tracing::Event
//! [`Subscriber`]: tracing::Subscriber
//! [subscriber layer]: tracing_subscriber::layer::Layer
//! [`tracing`]: tracing
//!
//! ## Supported Rust Versions
//!
//! Tracing is built against the latest stable release. The minimum supported
//! version is 1.96. The current Tracing version is not guaranteed to build on
//! Rust versions earlier than the minimum supported version.
//!
//! Tracing follows the same compiler support policies as the rest of the Tokio
//! project. The current stable Rust compiler and the three most recent minor
//! versions before it will always be supported. For example, if the current
//! stable compiler version is 1.69, the minimum supported version will not be
//! increased past 1.66, three minor versions prior. Increasing the minimum
//! supported compiler version is not considered a semver breaking change as
//! long as doing so complies with this policy.
#![cfg_attr(docsrs, feature(doc_cfg), deny(rustdoc::broken_intra_doc_links))]
#![doc(
  html_logo_url = "https://raw.githubusercontent.com/tokio-rs/tracing/main/assets/logo-type.png",
  html_favicon_url = "https://raw.githubusercontent.com/tokio-rs/tracing/main/assets/favicon.ico",
  issue_tracker_base_url = "https://github.com/strict-rs/strict-tracing/issues/"
)]
use std::borrow::Borrow;
use std::fmt;

use tracing::Dispatch;
use tracing::Metadata;
use tracing::span;

/// Visitor invoked for each span in a captured trace.
type SpanTraceVisitor<'a> = dyn FnMut(&'static Metadata<'static>, &str) -> bool + 'a;

/// Type-erased callback used to walk span context for a captured span.
type WithContextCallback = fn(&Dispatch, span::Id, visitor: &mut SpanTraceVisitor<'_>);

/// Type-erased callback for walking span context after a `SpanTrace` capture.
///
/// This function remembers the types of the subscriber and the formatter, so
/// that callers can downcast to something aware of them without knowing those
/// types at the callsite.
pub(crate) struct WithContext(
  /// Invokes the subscriber-specific context walker for a captured span.
  WithContextCallback,
);

impl fmt::Debug for WithContext {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.debug_struct("WithContext").finish_non_exhaustive()
  }
}

impl WithContext {
  /// Builds a type-erased callback for visiting formatted span context.
  #[allow(
    clippy::single_call_fn,
    reason = "constructor keeps the type-erased callback field private across sibling modules"
  )]
  pub(crate) const fn new(callback: WithContextCallback) -> Self {
    Self(callback)
  }

  /// Visits formatted span context with this type-erased callback.
  pub(crate) fn with_context(
    &self,
    dispatch: &Dispatch,
    id: impl Borrow<span::Id>,
    mut visitor: impl FnMut(&'static Metadata<'static>, &str) -> bool,
  ) {
    (self.0)(dispatch, *id.borrow(), &mut visitor);
  }
}

mod backtrace;
#[cfg(feature = "traced-error")]
mod error;
#[path = "layer.rs"]
mod layer_impl;
pub use self::backtrace::SpanTrace;
pub use self::backtrace::SpanTraceStatus;
#[cfg(feature = "traced-error")]
pub use self::error::ExtractSpanTrace;
#[cfg(feature = "traced-error")]
pub use self::error::InstrumentError;
#[cfg(feature = "traced-error")]
pub use self::error::InstrumentResult;
#[cfg(feature = "traced-error")]
pub use self::error::TracedError;
pub use self::layer_impl::ErrorLayer;
/// Crate-root alias for internal bridge types shared by sibling modules.
pub(crate) use crate as layer;

#[cfg(feature = "traced-error")]
#[cfg_attr(docsrs, doc(cfg(feature = "traced-error")))]
pub mod prelude {
  //! The `tracing-error` prelude.
  //!
  //! This brings into scope the `InstrumentError`, `InstrumentResult`, and `ExtractSpanTrace`
  //! extension traits. These traits allow attaching `SpanTrace`s to errors and
  //! subsequently retrieving them from `dyn Error` trait objects.

  pub use crate::ExtractSpanTrace as _;
  pub use crate::InstrumentError as _;
  pub use crate::InstrumentResult as _;
}

#[cfg(test)]
mod tests {
  use std::format;
  use std::num::NonZeroU64;
  use std::string::String;
  use std::vec::Vec;

  use strict_test_support::TestFailure;
  use strict_test_support::ensure;
  use strict_test_support::ensure_contains;
  use strict_test_support::ensure_eq;
  use strict_test_support::ensure_some;
  use tracing::Dispatch;
  use tracing::Level;
  use tracing::Metadata;
  use tracing::callsite::Callsite;
  use tracing::callsite::Identifier;
  use tracing::field::FieldSet;
  use tracing::metadata::Kind;
  use tracing::metadata::SourceLocation;
  use tracing::span;
  use tracing::subscriber::Interest;
  use tracing::subscriber::NoSubscriber;

  use super::*;

  struct ContextCallsite;

  static CONTEXT_CALLSITE: ContextCallsite = ContextCallsite;
  static CONTEXT_METADATA: Metadata<'static> = Metadata::new(
    "context_span",
    "context_target",
    Level::INFO,
    &SourceLocation::empty(),
    &FieldSet::new(&["answer"], Identifier(&CONTEXT_CALLSITE)),
    Kind::SPAN,
  );

  impl Callsite for ContextCallsite {
    fn set_interest(&self, _: Interest) {}

    fn metadata(&self) -> &Metadata<'_> {
      &CONTEXT_METADATA
    }
  }

  fn context_span_id() -> span::Id {
    span::Id::from_non_zero_u64(NonZeroU64::MIN)
  }

  fn visit_context_for_test(_dispatch: &Dispatch, id: span::Id, visitor: &mut SpanTraceVisitor<'_>) {
    if id != context_span_id() {
      return;
    }

    if visitor(&CONTEXT_METADATA, "answer=42") {
      let _continued = visitor(&CONTEXT_METADATA, "second=true");
    }
  }

  #[test]
  fn with_context_debug_names_erased_bridge() -> Result<(), TestFailure> {
    let bridge = WithContext::new(visit_context_for_test);
    let debugged = format!("{bridge:?}");
    ensure_contains(&debugged, "WithContext", "debug output should name the erased context bridge")
  }

  #[test]
  fn with_context_visits_metadata_and_honors_early_stop() -> Result<(), TestFailure> {
    let bridge = WithContext::new(visit_context_for_test);
    let dispatch = Dispatch::new(NoSubscriber::new());
    let context_id = context_span_id();
    let mut visits = Vec::new();
    bridge.with_context(&dispatch, context_id, |metadata, fields| {
      visits.push((String::from(metadata.name()), String::from(fields)));
      true
    });

    ensure_eq(&visits.len(), &2_usize, "continuing visitor should receive both callback entries")?;
    let first = ensure_some(visits.first(), "first bridge visit should be present")?;
    ensure_eq(&first.0, &String::from("context_span"), "bridge visitor should receive metadata")?;
    ensure_eq(
      &first.1,
      &String::from("answer=42"),
      "bridge visitor should receive formatted fields",
    )?;
    let second = ensure_some(visits.get(1), "second bridge visit should be present")?;
    ensure_eq(
      &second.1,
      &String::from("second=true"),
      "bridge should continue while the visitor returns true",
    )?;

    let mut stopped = Vec::new();
    bridge.with_context(&dispatch, context_id, |metadata, fields| {
      stopped.push((String::from(metadata.name()), String::from(fields)));
      false
    });
    ensure_eq(&stopped.len(), &1_usize, "false from the visitor should stop callback iteration")?;

    let mut ignored = Vec::new();
    bridge.with_context(&dispatch, span::Id::from_non_zero_u64(NonZeroU64::MAX), |metadata, fields| {
      ignored.push((String::from(metadata.name()), String::from(fields)));
      true
    });
    ensure(ignored.is_empty(), "callback should not visit metadata for another span ID")
  }
}
