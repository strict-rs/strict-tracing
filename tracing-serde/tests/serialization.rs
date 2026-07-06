//! Serialization contract tests for `tracing-serde`.

#[cfg(test)]
mod tests {
  use serde_json::Value;
  use serde_json::json;
  use strict_test_support::TestFailure;
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

  fn event_field(name: &'static str) -> Result<Field, TestFailure> {
    ensure_some(EVENT_META.fields().field(name), "event field exists")
  }

  fn span_field(name: &'static str) -> Result<Field, TestFailure> {
    ensure_some(SPAN_META.fields().field(name), "span field exists")
  }

  fn span_id(raw: u64) -> Result<Id, TestFailure> {
    ensure_some(Id::try_from_u64(raw), "span id is nonzero")
  }

  fn serialize_value(value: impl serde::Serialize) -> Result<Value, TestFailure> {
    ensure_ok(serde_json::to_value(value), "value should serialize")
  }

  fn object_field<'a>(value: &'a Value, name: &'static str) -> Result<&'a Value, TestFailure> {
    ensure_some(value.get(name), "JSON object field exists")
  }

  #[test]
  fn metadata_serializes_identity_location_fields_and_kind_flags() -> Result<(), TestFailure> {
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

    ensure_eq(&serialized, &expected, "metadata JSON shape")
  }

  #[test]
  fn metadata_optional_location_fields_serialize_as_null() -> Result<(), TestFailure> {
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

    ensure_eq(&serialized, &expected, "empty metadata location fields are null")
  }

  #[test]
  fn field_sets_levels_and_span_ids_serialize_as_public_values() -> Result<(), TestFailure> {
    let fields = serialize_value(EVENT_META.fields().as_serde())?;
    let level = serialize_value(Level::WARN.as_serde())?;
    let id = span_id(7)?;
    let serialized_id = serialize_value(id.as_serde())?;
    let message = event_field("message")?;
    let serialized_field = serialize_value(message.as_serde())?;

    ensure_eq(
      &fields,
      &json!(["message", "answer", "flag", "count", "ratio", "debugged", "empty"]),
      "field set preserves declaration order",
    )?;
    ensure_eq(&serialized_field, &json!("message"), "individual field serializes as its name")?;
    ensure_eq(&level, &json!("WARN"), "level serializes as its string form")?;
    ensure_eq(&serialized_id, &json!([7]), "span id serializes as one-field tuple")
  }

  #[test]
  fn events_serialize_metadata_and_recorded_fields() -> Result<(), TestFailure> {
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
      &serialize_value(EVENT_META.as_serde())?,
      "event metadata",
    )?;
    ensure_eq(object_field(&serialized, "message")?, &json!("hello"), "event message field")?;
    ensure_eq(object_field(&serialized, "answer")?, &json!(42), "event i64 field")?;
    ensure_eq(object_field(&serialized, "flag")?, &json!(true), "event bool field")?;
    ensure_eq(object_field(&serialized, "count")?, &json!(42), "event u64 field")?;
    ensure_eq(object_field(&serialized, "ratio")?, &json!(2.5), "event f64 field")?;
    ensure_eq(
      object_field(&serialized, "debugged")?,
      &json!("\"event debug\""),
      "event debug field",
    )
  }

  #[test]
  fn span_attributes_serialize_current_root_and_explicit_parent_variants() -> Result<(), TestFailure> {
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
      &Value::Null,
      "current-parent attributes have no explicit parent",
    )?;
    ensure_eq(
      object_field(&current, "is_root")?,
      &json!(false),
      "current-parent attributes are not root",
    )?;
    ensure_eq(
      object_field(&root, "parent")?,
      &Value::Null,
      "root attributes have no explicit parent",
    )?;
    ensure_eq(object_field(&root, "is_root")?, &json!(true), "root attributes set the root flag")?;
    ensure_eq(
      object_field(&child, "parent")?,
      &json!([9]),
      "explicit child attributes serialize parent id",
    )?;
    ensure_eq(
      object_field(&child, "is_root")?,
      &json!(false),
      "explicit child attributes are not root",
    )?;
    ensure_eq(
      &field_map,
      &json!({"message": "span hello", "answer": 42}),
      "attributes field map serializes only recorded fields",
    )
  }

  #[test]
  fn records_serialize_present_fields_and_omit_absent_fields() -> Result<(), TestFailure> {
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

    ensure_eq(&serialized, &json!({"message": "recorded", "answer": 42}), "record fields")?;
    ensure_eq(&field_map, &serialized, "record field map matches record serialization")
  }

  #[test]
  fn primitive_field_values_serialize_to_json_values() -> Result<(), TestFailure> {
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

    ensure_eq(&serialized, &expected, "primitive field JSON values")
  }
}
