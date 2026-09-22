//! Tests ANSI escape sanitization in formatted output.
#![cfg(feature = "fmt")]

#[cfg(test)]
mod tests {
  use std::error::Error;
  use std::fmt;
  use std::io::Result as IoResult;
  use std::io::Write;
  use std::sync::Arc;

  use parking_lot::Mutex;

  /// Native failures from these behavioral checks.
  #[derive(Debug, thiserror::Error)]
  enum TestError {
    /// A boolean expectation failed.
    #[error(transparent)]
    Condition(#[from] strict_test_support::ConditionFailure),
    /// Retains the searched text and expected substring.
    #[error(transparent)]
    Substring(#[from] strict_test_support::SubstringFailure<String, String>),
  }

  use strict_test_support::ensure;
  use strict_test_support::ensure_contains;
  use strict_test_support::ensure_lacks;
  use tracing::subscriber::with_default;
  use tracing_subscriber::fmt::MakeWriter;
  use tracing_subscriber::fmt::Subscriber;

  #[derive(Debug)]
  struct InjectionError {
    content: String,
  }

  impl fmt::Display for InjectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
      write!(formatter, "Error: {}", self.content)
    }
  }

  /// Shared test writer that collects output for verification
  #[derive(Debug, Clone)]
  struct TestWriter {
    buf: Arc<Mutex<Vec<u8>>>,
  }

  impl TestWriter {
    fn new() -> Self {
      Self {
        buf: Arc::new(Mutex::new(Vec::new())),
      }
    }

    fn get_output(&self) -> String {
      let buf = self.buf.lock();
      String::from_utf8_lossy(&buf).to_string()
    }
  }

  impl Write for TestWriter {
    fn write(&mut self, buf: &[u8]) -> IoResult<usize> {
      self.buf.lock().extend_from_slice(buf);
      Ok(buf.len())
    }

    fn flush(&mut self) -> IoResult<()> {
      Ok(())
    }
  }

  impl<'a> MakeWriter<'a> for TestWriter {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
      Self::clone(self)
    }
  }

  /// Test that ANSI escape sequences in error Display output are sanitized
  /// when interpolated into the event message.
  #[test]
  fn test_error_ansi_escaping() -> Result<(), TestError> {
    #[derive(Debug)]
    struct MaliciousError(&'static str);

    impl fmt::Display for MaliciousError {
      fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
      }
    }

    impl Error for MaliciousError {}

    let writer = TestWriter::new();
    let subscriber = Subscriber::builder()
      .with_writer(writer.clone())
      .with_ansi(false)
      .without_time()
      .with_target(false)
      .with_level(false)
      .finish();

    with_default(subscriber, || {
      let malicious_error = MaliciousError("\x1b]0;PWNED\x07\x1b[2J\x08\x0c\x7f");

      // Log the error as part of the message so it goes through the
      // message sanitization path (not just Debug field formatting).
      tracing::error!("An error occurred: {}", malicious_error);
    });

    let output = writer.get_output();

    ensure_contains((output).clone(), String::from("An error occurred"), "error message is logged").map(drop)?;
    ensure(!output.contains('\x1b'), "output lacks raw ESC characters").map(drop)?;
    ensure_contains(output, String::from("\\x1b"), "ESC is escaped as \\x1b")
      .map(drop)
      .map_err(TestError::from)
  }

  /// Test that ANSI escape sequences in log messages are properly escaped
  #[test]
  fn test_message_ansi_escaping() -> Result<(), TestError> {
    let writer = TestWriter::new();
    let subscriber = Subscriber::builder()
      .with_writer(writer.clone())
      .with_ansi(false)
      .without_time()
      .with_target(false)
      .with_level(false)
      .finish();

    with_default(subscriber, || {
      let malicious_input = "\x1b]0;PWNED\x07\x1b[2J\x08\x0c\x7f";

      // This should not cause ANSI injection
      tracing::info!("User input: {}", malicious_input);
    });

    let output = writer.get_output();

    // Verify ANSI sequences are escaped
    ensure(!output.contains('\x1b'), "message output lacks raw ESC characters").map(drop)?;
    ensure(!output.contains('\x07'), "message output lacks raw BEL characters")
      .map(drop)
      .map_err(TestError::from)
  }

  /// Test that JSON formatter properly escapes ANSI sequences
  #[cfg(feature = "json")]
  #[test]
  fn test_json_ansi_escaping() -> Result<(), TestError> {
    let writer = TestWriter::new();
    let subscriber = Subscriber::builder().json().with_writer(writer.clone()).finish();

    with_default(subscriber, || {
      let malicious_input = "\x1b]0;PWNED\x07\x1b[2J";

      // JSON formatter should escape ANSI sequences
      tracing::info!("Testing: {}", malicious_input);
      tracing::info!(user_input = %malicious_input, "Field test");
    });

    let output = writer.get_output();

    // JSON should escape ANSI sequences as Unicode escapes
    ensure(!output.contains('\x1b'), "JSON output lacks raw ESC characters").map(drop)?;
    ensure(!output.contains('\x07'), "JSON output lacks raw BEL characters")
      .map(drop)
      .map_err(TestError::from)
  }

  /// Test that pretty formatter properly escapes ANSI sequences
  #[cfg(feature = "ansi")]
  #[test]
  fn test_pretty_ansi_escaping() -> Result<(), TestError> {
    let writer = TestWriter::new();
    let subscriber = Subscriber::builder()
      .pretty()
      .with_writer(writer.clone())
      .with_ansi(false)
      .without_time()
      .with_target(false)
      .finish();

    with_default(subscriber, || {
      let malicious_input = "\x1b]0;PWNED\x07\x1b[2J";

      // Pretty formatter should escape ANSI sequences
      tracing::info!("Testing: {}", malicious_input);
    });

    let output = writer.get_output();

    // Verify ANSI sequences are escaped
    ensure(!output.contains('\x1b'), "pretty output lacks raw ESC characters").map(drop)?;
    ensure(!output.contains('\x07'), "pretty output lacks raw BEL characters")
      .map(drop)
      .map_err(TestError::from)
  }

  /// Comprehensive test for ANSI sanitization that prevents injection attacks
  #[test]
  fn ansi_sanitization_prevents_injection() -> Result<(), TestError> {
    let writer = TestWriter::new();
    let subscriber = Subscriber::builder()
      .with_writer(writer.clone())
      .with_ansi(false)
      .without_time()
      .with_target(false)
      .with_level(false)
      .finish();

    with_default(subscriber, || {
      // Test 1: Field values should remain properly escaped by Debug (baseline)
      let malicious_field_value = "\x1b]0;PWNED\x07\x1b[2J";
      tracing::error!(malicious_field = malicious_field_value, "Field test");

      // Test 2: Message content vulnerability should be mitigated
      let malicious_error = InjectionError {
        content: "\x1b]0;PWNED\x07\x1b[2J".to_owned(),
      };
      tracing::error!("{}", malicious_error);
    });

    let output = writer.get_output();

    // Field values should contain escaped sequences like \u{1b}
    ensure_contains(
      (output).clone(),
      String::from("\\u{1b}"),
      "field values are escaped by Debug formatting",
    )
    .map(drop)?;

    // Message content should be sanitized
    ensure_contains((output).clone(), String::from("\\x1b"), "message content is sanitized").map(drop)?;
    ensure_lacks(
      (output).clone(),
      String::from("\x1b]0;PWNED"),
      "message content lacks raw ANSI sequences",
    )
    .map(drop)?;
    ensure_lacks(output, String::from("\x07"), "message content lacks raw control characters")
      .map(drop)
      .map_err(TestError::from)
  }

  /// Test that C1 control characters (\x80-\x9f) are also properly escaped
  #[test]
  fn test_c1_control_characters_escaping() -> Result<(), TestError> {
    let writer = TestWriter::new();
    let subscriber = Subscriber::builder()
      .with_writer(writer.clone())
      .with_ansi(false)
      .without_time()
      .with_target(false)
      .with_level(false)
      .finish();

    with_default(subscriber, || {
      // Test C1 control characters that can be used in 8-bit terminal escape sequences
      let c1_controls = "\u{80}\u{85}\u{90}\u{9b}\u{9c}\u{9d}\u{9e}\u{9f}"; // Various C1 controls including CSI

      // This should escape C1 control characters to prevent 8-bit escape sequences
      tracing::info!("C1 controls: {}", c1_controls);
    });

    let output = writer.get_output();

    // Verify C1 control characters are escaped
    ensure(!output.contains('\u{80}'), "output lacks raw C1 control characters").map(drop)?;
    ensure(!output.contains('\u{9b}'), "output lacks raw CSI character").map(drop)?;
    ensure(!output.contains('\u{9c}'), "output lacks raw ST character").map(drop)?;

    // Should contain Unicode escapes for C1 characters
    ensure(
      output.contains("\\u{80}") || output.contains("\\u{8"),
      "output contains escaped C1 characters",
    )
    .map(drop)
    .map_err(TestError::from)
  }

  /// Test that sanitization can be disabled via `with_ansi_sanitization(false)`,
  /// allowing trusted ANSI sequences in messages to pass through.
  #[test]
  fn ansi_sanitization_can_be_disabled_for_messages() -> Result<(), TestError> {
    let writer = TestWriter::new();
    let subscriber = Subscriber::builder()
      .with_writer(writer.clone())
      .with_ansi(false)
      .with_ansi_sanitization(false)
      .without_time()
      .with_target(false)
      .with_level(false)
      .finish();

    with_default(subscriber, || {
      tracing::info!("Trusted color: \x1b[31mTEST\x1b[0m");
    });

    let output = writer.get_output();

    ensure_contains(
      output,
      String::from("\x1b[31mTEST\x1b[0m"),
      "ANSI message passes through when sanitization is disabled",
    )
    .map(drop)
    .map_err(TestError::from)
  }

  #[cfg(feature = "ansi")]
  #[test]
  fn ansi_sanitization_can_be_disabled_for_pretty_messages() -> Result<(), TestError> {
    let writer = TestWriter::new();
    let subscriber = Subscriber::builder()
      .pretty()
      .with_writer(writer.clone())
      .with_ansi(false)
      .with_ansi_sanitization(false)
      .without_time()
      .with_target(false)
      .finish();

    with_default(subscriber, || {
      tracing::info!("Trusted color: \x1b[31mTEST\x1b[0m");
    });

    let output = writer.get_output();
    ensure_contains(
      output,
      String::from("\x1b[31mTEST\x1b[0m"),
      "pretty formatter message passes through when sanitization is disabled",
    )
    .map(drop)
    .map_err(TestError::from)
  }
}
