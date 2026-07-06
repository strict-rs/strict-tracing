//! `Span` and `Event` key-value data.
//!
//! Spans and events may be annotated with key-value data, referred to as _fields_. This module
//! re-exports the field vocabulary defined by [`tracing_core::field`]: the [`Value`] trait
//! implemented by recordable types, the [`Visit`] trait implemented by subscribers to receive
//! typed values, the [`Field`] key type, and helpers such as [`display()`], [`debug()`], and
//! [`Empty`].
//!
//! On top of that shared model, this module defines the [`AsField`] trait, which lets a span
//! field be looked up either by a previously resolved [`Field`] key (constant-time access) or
//! by its string name (an iterative search).
//!
//! See the [`tracing_core::field`] documentation for the full data model, including how values
//! are recorded and the experimental [`valuable`] integration. The `valuable()` conversion
//! function and the optional `Visit::record_value` method it feeds are available only with the
//! unstable `valuable` feature.
//!
//! [`valuable`]: https://crates.io/crates/valuable
pub use tracing_core::field::*;

use crate::Metadata;
use crate::sealed::Sealed;

/// Trait implemented to allow a type to be used as a field key.
///
/// <pre class="ignore" style="white-space:normal;font:inherit;">
/// <strong>Note</strong>: Although this is implemented for both the
/// <a href="./struct.Field.html"><code>Field</code></a> type <em>and</em> any
/// type that can be borrowed as an <code>&str</code>, only <code>Field</code>
/// allows <em>O</em>(1) access.
/// Indexing a field with a string results in an iterative search that performs
/// string comparisons. Thus, if possible, once the key for a field is known, it
/// should be used whenever possible.
/// </pre>
pub trait AsField: Sealed {
  /// Attempts to convert `&self` into a `Field` with the specified `metadata`.
  ///
  /// If `metadata` defines this field, then the field is returned. Otherwise,
  /// this returns `None`.
  fn as_field(&self, metadata: &Metadata<'_>) -> Option<Field>;
}

// ===== impl AsField =====

impl AsField for Field {
  #[inline]
  fn as_field(&self, metadata: &Metadata<'_>) -> Option<Field> {
    (self.callsite() == metadata.callsite()).then_some(*self)
  }
}

impl AsField for &Field {
  #[inline]
  fn as_field(&self, metadata: &Metadata<'_>) -> Option<Field> {
    (self.callsite() == metadata.callsite()).then_some(**self)
  }
}

impl AsField for str {
  #[inline]
  fn as_field(&self, metadata: &Metadata<'_>) -> Option<Field> {
    metadata.fields().field(&self)
  }
}

impl Sealed for Field {}
impl Sealed for &Field {}
impl Sealed for str {}

#[cfg(test)]
mod tests {
  use strict_test_support::TestFailure;
  use strict_test_support::ensure;
  use strict_test_support::ensure_eq;
  use strict_test_support::ensure_some;

  use super::AsField as _;
  use crate::__macro_support::MacroCallsite;
  use crate::Level;
  use crate::Metadata;
  use crate::metadata::Kind;

  static FIRST_CALLSITE: MacroCallsite = MacroCallsite::new(&FIRST_METADATA);
  static FIRST_METADATA: Metadata<'static> = crate::metadata! {
      name: "first",
      target: module_path!(),
      level: Level::INFO,
      fields: &["alpha", "beta"],
      callsite: &FIRST_CALLSITE,
      kind: Kind::SPAN,
  };

  static SECOND_CALLSITE: MacroCallsite = MacroCallsite::new(&SECOND_METADATA);
  static SECOND_METADATA: Metadata<'static> = crate::metadata! {
      name: "second",
      target: module_path!(),
      level: Level::INFO,
      fields: &["alpha", "gamma"],
      callsite: &SECOND_CALLSITE,
      kind: Kind::SPAN,
  };

  #[test]
  fn string_keys_find_matching_fields_and_reject_missing_fields() -> Result<(), TestFailure> {
    let alpha = ensure_some("alpha".as_field(&FIRST_METADATA), "string key finds matching field")?;
    let beta = ensure_some("beta".as_field(&FIRST_METADATA), "string key finds second matching field")?;

    ensure_eq(&alpha.name(), &"alpha", "string lookup returns the requested field")?;
    ensure_eq(&beta.name(), &"beta", "string lookup returns the second requested field")?;
    ensure("gamma".as_field(&FIRST_METADATA).is_none(), "string lookup rejects missing fields")
  }

  #[test]
  fn resolved_fields_match_only_their_original_callsite() -> Result<(), TestFailure> {
    let alpha = ensure_some("alpha".as_field(&FIRST_METADATA), "alpha field exists")?;
    let alpha_ref = &alpha;
    let same = ensure_some(alpha.as_field(&FIRST_METADATA), "field key matches its original metadata")?;
    let by_ref = ensure_some(alpha_ref.as_field(&FIRST_METADATA), "field reference matches its original metadata")?;

    ensure_eq(&same, &alpha, "field key returns itself for matching metadata")?;
    ensure_eq(&by_ref, &alpha, "field reference returns the copied field for matching metadata")?;
    ensure(alpha.as_field(&SECOND_METADATA).is_none(), "field key rejects a different callsite")?;
    ensure(
      alpha_ref.as_field(&SECOND_METADATA).is_none(),
      "field reference rejects a different callsite",
    )
  }
}
