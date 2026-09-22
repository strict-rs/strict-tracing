//! Serialization contract tests for `tracing-serde`.

#[cfg(test)]
mod tests {
  use serde_json::Value;
  use serde_json::json;

  /// Native failures from these behavioral checks.
  #[derive(Debug, thiserror::Error)]
  enum TestError {
    /// Retains the native field failure.
    #[error(transparent)]
    Field(#[from] strict_test_support::OptionFailure<Field>),
    /// Retains the native spanid failure.
    #[error(transparent)]
    SpanId(#[from] strict_test_support::OptionFailure<Id>),
    /// Retains the native serialize failure.
    #[error(transparent)]
    Serialize(#[from] strict_test_support::ResultFailure<serde_json::Error>),
    /// Retains the native jsonfield failure.
    #[error(transparent)]
    JsonField(#[from] strict_test_support::OptionFailure<Value>),
    /// Retains the native jsoncomparison failure.
    #[error(transparent)]
    JsonComparison(#[from] strict_test_support::ComparisonFailure<Value, Value>),
  }

  use strict_test_support::ensure_eq;
  use strict_test_support::ensure_ok;
  use strict_test_support::ensure_some;
  use tracing_core::callsite::Callsite;
  use tracing_core::callsite::Identifier;
  use tracing_core::field::Field;
  use tracing_core::field::FieldSet;
  use tracing_core::field::Value as FieldValue;
  use tracing_core::field::debug;
  use tracing_core::metadata::Kind;
  use tracing_core::metadata::Level;
  use tracing_core::metadata::Metadata;
  use tracing_core::metadata::SourceLocation;
  use tracing_core::span::Attributes;
  use tracing_core::span::Id;
  use tracing_core::span::Record;
  use tracing_core::subscriber::Interest;
  use tracing_serde::AsSerde as _;
  use tracing_serde::fields::AsMap as _;

  struct EventCallsite;
  struct SpanCallsite;
  struct EmptyCallsite;

  static EVENT_CALLSITE: EventCallsite = EventCallsite;
  static SPAN_CALLSITE: SpanCallsite = SpanCallsite;
  static EMPTY_CALLSITE: EmptyCallsite = EmptyCallsite;

  static EVENT_META: Metadata<'static> = tracing_core::metadata! {
      name: "serde_event",
      target: "serde_target",
      level: Level::INFO,
      fields: &["message", "answer", "flag", "count", "ratio", "debugged", "empty"],
      callsite: &EVENT_CALLSITE,
      kind: Kind::EVENT,
  };

  static SPAN_META: Metadata<'static> = tracing_core::metadata! {
      name: "serde_span",
      target: "serde_target",
      level: Level::DEBUG,
      fields: &["message", "answer"],
      callsite: &SPAN_CALLSITE,
      kind: Kind::SPAN,
  };

  static EMPTY_META: Metadata<'static> = Metadata::new(
    "empty_event",
    "empty_target",
    Level::TRACE,
    &SourceLocation::empty(),
    &FieldSet::new(&["message"], Identifier(&EMPTY_CALLSITE)),
    Kind::EVENT,
  );

  impl Callsite for EventCallsite {
    fn set_interest(&self, _: Interest) {}

    fn metadata(&self) -> &Metadata<'_> {
      &EVENT_META
    }
  }

  impl Callsite for SpanCallsite {
    fn set_interest(&self, _: Interest) {}

    fn metadata(&self) -> &Metadata<'_> {
      &SPAN_META
    }
  }

  impl Callsite for EmptyCallsite {
    fn set_interest(&self, _: Interest) {}

    fn metadata(&self) -> &Metadata<'_> {
      &EMPTY_META
    }
  }

  fn event_field(name: &'static str) -> Result<Field, TestError> {
    ensure_some(EVENT_META.fields().field(name), "event field exists").map_err(TestError::from)
  }

  fn span_field(name: &'static str) -> Result<Field, TestError> {
    ensure_some(SPAN_META.fields().field(name), "span field exists").map_err(TestError::from)
  }

  fn span_id(raw: u64) -> Result<Id, TestError> {
    ensure_some(Id::try_from_u64(raw), "span id is nonzero").map_err(TestError::from)
  }

  fn serialize_value(value: impl serde::Serialize) -> Result<Value, TestError> {
    ensure_ok(serde_json::to_value(value), "value should serialize").map_err(TestError::from)
  }

  fn object_field(value: &Value, name: &'static str) -> Result<Value, TestError> {
    ensure_some(value.get(name).cloned(), "JSON object field exists").map_err(TestError::from)
  }

  #[test]
  fn metadata_serializes_identity_location_fields_and_kind_flags() -> Result<(), TestError> {
    let serialized = serialize_value(EVENT_META.as_serde())?;
    let expected = json!({
      "name": EVENT_META.name(),
      "target": EVENT_META.target(),
      "level": EVENT_META.level().as_str(),
      "module_path": EVENT_META.module_path(),
      "file": EVENT_META.file(),
      "line": EVENT_META.line(),
      "fields": ["message", "answer", "flag", "count", "ratio", "debugged", "empty"],
      "is_span": false,
      "is_event": true,
    });

    ensure_eq(serialized, expected, "metadata JSON shape")
      .map(drop)
      .map_err(TestError::from)
  }

  #[test]
  fn metadata_optional_location_fields_serialize_as_null() -> Result<(), TestError> {
    let serialized = serialize_value(EMPTY_META.as_serde())?;
    let expected = json!({
      "name": "empty_event",
      "target": "empty_target",
      "level": "TRACE",
      "module_path": null,
      "file": null,
      "line": null,
      "fields": ["message"],
      "is_span": false,
      "is_event": true,
    });

    ensure_eq(serialized, expected, "empty metadata location fields are null")
      .map(drop)
      .map_err(TestError::from)
  }

  #[test]
  fn field_sets_levels_and_span_ids_serialize_as_public_values() -> Result<(), TestError> {
    let fields = serialize_value(EVENT_META.fields().as_serde())?;
    let level = serialize_value(Level::WARN.as_serde())?;
    let id = span_id(7)?;
    let serialized_id = serialize_value(id.as_serde())?;
    let message = event_field("message")?;
    let serialized_field = serialize_value(message.as_serde())?;

    ensure_eq(
      fields,
      json!(["message", "answer", "flag", "count", "ratio", "debugged", "empty"]),
      "field set preserves declaration order",
    )
    .map(drop)?;
    ensure_eq(serialized_field, json!("message"), "individual field serializes as its name").map(drop)?;
    ensure_eq(level, json!("WARN"), "level serializes as its string form").map(drop)?;
    ensure_eq(serialized_id, json!([7]), "span id serializes as one-field tuple")
      .map(drop)
      .map_err(TestError::from)
  }

  #[test]
  fn events_serialize_metadata_and_recorded_fields() -> Result<(), TestError> {
    let message = event_field("message")?;
    let answer = event_field("answer")?;
    let flag = event_field("flag")?;
    let count = event_field("count")?;
    let ratio = event_field("ratio")?;
    let debugged = event_field("debugged")?;
    let debugged_value = debug("event debug");
    let message_value: &dyn FieldValue = &"hello";
    let answer_value: &dyn FieldValue = &42_i64;
    let flag_value: &dyn FieldValue = &true;
    let count_value: &dyn FieldValue = &42_u64;
    let ratio_value: &dyn FieldValue = &2.5_f64;
    let debug_value: &dyn FieldValue = &debugged_value;
    let values = [
      (&message, Some(message_value)),
      (&answer, Some(answer_value)),
      (&flag, Some(flag_value)),
      (&count, Some(count_value)),
      (&ratio, Some(ratio_value)),
      (&debugged, Some(debug_value)),
    ];
    let valueset = EVENT_META.fields().value_set(&values);
    let event = tracing_core::Event::new(&EVENT_META, &valueset);

    let serialized = serialize_value(event.as_serde())?;

    ensure_eq(
      object_field(&serialized, "metadata")?,
      serialize_value(EVENT_META.as_serde())?,
      "event metadata",
    )
    .map(drop)?;
    let fields = object_field(&serialized, "fields")?;
    ensure_eq(object_field(&fields, "message")?, json!("hello"), "event message field").map(drop)?;
    ensure_eq(object_field(&fields, "answer")?, json!(42), "event i64 field").map(drop)?;
    ensure_eq(object_field(&fields, "flag")?, json!(true), "event bool field").map(drop)?;
    ensure_eq(object_field(&fields, "count")?, json!(42), "event u64 field").map(drop)?;
    ensure_eq(object_field(&fields, "ratio")?, json!(2.5), "event f64 field").map(drop)?;
    ensure_eq(object_field(&fields, "debugged")?, json!("\"event debug\""), "event debug field")
      .map(drop)
      .map_err(TestError::from)
  }

  #[test]
  fn span_attributes_serialize_current_root_and_explicit_parent_variants() -> Result<(), TestError> {
    let message = span_field("message")?;
    let answer = span_field("answer")?;
    let message_value: &dyn FieldValue = &"span hello";
    let answer_value: &dyn FieldValue = &42_i64;
    let values = [(&message, Some(message_value)), (&answer, Some(answer_value))];
    let valueset = SPAN_META.fields().value_set(&values);
    let parent = span_id(9)?;

    let current = serialize_value(Attributes::new(&SPAN_META, &valueset).as_serde())?;
    let root = serialize_value(Attributes::new_root(&SPAN_META, &valueset).as_serde())?;
    let child = serialize_value(Attributes::child_of(parent, &SPAN_META, &valueset).as_serde())?;
    let field_map = serialize_value(Attributes::new(&SPAN_META, &valueset).field_map())?;

    ensure_eq(
      object_field(&current, "parent")?,
      json!("Current"),
      "contextual attributes preserve current parenting",
    )
    .map(drop)?;
    ensure_eq(
      object_field(&root, "parent")?,
      json!("Root"),
      "root attributes preserve explicit root parenting",
    )
    .map(drop)?;
    ensure_eq(
      object_field(&child, "parent")?,
      json!({"Explicit": [9]}),
      "child attributes preserve explicit parent id",
    )
    .map(drop)?;
    for attributes in [&current, &root, &child] {
      ensure_eq(
        object_field(attributes, "metadata")?,
        serialize_value(SPAN_META.as_serde())?,
        "attributes preserve metadata",
      )
      .map(drop)?;
      ensure_eq(
        object_field(attributes, "fields")?,
        field_map.clone(),
        "attributes nest recorded fields",
      )
      .map(drop)?;
    }
    ensure_eq(
      field_map,
      json!({"message": "span hello", "answer": 42}),
      "attributes field map serializes only recorded fields",
    )
    .map(drop)
    .map_err(TestError::from)
  }

  #[test]
  fn records_serialize_present_fields_and_omit_absent_fields() -> Result<(), TestError> {
    let message = event_field("message")?;
    let answer = event_field("answer")?;
    let empty = event_field("empty")?;
    let message_value: &dyn FieldValue = &"recorded";
    let answer_value: &dyn FieldValue = &42_u64;
    let values = [(&message, Some(message_value)), (&answer, Some(answer_value)), (&empty, None)];
    let valueset = EVENT_META.fields().value_set(&values);
    let record = Record::new(&valueset);

    let serialized = serialize_value(record.as_serde())?;
    let field_map = serialize_value(record.field_map())?;

    ensure_eq((serialized).clone(), json!({"message": "recorded", "answer": 42}), "record fields").map(drop)?;
    ensure_eq(field_map, serialized, "record field map matches record serialization")
      .map(drop)
      .map_err(TestError::from)
  }

  #[test]
  fn primitive_field_values_serialize_to_json_values() -> Result<(), TestError> {
    let flag = event_field("flag")?;
    let answer = event_field("answer")?;
    let count = event_field("count")?;
    let ratio = event_field("ratio")?;
    let message = event_field("message")?;
    let debugged = event_field("debugged")?;
    let empty = event_field("empty")?;
    let debugged_value = debug("debug text");
    let flag_value: &dyn FieldValue = &false;
    let answer_value: &dyn FieldValue = &-5_i64;
    let count_value: &dyn FieldValue = &5_u64;
    let ratio_value: &dyn FieldValue = &1.5_f64;
    let message_value: &dyn FieldValue = &"plain";
    let debug_value: &dyn FieldValue = &debugged_value;
    let values = [
      (&flag, Some(flag_value)),
      (&answer, Some(answer_value)),
      (&count, Some(count_value)),
      (&ratio, Some(ratio_value)),
      (&message, Some(message_value)),
      (&debugged, Some(debug_value)),
      (&empty, None),
    ];
    let valueset = EVENT_META.fields().value_set(&values);
    let record = Record::new(&valueset);

    let serialized = serialize_value(record.as_serde())?;
    let expected = json!({
      "flag": false,
      "answer": -5,
      "count": 5,
      "ratio": 1.5,
      "message": "plain",
      "debugged": "\"debug text\"",
    });

    ensure_eq(serialized, expected, "primitive field JSON values")
      .map(drop)
      .map_err(TestError::from)
  }
}
