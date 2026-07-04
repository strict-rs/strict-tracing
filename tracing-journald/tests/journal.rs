#![cfg(target_os = "linux")]
//! Integration tests for native journald output.

#[cfg(test)]
mod tests {
  use std::collections::HashMap;
  use std::process;
  use std::process::Command;
  use std::thread;
  use std::time::Duration;

  use serde::Deserialize;
  use strict_test_support::TestFailure;
  use strict_test_support::ensure;
  use strict_test_support::ensure_eq;
  use strict_test_support::ensure_ok;
  use strict_test_support::ensure_some;
  use tracing::debug;
  use tracing::error;
  use tracing::info;
  use tracing::info_span;
  use tracing::subscriber::with_default;
  use tracing::trace;
  use tracing::warn;
  use tracing_journald::JournalNamespace;
  use tracing_journald::Layer;
  use tracing_journald::Priority;
  use tracing_journald::PriorityMappings;
  use tracing_subscriber::Registry;
  use tracing_subscriber::layer::SubscriberExt as _;

  fn with_journald(f: impl FnOnce() -> Result<(), TestFailure>) -> Result<(), TestFailure> {
    with_journald_layer(
      JournalNamespace::System,
      ensure_ok(Layer::new(), "system journald layer opens")?
        .with_field_prefix(None)
        .with_priority_mappings(PriorityMappings {
          trace: Priority::Informational,
          ..PriorityMappings::new()
        }),
      f,
    )
  }

  #[allow(
    clippy::single_call_fn,
    reason = "user-journal setup keeps optional socket skip policy parallel to system journald tests"
  )]
  fn with_user_journald(f: impl FnOnce() -> Result<(), TestFailure>) -> Result<(), TestFailure> {
    let Ok(layer) = Layer::new_user() else {
      return Ok(());
    };

    with_journald_layer(
      JournalNamespace::User,
      layer.with_field_prefix(None).with_priority_mappings(PriorityMappings {
        trace: Priority::Informational,
        ..PriorityMappings::new()
      }),
      f,
    )
  }

  fn with_journald_layer(
    _namespace: JournalNamespace,
    layer: Layer,
    f: impl FnOnce() -> Result<(), TestFailure>,
  ) -> Result<(), TestFailure> {
    if Command::new("journalctl").arg("--version").output().is_err() {
      return Ok(());
    }

    let sub = Registry::default().with(layer);
    with_default(sub, f)
  }

  #[derive(Debug, PartialEq, Deserialize)]
  #[serde(untagged)]
  enum Field {
    Text(String),
    Array(Vec<String>),
    Binary(Vec<u8>),
  }

  impl Field {
    fn as_array(&self) -> Option<&[String]> {
      match *self {
        Self::Text(_) | Self::Binary(_) => None,
        Self::Array(ref values) => Some(values),
      }
    }

    fn as_text(&self) -> Option<&str> {
      match *self {
        Self::Text(ref value) => Some(value.as_str()),
        Self::Binary(_) | Self::Array(_) => None,
      }
    }

    fn bytes_eq(&self, expected: &[u8]) -> bool {
      match *self {
        Self::Text(ref value) => value.as_bytes() == expected,
        Self::Binary(ref value) => value == expected,
        Self::Array(_) => false,
      }
    }
  }

  /// Retry `f` 30 times 100ms apart, i.e. a total of three seconds.
  #[allow(
    clippy::single_call_fn,
    reason = "journald integration tests need a named polling boundary for eventual journal visibility"
  )]
  fn retry<T>(f: impl Fn() -> Option<T>) -> Result<T, TestFailure> {
    let attempts = 30;
    let interval = Duration::from_millis(100);
    for _attempt in 0..attempts {
      if let Some(result) = f() {
        return Ok(result);
      }
      thread::sleep(interval);
    }

    Err(TestFailure::Condition {
      context: "journal entry should become visible",
    })
  }

  /// Read from journal with `journalctl`.
  #[allow(
    clippy::single_call_fn,
    reason = "journalctl invocation stays isolated from assertion helpers and namespace retry logic"
  )]
  fn read_from_journal(namespace: JournalNamespace, test_name: &str) -> Result<Vec<HashMap<String, Field>>, TestFailure> {
    let mut command = Command::new("journalctl");
    if namespace == JournalNamespace::User {
      let _command = command.arg("--user");
    }

    let pid = process::id();
    let output = ensure_ok(
      command
                // We pass --all to circumvent journalctl's default limit of 4096 bytes for field values
                .args(["--output=json", "--all"])
                // Filter by the PID of the current test process
                .arg(format!("_PID={pid}"))
                .arg(format!("TEST_NAME={test_name}"))
                .output(),
      "read journalctl output",
    )?;
    let stdout = String::from_utf8_lossy(&output.stdout);

    stdout
      .lines()
      .map(|line| ensure_ok(serde_json::from_str(line), "parse journalctl JSON line"))
      .collect()
  }

  /// Read exactly one line from journal for the given test name.
  fn retry_read_one_line_from_journal(testname: &str) -> Result<HashMap<String, Field>, TestFailure> {
    retry_read_one_line_from_namespace(JournalNamespace::System, testname)
  }

  #[allow(
    clippy::single_call_fn,
    reason = "user-journal tests keep namespace-specific read helper parallel to system reads"
  )]
  fn retry_read_one_line_from_user_journal(testname: &str) -> Result<HashMap<String, Field>, TestFailure> {
    retry_read_one_line_from_namespace(JournalNamespace::User, testname)
  }

  fn retry_read_one_line_from_namespace(namespace: JournalNamespace, testname: &str) -> Result<HashMap<String, Field>, TestFailure> {
    retry(|| {
      let mut messages = read_from_journal(namespace, testname).ok()?;
      if messages.len() == 1 { messages.pop() } else { None }
    })
  }

  fn field<'a>(message: &'a HashMap<String, Field>, name: &'static str) -> Result<&'a Field, TestFailure> {
    ensure_some(message.get(name), "journal field exists")
  }

  fn ensure_text(message: &HashMap<String, Field>, name: &'static str, expected: &str) -> Result<(), TestFailure> {
    let actual = ensure_some(field(message, name)?.as_text(), "journal field is text")?;
    ensure_eq(&actual, &expected, "journal text field matches")
  }

  #[allow(
    clippy::single_call_fn,
    reason = "binary journal field assertion mirrors text and array assertion helpers in protocol tests"
  )]
  fn ensure_binary(message: &HashMap<String, Field>, name: &'static str, expected: &[u8]) -> Result<(), TestFailure> {
    ensure(field(message, name)?.bytes_eq(expected), "journal binary field matches")
  }

  fn ensure_array(message: &HashMap<String, Field>, name: &'static str, expected: &[&str]) -> Result<(), TestFailure> {
    let actual = ensure_some(field(message, name)?.as_array(), "journal field is array")?;
    ensure(
      actual.iter().map(String::as_str).eq(expected.iter().copied()),
      "journal array field matches",
    )
  }

  fn ensure_text_present(message: &HashMap<String, Field>, name: &'static str) -> Result<(), TestFailure> {
    ensure(field(message, name)?.as_text().is_some(), "journal text field is present")
  }

  #[test]
  fn simple_message() -> Result<(), TestFailure> {
    with_journald(|| {
      info!(test.name = "simple_message", "Hello World");

      let message = retry_read_one_line_from_journal("simple_message")?;
      ensure_text(&message, "MESSAGE", "Hello World")?;
      ensure_text(&message, "PRIORITY", "5")
    })
  }

  #[test]
  fn simple_message_user_journal() -> Result<(), TestFailure> {
    with_user_journald(|| {
      info!(test.name = "simple_message_user_journal", "Hello User Journal");

      let message = retry_read_one_line_from_user_journal("simple_message_user_journal")?;
      ensure_text(&message, "MESSAGE", "Hello User Journal")?;
      ensure_text(&message, "PRIORITY", "5")
    })
  }

  #[test]
  fn custom_priorities() -> Result<(), TestFailure> {
    fn check_message(level: &str, priority: &str) -> Result<(), TestFailure> {
      let entry = retry_read_one_line_from_journal(&format!("custom_priority.{level}"))?;
      ensure_text(&entry, "MESSAGE", &format!("hello {level}"))?;
      ensure_text(&entry, "PRIORITY", priority)
    }

    let priorities = PriorityMappings {
      error: Priority::Critical,
      warn:  Priority::Error,
      info:  Priority::Warning,
      debug: Priority::Notice,
      trace: Priority::Informational,
    };
    let layer = ensure_ok(Layer::new(), "system journald layer opens")?
      .with_field_prefix(None)
      .with_priority_mappings(priorities);
    let test = || {
      trace!(test.name = "custom_priority.trace", "hello trace");
      check_message("trace", "6")?;
      debug!(test.name = "custom_priority.debug", "hello debug");
      check_message("debug", "5")?;
      info!(test.name = "custom_priority.info", "hello info");
      check_message("info", "4")?;
      warn!(test.name = "custom_priority.warn", "hello warn");
      check_message("warn", "3")?;
      error!(test.name = "custom_priority.error", "hello error");
      check_message("error", "2")
    };

    with_journald_layer(JournalNamespace::System, layer, test)
  }

  #[test]
  fn multiline_message() -> Result<(), TestFailure> {
    with_journald(|| {
      warn!(test.name = "multiline_message", "Hello\nMultiline\nWorld");

      let message = retry_read_one_line_from_journal("multiline_message")?;
      ensure_text(&message, "MESSAGE", "Hello\nMultiline\nWorld")?;
      ensure_text(&message, "PRIORITY", "4")
    })
  }

  #[test]
  fn multiline_message_trailing_newline() -> Result<(), TestFailure> {
    with_journald(|| {
      error!(test.name = "multiline_message_trailing_newline", "A trailing newline\n");

      let message = retry_read_one_line_from_journal("multiline_message_trailing_newline")?;
      ensure_text(&message, "MESSAGE", "A trailing newline\n")?;
      ensure_text(&message, "PRIORITY", "3")
    })
  }

  #[test]
  fn internal_null_byte() -> Result<(), TestFailure> {
    with_journald(|| {
      debug!(test.name = "internal_null_byte", "An internal\x00byte");

      let message = retry_read_one_line_from_journal("internal_null_byte")?;
      ensure_binary(&message, "MESSAGE", b"An internal\x00byte")?;
      ensure_text(&message, "PRIORITY", "6")
    })
  }

  #[test]
  fn large_message() -> Result<(), TestFailure> {
    let large_string = "b".repeat(512_000);
    with_journald(|| {
      debug!(test.name = "large_message", "Message: {}", large_string);

      let message = retry_read_one_line_from_journal("large_message")?;
      ensure_text(&message, "MESSAGE", &format!("Message: {large_string}"))?;
      ensure_text(&message, "PRIORITY", "6")
    })
  }

  #[test]
  fn simple_metadata() -> Result<(), TestFailure> {
    let sub = ensure_ok(Layer::new(), "system journald layer opens")?
      .with_field_prefix(None)
      .with_syslog_identifier("test_ident".to_owned());
    with_journald_layer(JournalNamespace::System, sub, || {
      info!(
          target: "journal",
          { test.name = "simple_metadata" },
          "Hello World"
      );

      let message = retry_read_one_line_from_journal("simple_metadata")?;
      ensure_text(&message, "MESSAGE", "Hello World")?;
      ensure_text(&message, "PRIORITY", "5")?;
      ensure_text(&message, "TARGET", "journal")?;
      ensure_text(&message, "SYSLOG_IDENTIFIER", "test_ident")?;
      ensure_text_present(&message, "CODE_FILE")?;
      ensure_text_present(&message, "CODE_LINE")
    })
  }

  #[test]
  fn journal_fields() -> Result<(), TestFailure> {
    let sub = ensure_ok(Layer::new(), "system journald layer opens")?
      .with_field_prefix(None)
      .with_custom_fields([("SYSLOG_FACILITY", "17")])
      .with_custom_fields([("ABC", "dEf"), ("XYZ", "123")]);
    with_journald_layer(JournalNamespace::System, sub, || {
      info!(
          target: "journal",
          { test.name = "journal_fields" },
          "Hello World"
      );

      let message = retry_read_one_line_from_journal("journal_fields")?;
      ensure_text(&message, "MESSAGE", "Hello World")?;
      ensure_text(&message, "PRIORITY", "5")?;
      ensure_text(&message, "TARGET", "journal")?;
      ensure_text(&message, "SYSLOG_FACILITY", "17")?;
      ensure_text(&message, "ABC", "dEf")?;
      ensure_text(&message, "XYZ", "123")?;
      ensure_text_present(&message, "CODE_FILE")?;
      ensure_text_present(&message, "CODE_LINE")
    })
  }

  #[test]
  fn span_metadata() -> Result<(), TestFailure> {
    with_journald(|| {
      let s1 = info_span!("span1", span_field1 = "foo1");
      let _g1 = s1.enter();

      info!(
          target: "journal",
          { test.name = "span_metadata" },
          "Hello World"
      );

      let message = retry_read_one_line_from_journal("span_metadata")?;
      ensure_text(&message, "MESSAGE", "Hello World")?;
      ensure_text(&message, "PRIORITY", "5")?;
      ensure_text(&message, "TARGET", "journal")?;
      ensure_text(&message, "SPAN_FIELD1", "foo1")?;
      ensure_text(&message, "SPAN_NAME", "span1")?;
      ensure_text_present(&message, "CODE_FILE")?;
      ensure_text_present(&message, "CODE_LINE")?;
      ensure_text_present(&message, "SPAN_CODE_FILE")?;
      ensure_text_present(&message, "SPAN_CODE_LINE")
    })
  }

  #[test]
  fn multiple_spans_metadata() -> Result<(), TestFailure> {
    with_journald(|| {
      let s1 = info_span!("span1", span_field1 = "foo1");
      let _g1 = s1.enter();
      let s2 = info_span!("span2", span_field1 = "foo2");
      let _g2 = s2.enter();

      info!(
          target: "journal",
          { test.name = "multiple_spans_metadata" },
          "Hello World"
      );

      let message = retry_read_one_line_from_journal("multiple_spans_metadata")?;
      ensure_text(&message, "MESSAGE", "Hello World")?;
      ensure_text(&message, "PRIORITY", "5")?;
      ensure_text(&message, "TARGET", "journal")?;
      ensure_array(&message, "SPAN_FIELD1", &["foo1", "foo2"])?;
      ensure_array(&message, "SPAN_NAME", &["span1", "span2"])?;
      ensure_text_present(&message, "CODE_FILE")?;
      ensure_text_present(&message, "CODE_LINE")?;
      let _span_code_file = field(&message, "SPAN_CODE_FILE")?;
      let span_code_line = ensure_some(field(&message, "SPAN_CODE_LINE")?.as_array(), "span code line is array")?;
      ensure(span_code_line.len() == 2, "span code line contains both spans")
    })
  }

  #[test]
  fn spans_field_collision() -> Result<(), TestFailure> {
    with_journald(|| {
      let s1 = info_span!("span1", span_field = "foo1");
      let _g1 = s1.enter();
      let s2 = info_span!("span2", span_field = "foo2");
      let _g2 = s2.enter();

      info!(test.name = "spans_field_collision", span_field = "foo3", "Hello World");

      let message = retry_read_one_line_from_journal("spans_field_collision")?;
      ensure_text(&message, "MESSAGE", "Hello World")?;
      ensure_array(&message, "SPAN_NAME", &["span1", "span2"])?;
      ensure_array(&message, "SPAN_FIELD", &["foo1", "foo2", "foo3"])
    })
  }
}
