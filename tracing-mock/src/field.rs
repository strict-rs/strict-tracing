//! Define expectations to validate fields on events and spans.
//!
//! The [`ExpectedField`] struct define expected values for fields in
//! order to match events and spans via the mock subscriber API in the
//! [`subscriber`] module.
//!
//! Expected fields should be created with [`expect::field`] and a
//! chain of method calls to specify the field value and additional
//! fields as necessary.
//!
//! # Examples
//!
//! The simplest case is to expect that an event has a field with a
//! specific name, without any expectation about the value:
//!
//! ```
//! # fn main() -> Result<(), strict_test_support::TestFailure> {
//! use tracing_mock::expect;
//! use tracing_mock::subscriber;
//!
//! let event = expect::event().with_fields(expect::field("field_name"));
//!
//! let (subscriber, handle) = subscriber::mock().event(event).run_with_handle();
//!
//! tracing::subscriber::with_default(subscriber, || {
//!   tracing::info!(field_name = "value");
//! });
//!
//! strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
//! # Ok(())
//! # }
//! ```
//!
//! It is possible to expect multiple fields and specify the value for
//! each of them:
//!
//! ```
//! # fn main() -> Result<(), strict_test_support::TestFailure> {
//! use tracing_mock::expect;
//! use tracing_mock::subscriber;
//!
//! let event = expect::event().with_fields(
//!   expect::field("string_field")
//!     .with_value(&"field_value")
//!     .and(expect::field("integer_field").with_value(&54_i64))
//!     .and(expect::field("bool_field").with_value(&true)),
//! );
//!
//! let (subscriber, handle) = subscriber::mock().event(event).run_with_handle();
//!
//! tracing::subscriber::with_default(subscriber, || {
//!   tracing::info!(
//!     string_field = "field_value",
//!     integer_field = 54_i64,
//!     bool_field = true,
//!   );
//! });
//!
//! strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
//! # Ok(())
//! # }
//! ```
//!
//! If an expected field is not present, or if the value of the field
//! is different, the test will fail. In this example, the value is
//! different:
//!
//! ```
//! # fn main() -> Result<(), strict_test_support::TestFailure> {
//! use tracing_mock::expect;
//! use tracing_mock::subscriber;
//!
//! let event = expect::event().with_fields(expect::field("field_name").with_value(&"value"));
//!
//! let (subscriber, handle) = subscriber::mock().event(event).run_with_handle();
//!
//! tracing::subscriber::with_default(subscriber, || {
//!   tracing::info!(field_name = "different value");
//! });
//!
//! strict_test_support::ensure(
//!   handle.finished().is_err(),
//!   "mock expectation mismatch returns an error",
//! )?;
//! # Ok(())
//! # }
//! ```
//!
//! [`subscriber`]: mod@crate::subscriber
//! [`expect::field`]: fn@crate::expect::field
use std::collections::HashMap;
use std::fmt;

use tracing::callsite;
use tracing::callsite::Callsite as _;
use tracing::field::Field;
use tracing::field::Value;
use tracing::field::Visit;
use tracing::field::{
  self,
};
use tracing::metadata::Kind;

use crate::failure::ExpectationError;
use crate::failure::ExpectationResult;

/// An expectation for multiple fields.
///
/// For a detailed description and examples, see the documentation for
/// the methods and the [`field`] module.
///
/// [`field`]: mod@crate::field
#[derive(Default, Debug, Eq, PartialEq)]
pub struct ExpectedFields {
  /// Expected field names and values.
  fields: HashMap<String, ExpectedValue>,
  /// Whether unexpected extra fields should fail the expectation.
  only:   bool,
}

/// An expected field.
///
/// For a detailed description and examples, see the documentation for
/// the methods and the [`field`] module.
///
/// [`field`]: mod@crate::field
#[derive(Debug)]
pub struct ExpectedField {
  /// Expected field name.
  pub(super) name:  String,
  /// Expected field value.
  pub(super) value: ExpectedValue,
}

/// Expected value for a field.
#[derive(Debug)]
pub(crate) enum ExpectedValue {
  /// Expected floating-point value.
  F64(f64),
  /// Expected signed integer value.
  I64(i64),
  /// Expected unsigned integer value.
  U64(u64),
  /// Expected boolean value.
  Bool(bool),
  /// Expected string value.
  Str(String),
  /// Expected debug-rendered value.
  Debug(String),
  /// Any value is accepted.
  Any,
  /// Value conversion failed while constructing an expected value.
  Invalid(String),
}

impl Eq for ExpectedValue {}

impl PartialEq for ExpectedValue {
  fn eq(&self, other: &Self) -> bool {
    let values_match = match *self {
      Self::F64(left) => matches!(*other, Self::F64(right) if left.eq(&right)),
      Self::I64(left) => matches!(*other, Self::I64(right) if left.eq(&right)),
      Self::U64(left) => matches!(*other, Self::U64(right) if left.eq(&right)),
      Self::Bool(left) => matches!(*other, Self::Bool(right) if left.eq(&right)),
      Self::Str(ref left) => {
        matches!(*other, Self::Str(ref right) if left.eq(right))
      }
      Self::Debug(ref left) => {
        matches!(*other, Self::Debug(ref right) if left.eq(right))
      }
      Self::Any => true,
      Self::Invalid(_) => false,
    };
    values_match || matches!(*other, Self::Any)
  }
}

impl ExpectedField {
  /// Sets the value to expect when matching this field.
  ///
  /// If the recorded value for this field is different, the
  /// expectation will fail.
  ///
  /// # Examples
  ///
  /// ```
  /// # fn main() -> Result<(), strict_test_support::TestFailure> {
  /// use tracing_mock::expect;
  /// use tracing_mock::subscriber;
  ///
  /// let event = expect::event().with_fields(expect::field("field_name").with_value(&"value"));
  ///
  /// let (subscriber, handle) = subscriber::mock().event(event).run_with_handle();
  ///
  /// tracing::subscriber::with_default(subscriber, || {
  ///   tracing::info!(field_name = "value");
  /// });
  ///
  /// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
  /// # Ok(())
  /// # }
  /// ```
  ///
  /// A different value will cause the test to fail:
  ///
  /// ```
  /// # fn main() -> Result<(), strict_test_support::TestFailure> {
  /// use tracing_mock::expect;
  /// use tracing_mock::subscriber;
  ///
  /// let event = expect::event().with_fields(expect::field("field_name").with_value(&"value"));
  ///
  /// let (subscriber, handle) = subscriber::mock().event(event).run_with_handle();
  ///
  /// tracing::subscriber::with_default(subscriber, || {
  ///   tracing::info!(field_name = "different value");
  /// });
  ///
  /// strict_test_support::ensure(
  ///   handle.finished().is_err(),
  ///   "mock expectation mismatch returns an error",
  /// )?;
  /// # Ok(())
  /// # }
  /// ```
  #[must_use]
  pub fn with_value(self, value: &dyn Value) -> Self {
    Self {
      value: ExpectedValue::try_from_value(value).unwrap_or_else(|error| ExpectedValue::Invalid(error.to_string())),
      ..self
    }
  }

  /// Adds an additional [`ExpectedField`] to be matched.
  ///
  /// Any fields introduced by `.and` must also match. If any fields
  /// are not present, or if the value for any field is different,
  /// then the expectation will fail.
  ///
  /// # Examples
  ///
  /// ```
  /// # fn main() -> Result<(), strict_test_support::TestFailure> {
  /// use tracing_mock::expect;
  /// use tracing_mock::subscriber;
  ///
  /// let event = expect::event().with_fields(
  ///   expect::field("field")
  ///     .with_value(&"value")
  ///     .and(expect::field("another_field").with_value(&42)),
  /// );
  ///
  /// let (subscriber, handle) = subscriber::mock().event(event).run_with_handle();
  ///
  /// tracing::subscriber::with_default(subscriber, || {
  ///   tracing::info!(field = "value", another_field = 42,);
  /// });
  ///
  /// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
  /// # Ok(())
  /// # }
  /// ```
  ///
  /// If the second field is not present, the test will fail:
  ///
  /// ```
  /// # fn main() -> Result<(), strict_test_support::TestFailure> {
  /// use tracing_mock::expect;
  /// use tracing_mock::subscriber;
  ///
  /// let event = expect::event().with_fields(
  ///   expect::field("field")
  ///     .with_value(&"value")
  ///     .and(expect::field("another_field").with_value(&42)),
  /// );
  ///
  /// let (subscriber, handle) = subscriber::mock().event(event).run_with_handle();
  ///
  /// tracing::subscriber::with_default(subscriber, || {
  ///   tracing::info!(field = "value");
  /// });
  ///
  /// strict_test_support::ensure(
  ///   handle.finished().is_err(),
  ///   "mock expectation mismatch returns an error",
  /// )?;
  /// # Ok(())
  /// # }
  /// ```
  #[must_use]
  pub fn and(self, other: Self) -> ExpectedFields {
    ExpectedFields {
      fields: HashMap::new(),
      only:   false,
    }
    .and(self)
    .and(other)
  }

  /// Indicates that no fields other than those specified should be
  /// expected.
  ///
  /// If additional fields are present on the recorded event or span,
  /// the expectation will fail.
  ///
  /// # Examples
  ///
  /// The following test passes despite the recorded event having
  /// fields that were not expected because `only` was not
  /// used:
  ///
  /// ```
  /// # fn main() -> Result<(), strict_test_support::TestFailure> {
  /// use tracing_mock::expect;
  /// use tracing_mock::subscriber;
  ///
  /// let event = expect::event().with_fields(expect::field("field").with_value(&"value"));
  ///
  /// let (subscriber, handle) = subscriber::mock().event(event).run_with_handle();
  ///
  /// tracing::subscriber::with_default(subscriber, || {
  ///   tracing::info!(field = "value", another_field = 42,);
  /// });
  ///
  /// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
  /// # Ok(())
  /// # }
  /// ```
  ///
  /// If we include `only` on the `ExpectedField` then the test
  /// will fail:
  ///
  /// ```
  /// # fn main() -> Result<(), strict_test_support::TestFailure> {
  /// use tracing_mock::expect;
  /// use tracing_mock::subscriber;
  ///
  /// let event = expect::event().with_fields(expect::field("field").with_value(&"value").only());
  ///
  /// let (subscriber, handle) = subscriber::mock().event(event).run_with_handle();
  ///
  /// tracing::subscriber::with_default(subscriber, || {
  ///   tracing::info!(field = "value", another_field = 42,);
  /// });
  ///
  /// strict_test_support::ensure(
  ///   handle.finished().is_err(),
  ///   "mock expectation mismatch returns an error",
  /// )?;
  /// # Ok(())
  /// # }
  /// ```
  #[must_use]
  pub fn only(self) -> ExpectedFields {
    ExpectedFields {
      fields: HashMap::new(),
      only:   true,
    }
    .and(self)
  }
}

impl From<ExpectedField> for ExpectedFields {
  fn from(field: ExpectedField) -> Self {
    Self {
      fields: HashMap::new(),
      only:   false,
    }
    .and(field)
  }
}

impl ExpectedFields {
  /// Adds an additional [`ExpectedField`] to be matched.
  ///
  /// All fields must match, if any of them are not present, or if
  /// the value for any field is different, the expectation will
  /// fail.
  ///
  /// This method performs the same function as
  /// [`ExpectedField::and`], but applies in the case where there are
  /// already multiple fields expected.
  ///
  /// # Examples
  ///
  /// ```
  /// # fn main() -> Result<(), strict_test_support::TestFailure> {
  /// use tracing_mock::expect;
  /// use tracing_mock::subscriber;
  ///
  /// let event = expect::event().with_fields(
  ///   expect::field("field")
  ///     .with_value(&"value")
  ///     .and(expect::field("another_field").with_value(&42))
  ///     .and(expect::field("a_third_field").with_value(&true)),
  /// );
  ///
  /// let (subscriber, handle) = subscriber::mock().event(event).run_with_handle();
  ///
  /// tracing::subscriber::with_default(subscriber, || {
  ///   tracing::info!(field = "value", another_field = 42, a_third_field = true,);
  /// });
  ///
  /// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
  /// # Ok(())
  /// # }
  /// ```
  ///
  /// If any of the expected fields are not present on the recorded
  /// event, the test will fail:
  ///
  /// ```
  /// # fn main() -> Result<(), strict_test_support::TestFailure> {
  /// use tracing_mock::expect;
  /// use tracing_mock::subscriber;
  ///
  /// let event = expect::event().with_fields(
  ///   expect::field("field")
  ///     .with_value(&"value")
  ///     .and(expect::field("another_field").with_value(&42))
  ///     .and(expect::field("a_third_field").with_value(&true)),
  /// );
  ///
  /// let (subscriber, handle) = subscriber::mock().event(event).run_with_handle();
  ///
  /// tracing::subscriber::with_default(subscriber, || {
  ///   tracing::info!(field = "value", a_third_field = true,);
  /// });
  ///
  /// strict_test_support::ensure(
  ///   handle.finished().is_err(),
  ///   "mock expectation mismatch returns an error",
  /// )?;
  /// # Ok(())
  /// # }
  /// ```
  ///
  /// [`ExpectedField::and`]: fn@crate::field::ExpectedField::and
  #[must_use]
  pub fn and(mut self, field: ExpectedField) -> Self {
    let _previous = self.fields.insert(field.name, field.value);
    self
  }

  /// Indicates that no fields other than those specified should be
  /// expected.
  ///
  /// This method performs the same function as
  /// [`ExpectedField::only`], but applies in the case where there are
  /// multiple fields expected.
  ///
  /// # Examples
  ///
  /// The following test will pass, even though additional fields are
  /// recorded on the event.
  ///
  /// ```
  /// # fn main() -> Result<(), strict_test_support::TestFailure> {
  /// use tracing_mock::expect;
  /// use tracing_mock::subscriber;
  ///
  /// let event = expect::event().with_fields(
  ///   expect::field("field")
  ///     .with_value(&"value")
  ///     .and(expect::field("another_field").with_value(&42)),
  /// );
  ///
  /// let (subscriber, handle) = subscriber::mock().event(event).run_with_handle();
  ///
  /// tracing::subscriber::with_default(subscriber, || {
  ///   tracing::info!(field = "value", another_field = 42, a_third_field = true,);
  /// });
  ///
  /// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
  /// # Ok(())
  /// # }
  /// ```
  ///
  /// If we include `only` on the `ExpectedFields` then the test
  /// will fail:
  ///
  /// ```
  /// # fn main() -> Result<(), strict_test_support::TestFailure> {
  /// use tracing_mock::expect;
  /// use tracing_mock::subscriber;
  ///
  /// let event = expect::event().with_fields(
  ///   expect::field("field")
  ///     .with_value(&"value")
  ///     .and(expect::field("another_field").with_value(&42))
  ///     .only(),
  /// );
  ///
  /// let (subscriber, handle) = subscriber::mock().event(event).run_with_handle();
  ///
  /// tracing::subscriber::with_default(subscriber, || {
  ///   tracing::info!(field = "value", another_field = 42, a_third_field = true,);
  /// });
  ///
  /// strict_test_support::ensure(
  ///   handle.finished().is_err(),
  ///   "mock expectation mismatch returns an error",
  /// )?;
  /// # Ok(())
  /// # }
  /// ```
  #[must_use]
  pub fn only(self) -> Self {
    Self {
      only: true,
      ..self
    }
  }

  /// Compares an observed field value against the matching expectation.
  fn compare(&mut self, name: &str, value: &dyn Value, ctx: &str, subscriber_name: &str) -> ExpectationResult {
    let actual_value = ExpectedValue::try_from_value(value)?;
    match self.fields.remove(name) {
      Some(ExpectedValue::Any) => {}
      Some(expected) => {
        if expected != actual_value {
          return Err(ExpectationError::from_args(format_args!(
            "\n[{subscriber_name}] expected `{ctx}` to contain:\n\t`{name}{expected}`\nbut got:\n\t`{name}{actual_value}`"
          )));
        }
      }
      None if self.only => {
        return Err(ExpectationError::from_args(format_args!(
          "[{subscriber_name}]expected `{ctx}` to contain only:\n\t`{self}`\nbut got:\n\t`{name}{actual_value}`"
        )));
      }
      _ => {}
    }
    Ok(())
  }

  /// Creates a visitor that checks observed fields against these expectations.
  pub(crate) const fn checker<'a>(&'a mut self, ctx: &'a str, subscriber_name: &'a str) -> CheckVisitor<'a> {
    CheckVisitor {
      expect: self,
      ctx,
      subscriber_name,
      error: None,
    }
  }

  /// Returns whether there are no pending field expectations.
  pub(crate) fn is_empty(&self) -> bool {
    self.fields.is_empty()
  }
}

impl fmt::Display for ExpectedValue {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match *self {
      Self::F64(value) => write!(f, "f64 = {value}"),
      Self::I64(value) => write!(f, "i64 = {value}"),
      Self::U64(value) => write!(f, "u64 = {value}"),
      Self::Bool(value) => write!(f, "bool = {value}"),
      Self::Str(ref value) => write!(f, "&str = \"{value}\""),
      Self::Debug(ref value) => write!(f, "&fmt::Debug = \"{value}\""),
      Self::Any => write!(f, "_ = _"),
      Self::Invalid(ref error) => write!(f, "<invalid expected value: {error}>"),
    }
  }
}

/// Visitor that records observed fields into an expectation check.
pub(crate) struct CheckVisitor<'a> {
  /// Expected fields still waiting to be matched.
  expect:          &'a mut ExpectedFields,
  /// Context rendered into failure messages.
  ctx:             &'a str,
  /// Subscriber or layer name rendered into failure messages.
  subscriber_name: &'a str,
  /// First field validation error observed by the visitor.
  error:           Option<ExpectationError>,
}

impl Visit for CheckVisitor<'_> {
  fn record_f64(&mut self, field: &Field, value: f64) {
    self.compare_field(field.name(), &value);
  }

  fn record_i64(&mut self, field: &Field, value: i64) {
    self.compare_field(field.name(), &value);
  }

  fn record_u64(&mut self, field: &Field, value: u64) {
    self.compare_field(field.name(), &value);
  }

  fn record_bool(&mut self, field: &Field, value: bool) {
    self.compare_field(field.name(), &value);
  }

  fn record_str(&mut self, field: &Field, value: &str) {
    self.compare_field(field.name(), &value);
  }

  fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
    self.compare_field(field.name(), &field::debug(value));
  }
}

/// Display adapter for values recorded through `Visit::record_debug`.
struct RenderDebug<'a>(&'a dyn fmt::Debug);

impl fmt::Display for RenderDebug<'_> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    fmt::Debug::fmt(self.0, f)
  }
}

impl CheckVisitor<'_> {
  /// Compares a visited field unless an earlier field mismatch was recorded.
  fn compare_field(&mut self, name: &str, value: &dyn Value) {
    if self.error.is_some() {
      return;
    }
    let result = self.expect.compare(name, value, self.ctx, self.subscriber_name);
    self.record_result(result);
  }

  /// Preserves the first validation failure reported while visiting fields.
  fn record_result(&mut self, result: ExpectationResult) {
    if self.error.is_none()
      && let Err(error) = result
    {
      self.error = Some(error);
    }
  }

  /// Finishes field validation and fails if expected fields were not seen.
  pub(crate) fn finish(self) -> ExpectationResult {
    if let Some(error) = self.error {
      return Err(error);
    }
    if !self.expect.fields.is_empty() {
      return Err(ExpectationError::from_args(format_args!(
        "[{}] {}missing {}",
        self.subscriber_name, self.expect, self.ctx
      )));
    }
    Ok(())
  }
}

impl ExpectedValue {
  /// Converts a tracing field value into a comparable expectation value.
  fn try_from_value(value: &dyn Value) -> ExpectationResult<Self> {
    struct MockValueBuilder {
      value: Option<ExpectedValue>,
    }

    impl Visit for MockValueBuilder {
      fn record_f64(&mut self, _: &Field, value: f64) {
        self.value = Some(ExpectedValue::F64(value));
      }

      fn record_i64(&mut self, _: &Field, value: i64) {
        self.value = Some(ExpectedValue::I64(value));
      }

      fn record_u64(&mut self, _: &Field, value: u64) {
        self.value = Some(ExpectedValue::U64(value));
      }

      fn record_bool(&mut self, _: &Field, value: bool) {
        self.value = Some(ExpectedValue::Bool(value));
      }

      fn record_str(&mut self, _: &Field, value: &str) {
        self.value = Some(ExpectedValue::Str(value.to_owned()));
      }

      fn record_debug(&mut self, _: &Field, value: &dyn fmt::Debug) {
        self.value = Some(ExpectedValue::Debug(RenderDebug(value).to_string()));
      }
    }

    let internal_field = callsite!(name: "fake", kind: Kind::EVENT, fields: fake_field)
      .metadata()
      .fields()
      .field("fake_field");
    let Some(fake_field) = internal_field else {
      return Err(ExpectationError::from_args(format_args!(
        "tracing-mock could not construct the internal fake field"
      )));
    };
    let mut builder = MockValueBuilder {
      value: None
    };
    value.record(&fake_field, &mut builder);
    builder
      .value
      .ok_or_else(|| ExpectationError::from_args(format_args!("tracing-mock value conversion finished before a value was recorded")))
  }
}

impl fmt::Display for ExpectedFields {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "fields ")?;
    let entries = self
      .fields
      .iter()
      .map(|(name, value)| (field::display(name), field::display(value)));
    f.debug_map().entries(entries).finish()
  }
}
