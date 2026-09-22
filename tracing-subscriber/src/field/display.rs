//! `MakeVisitor` wrappers for working with `fmt::Display` fields.
use core::fmt;

use tracing_core::field::Field;
use tracing_core::field::Visit;

use super::MakeVisitor;
use super::VisitFmt;
use super::VisitOutput;

/// A visitor wrapper that ensures any strings named "message" are formatted
/// using `fmt::Display`
#[derive(Debug, Clone)]
pub struct Messages<V>(V);

// TODO(eliza): When `error` as a primitive type is stable, add a
// `DisplayErrors` wrapper...

// === impl Messages ===
//
impl<V> Messages<V> {
  /// Returns a new [`MakeVisitor`] implementation that will wrap `inner` so
  /// that any strings named `message` are formatted using `fmt::Display`.
  ///
  /// [`MakeVisitor`]: super::MakeVisitor
  #[allow(
    clippy::single_call_fn,
    reason = "public field visitor constructor is part of the formatting extension API"
  )]
  pub const fn new(inner: V) -> Self {
    Self(inner)
  }
}

impl<T, V> MakeVisitor<T> for Messages<V>
where
  V: MakeVisitor<T>,
{
  type Visitor = Messages<V::Visitor>;

  #[inline]
  fn make_visitor(&self, target: T) -> Self::Visitor {
    Messages(self.0.make_visitor(target))
  }
}

impl<V> Visit for Messages<V>
where
  V: Visit,
{
  #[inline]
  fn record_f64(&mut self, field: &Field, field_value: f64) {
    self.0.record_f64(field, field_value);
  }

  #[inline]
  fn record_i64(&mut self, field: &Field, field_value: i64) {
    self.0.record_i64(field, field_value);
  }

  #[inline]
  fn record_u64(&mut self, field: &Field, field_value: u64) {
    self.0.record_u64(field, field_value);
  }

  #[inline]
  fn record_bool(&mut self, field: &Field, field_value: bool) {
    self.0.record_bool(field, field_value);
  }

  /// Visit a string value.
  fn record_str(&mut self, field: &Field, field_value: &str) {
    if field.name() == "message" {
      self.0.record_debug(field, &format_args!("{field_value}"));
    } else {
      self.0.record_str(field, field_value);
    }
  }

  // TODO(eliza): add RecordError when stable
  // fn record_error(&mut self, field: &Field, value: &(dyn std::error::Error + 'static)) {
  //     self.record_debug(field, &format_args!("{}", value))
  // }

  #[inline]
  fn record_debug(&mut self, field: &Field, field_value: &dyn fmt::Debug) {
    self.0.record_debug(field, field_value);
  }
}

impl<V, O> VisitOutput<O> for Messages<V>
where
  V: VisitOutput<O>,
{
  #[inline]
  fn finish(self) -> O {
    self.0.finish()
  }
}

feature! {
    #![feature = "std"]
    use super::VisitWrite;
    use std::io;

    impl<V> VisitWrite for Messages<V>
    where
        V: VisitWrite,
    {
        #[inline]
        fn writer(&mut self) -> &mut dyn io::Write {
            self.0.writer()
        }
    }
}

impl<V> VisitFmt for Messages<V>
where
  V: VisitFmt,
{
  #[inline]
  fn writer(&mut self) -> &mut dyn fmt::Write {
    self.0.writer()
  }
}

#[cfg(test)]
#[cfg(feature = "std")]
mod tests {
  use std::format;
  use std::io;
  use std::string::String;
  use std::vec::Vec;

  /// Native failures from these behavioral checks.
  #[derive(Debug, thiserror::Error)]
  enum TestError {
    /// A boolean expectation failed.
    #[error(transparent)]
    Condition(#[from] strict_test_support::ConditionFailure),
    /// Retains the searched text and expected substring.
    #[error(transparent)]
    Substring(#[from] strict_test_support::SubstringFailure<String, String>),
    /// Preserves the complete native failure and its inputs.
    #[error(transparent)]
    ComparisonString(#[from] strict_test_support::ComparisonFailure<String, String>),
    /// Preserves the complete native failure and its inputs.
    #[error(transparent)]
    OptionField(#[from] strict_test_support::OptionFailure<Field>),
    /// Preserves the complete native failure and its inputs.
    #[error(transparent)]
    ResultError(#[from] strict_test_support::ResultFailure<io::Error>),
  }

  use strict_test_support::ensure;
  use strict_test_support::ensure_contains;
  use strict_test_support::ensure_eq;
  use strict_test_support::ensure_lacks;
  use strict_test_support::ensure_ok;
  use strict_test_support::ensure_some;
  use tracing_core::callsite::Callsite;
  use tracing_core::metadata::Kind;
  use tracing_core::metadata::Level;
  use tracing_core::metadata::Metadata;
  use tracing_core::subscriber::Interest;

  use super::*;
  use crate::field::VisitWrite;
  use crate::field::test_util::DebugVisitor;

  struct TestCallsite;

  static TEST_CALLSITE: TestCallsite = TestCallsite;
  static TEST_META: Metadata<'static> = tracing_core::metadata! {
      name: "display_field_tests",
      target: module_path!(),
      level: Level::INFO,
      fields: &["message", "answer", "other"],
      callsite: &TEST_CALLSITE,
      kind: Kind::EVENT,
  };

  impl Callsite for TestCallsite {
    fn set_interest(&self, _: Interest) {}

    fn metadata(&self) -> &Metadata<'_> {
      &TEST_META
    }
  }

  #[derive(Debug, Default)]
  struct RecordingVisitor {
    records: Vec<String>,
  }

  impl Visit for RecordingVisitor {
    fn record_f64(&mut self, field: &Field, field_value: f64) {
      self.records.push(format!("{}=f64:{field_value}", field.name()));
    }

    fn record_i64(&mut self, field: &Field, field_value: i64) {
      self.records.push(format!("{}=i64:{field_value}", field.name()));
    }

    fn record_u64(&mut self, field: &Field, field_value: u64) {
      self.records.push(format!("{}=u64:{field_value}", field.name()));
    }

    fn record_bool(&mut self, field: &Field, field_value: bool) {
      self.records.push(format!("{}=bool:{field_value}", field.name()));
    }

    fn record_str(&mut self, field: &Field, field_value: &str) {
      self.records.push(format!("{}=str:{field_value}", field.name()));
    }

    fn record_debug(&mut self, field: &Field, _: &dyn fmt::Debug) {
      self.records.push(format!("{}=debug", field.name()));
    }
  }

  impl VisitOutput<String> for RecordingVisitor {
    fn finish(self) -> String {
      self.records.join("|")
    }
  }

  struct MakeRecording;

  impl MakeVisitor<()> for MakeRecording {
    type Visitor = RecordingVisitor;

    fn make_visitor(&self, (): ()) -> Self::Visitor {
      RecordingVisitor::default()
    }
  }

  #[derive(Debug, Default)]
  struct FmtVisitor {
    writer: String,
  }

  impl Visit for FmtVisitor {
    fn record_debug(&mut self, _: &Field, _: &dyn fmt::Debug) {}
  }

  impl VisitOutput<fmt::Result> for FmtVisitor {
    fn finish(self) -> fmt::Result {
      Ok(())
    }
  }

  impl VisitFmt for FmtVisitor {
    fn writer(&mut self) -> &mut dyn fmt::Write {
      &mut self.writer
    }
  }

  #[derive(Debug, Default)]
  struct IoVisitor {
    writer: Vec<u8>,
  }

  impl Visit for IoVisitor {
    fn record_debug(&mut self, _: &Field, _: &dyn fmt::Debug) {}
  }

  impl VisitOutput<Result<(), io::Error>> for IoVisitor {
    fn finish(self) -> Result<(), io::Error> {
      Ok(())
    }
  }

  impl VisitWrite for IoVisitor {
    fn writer(&mut self) -> &mut dyn io::Write {
      &mut self.writer
    }
  }

  fn field_by_name(name: &'static str) -> Result<Field, TestError> {
    ensure_some(TEST_META.fields().field(name), "test field exists").map_err(TestError::from)
  }

  #[test]
  fn messages_formats_message_string_with_display() -> Result<(), TestError> {
    let field = field_by_name("message")?;
    let mut output = String::new();
    let mut visitor = Messages::new(DebugVisitor::new(&mut output));

    visitor.record_str(&field, "hello world");

    ensure(VisitOutput::<fmt::Result>::finish(visitor).is_ok(), "debug visitor should finish").map(drop)?;
    ensure_contains(
      output.clone(),
      String::from("message=hello world"),
      "message uses display formatting",
    )
    .map(drop)?;
    ensure_lacks(
      output.clone(),
      String::from("\"hello world\""),
      "message display formatting does not quote strings",
    )
    .map(drop)
    .map_err(TestError::from)
  }

  #[test]
  fn messages_forwards_non_message_strings_as_strings() -> Result<(), TestError> {
    let field = field_by_name("other")?;
    let mut visitor = Messages::new(RecordingVisitor::default());

    visitor.record_str(&field, "hello world");

    let expected = String::from("other=str:hello world");
    ensure_eq(visitor.finish(), expected, "non-message strings remain string values")
      .map(drop)
      .map_err(TestError::from)
  }

  #[test]
  fn messages_forwards_debug_and_numeric_values() -> Result<(), TestError> {
    let answer = field_by_name("answer")?;
    let other = field_by_name("other")?;
    let mut visitor = Messages::new(RecordingVisitor::default());

    visitor.record_i64(&answer, -42);
    visitor.record_u64(&answer, 42);
    visitor.record_f64(&other, 3.5);
    visitor.record_bool(&other, true);
    visitor.record_debug(&other, &"debugged");

    let expected = String::from("answer=i64:-42|answer=u64:42|other=f64:3.5|other=bool:true|other=debug");
    ensure_eq(visitor.finish(), expected, "Messages forwards non-string-message values")
      .map(drop)
      .map_err(TestError::from)
  }

  #[test]
  fn messages_make_visitor_wraps_inner_visitor() -> Result<(), TestError> {
    let message = field_by_name("message")?;
    let maker = Messages::new(MakeRecording);
    let mut visitor = maker.make_visitor(());

    visitor.record_str(&message, "hello");

    let expected = String::from("message=debug");
    ensure_eq(visitor.finish(), expected, "Messages wraps visitors produced by MakeVisitor")
      .map(drop)
      .map_err(TestError::from)
  }

  #[test]
  fn messages_finish_returns_inner_output() -> Result<(), TestError> {
    let other = field_by_name("other")?;
    let mut visitor = Messages::new(RecordingVisitor::default());

    visitor.record_debug(&other, &"ignored");

    let expected = String::from("other=debug");
    ensure_eq(visitor.finish(), expected, "Messages finish returns the inner visitor output")
      .map(drop)
      .map_err(TestError::from)
  }

  #[test]
  fn messages_visit_fmt_and_visit_write_forward_writers() -> Result<(), TestError> {
    let mut fmt_visitor = Messages::new(FmtVisitor::default());
    ensure(fmt_visitor.writer().write_str("fmt").is_ok(), "fmt writer should accept output").map(drop)?;
    ensure_eq((fmt_visitor.0.writer).clone(), String::from("fmt"), "Messages forwards fmt writers").map(drop)?;
    ensure(VisitOutput::<fmt::Result>::finish(fmt_visitor).is_ok(), "fmt visitor should finish").map(drop)?;

    let mut io_visitor = Messages::new(IoVisitor::default());
    ensure_ok(io_visitor.writer().write_all(b"io"), "io writer should accept output")?;
    ensure(io_visitor.0.writer == b"io", "Messages forwards io writers").map(drop)?;
    ensure_ok(VisitOutput::<Result<(), io::Error>>::finish(io_visitor), "io visitor should finish")?;
    Ok(())
  }
}
