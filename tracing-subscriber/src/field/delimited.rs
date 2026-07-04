//! A `MakeVisitor` wrapper that separates formatted fields with a delimiter.
use core::fmt;

use tracing_core::field::Field;
use tracing_core::field::Visit;

use super::MakeVisitor;
use super::VisitFmt;
use super::VisitOutput;

/// A `MakeVisitor` wrapper that wraps a visitor that writes formatted output so
/// that a delimiter is inserted between writing formatted field values.
#[derive(Debug, Clone)]
pub struct Delimited<D, V> {
  /// The string-like delimiter inserted between formatted fields.
  delimiter: D,
  /// The wrapped visitor factory.
  inner:     V,
}

/// A visitor wrapper that inserts a delimiter after the wrapped visitor formats
/// a field value.
#[derive(Debug)]
pub struct VisitDelimited<D, V> {
  /// The string-like delimiter inserted between formatted fields.
  delimiter: D,
  /// Whether any field has already been formatted.
  seen:      bool,
  /// The wrapped field visitor.
  inner:     V,
  /// The first formatting error returned while writing delimiters.
  err:       fmt::Result,
}

// === impl Delimited ===

impl<D, V, T> MakeVisitor<T> for Delimited<D, V>
where
  D: AsRef<str> + Clone,
  V: MakeVisitor<T>,
  V::Visitor: VisitFmt,
{
  type Visitor = VisitDelimited<D, V::Visitor>;
  fn make_visitor(&self, target: T) -> Self::Visitor {
    let inner = self.inner.make_visitor(target);
    VisitDelimited::new(self.delimiter.clone(), inner)
  }
}

impl<D, V> Delimited<D, V> {
  /// Returns a new [`MakeVisitor`] implementation that wraps `inner` so that
  /// it will format each visited field separated by the provided `delimiter`.
  ///
  /// [`MakeVisitor`]: super::MakeVisitor
  #[allow(
    clippy::single_call_fn,
    reason = "public field visitor constructor is part of the formatting extension API"
  )]
  pub const fn new(delimiter: D, inner: V) -> Self {
    Self {
      delimiter,
      inner,
    }
  }
}

// === impl VisitDelimited ===

impl<D, V> VisitDelimited<D, V> {
  /// Returns a new [`Visit`] implementation that wraps `inner` so that
  /// each formatted field is separated by the provided `delimiter`.
  ///
  /// [`Visit`]: tracing_core::field::Visit
  #[allow(
    clippy::single_call_fn,
    reason = "visitor constructor preserves the field formatting wrapper boundary"
  )]
  pub const fn new(delimiter: D, inner: V) -> Self {
    Self {
      delimiter,
      inner,
      seen: false,
      err: Ok(()),
    }
  }

  /// Writes a delimiter before the current field when a prior field was
  /// formatted successfully.
  fn delimit(&mut self)
  where
    V: VisitFmt,
    D: AsRef<str>,
  {
    if self.err.is_err() {
      return;
    }

    if self.seen {
      self.err = self.inner.writer().write_str(self.delimiter.as_ref());
    }

    self.seen = true;
  }
}

impl<D, V> Visit for VisitDelimited<D, V>
where
  V: VisitFmt,
  D: AsRef<str>,
{
  fn record_i64(&mut self, field: &Field, value: i64) {
    self.delimit();
    self.inner.record_i64(field, value);
  }

  fn record_u64(&mut self, field: &Field, value: u64) {
    self.delimit();
    self.inner.record_u64(field, value);
  }

  fn record_bool(&mut self, field: &Field, value: bool) {
    self.delimit();
    self.inner.record_bool(field, value);
  }

  fn record_str(&mut self, field: &Field, value: &str) {
    self.delimit();
    self.inner.record_str(field, value);
  }

  fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
    self.delimit();
    self.inner.record_debug(field, value);
  }
}

impl<D, V> VisitOutput<fmt::Result> for VisitDelimited<D, V>
where
  V: VisitFmt,
  D: AsRef<str>,
{
  fn finish(self) -> fmt::Result {
    self.err?;
    self.inner.finish()
  }
}

impl<D, V> VisitFmt for VisitDelimited<D, V>
where
  V: VisitFmt,
  D: AsRef<str>,
{
  fn writer(&mut self) -> &mut dyn fmt::Write {
    self.inner.writer()
  }
}

#[cfg(test)]
#[cfg(all(test, feature = "alloc"))]
mod test {
  use strict_test_support::TestFailure;
  use strict_test_support::ensure;
  use strict_test_support::ensure_ok;

  use super::*;
  use crate::field::test_util::*;

  #[test]
  fn delimited_visitor() -> Result<(), TestFailure> {
    let mut output = String::new();
    let debug_visitor = DebugVisitor::new(&mut output);
    let mut visitor = VisitDelimited::new(", ", debug_visitor);

    TestAttrs1::with(|attrs| attrs.record(&mut visitor))?;
    ensure_ok(visitor.finish(), "delimited visitor should finish")?;

    ensure(
      output.as_str() == "question=\"life, the universe, and everything\", tricky=true, can_you_do_it=true",
      "delimited fields render with comma separators",
    )
  }

  #[test]
  fn delimited_new_visitor() -> Result<(), TestFailure> {
    let make = Delimited::new("; ", MakeDebug);

    TestAttrs1::with(|attrs| -> Result<(), TestFailure> {
      let mut output = String::new();
      {
        let mut visitor = make.make_visitor(&mut output);
        attrs.record(&mut visitor);
      };
      ensure(
        output.as_str() == "question=\"life, the universe, and everything\"; tricky=true; can_you_do_it=true",
        "first attribute set renders with semicolon separators",
      )
    })??;

    TestAttrs2::with(|attrs| -> Result<(), TestFailure> {
      let mut output = String::new();
      {
        let mut visitor = make.make_visitor(&mut output);
        attrs.record(&mut visitor);
      };
      ensure(
        output.as_str() == "question=None; question.answer=42; tricky=true; can_you_do_it=false",
        "second attribute set renders with semicolon separators",
      )
    })?
  }
}
