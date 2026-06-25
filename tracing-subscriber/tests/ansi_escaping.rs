//! Tests ANSI escape sanitization in formatted output.

#[cfg(test)]
mod tests {
    use parking_lot::Mutex;
    use std::sync::Arc;
    use std::{
        error::Error,
        fmt,
        io::{Result as IoResult, Write},
    };
    use strict_test_support::{TestFailure, ensure, ensure_contains, ensure_lacks};
    use tracing::subscriber::with_default;
    use tracing_subscriber::fmt::{MakeWriter, Subscriber};

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
    fn test_error_ansi_escaping() -> Result<(), TestFailure> {
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

        ensure_contains(&output, "An error occurred", "error message is logged")?;
        ensure(!output.contains('\x1b'), "output lacks raw ESC characters")?;
        ensure_contains(&output, "\\x1b", "ESC is escaped as \\x1b")
    }

    /// Test that ANSI escape sequences in log messages are properly escaped
    #[test]
    fn test_message_ansi_escaping() -> Result<(), TestFailure> {
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
        ensure(
            !output.contains('\x1b'),
            "message output lacks raw ESC characters",
        )?;
        ensure(
            !output.contains('\x07'),
            "message output lacks raw BEL characters",
        )
    }

    /// Test that JSON formatter properly escapes ANSI sequences
    #[cfg(feature = "json")]
    #[test]
    fn test_json_ansi_escaping() -> Result<(), TestFailure> {
        let writer = TestWriter::new();
        let subscriber = Subscriber::builder()
            .json()
            .with_writer(writer.clone())
            .finish();

        with_default(subscriber, || {
            let malicious_input = "\x1b]0;PWNED\x07\x1b[2J";

            // JSON formatter should escape ANSI sequences
            tracing::info!("Testing: {}", malicious_input);
            tracing::info!(user_input = %malicious_input, "Field test");
        });

        let output = writer.get_output();

        // JSON should escape ANSI sequences as Unicode escapes
        ensure(
            !output.contains('\x1b'),
            "JSON output lacks raw ESC characters",
        )?;
        ensure(
            !output.contains('\x07'),
            "JSON output lacks raw BEL characters",
        )
    }

    /// Test that pretty formatter properly escapes ANSI sequences
    #[cfg(feature = "ansi")]
    #[test]
    fn test_pretty_ansi_escaping() -> Result<(), TestFailure> {
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
        ensure(
            !output.contains('\x1b'),
            "pretty output lacks raw ESC characters",
        )?;
        ensure(
            !output.contains('\x07'),
            "pretty output lacks raw BEL characters",
        )
    }

    /// Comprehensive test for ANSI sanitization that prevents injection attacks
    #[test]
    fn ansi_sanitization_prevents_injection() -> Result<(), TestFailure> {
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
            &output,
            "\\u{1b}",
            "field values are escaped by Debug formatting",
        )?;

        // Message content should be sanitized
        ensure_contains(&output, "\\x1b", "message content is sanitized")?;
        ensure_lacks(
            &output,
            "\x1b]0;PWNED",
            "message content lacks raw ANSI sequences",
        )?;
        ensure_lacks(
            &output,
            "\x07",
            "message content lacks raw control characters",
        )
    }

    /// Test that C1 control characters (\x80-\x9f) are also properly escaped
    #[test]
    fn test_c1_control_characters_escaping() -> Result<(), TestFailure> {
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
        ensure(
            !output.contains('\u{80}'),
            "output lacks raw C1 control characters",
        )?;
        ensure(!output.contains('\u{9b}'), "output lacks raw CSI character")?;
        ensure(!output.contains('\u{9c}'), "output lacks raw ST character")?;

        // Should contain Unicode escapes for C1 characters
        ensure(
            output.contains("\\u{80}") || output.contains("\\u{8"),
            "output contains escaped C1 characters",
        )
    }

    /// Test that sanitization can be disabled via `with_ansi_sanitization(false)`,
    /// allowing trusted ANSI sequences in messages to pass through.
    #[test]
    fn ansi_sanitization_can_be_disabled_for_messages() -> Result<(), TestFailure> {
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
            &output,
            "\x1b[31mTEST\x1b[0m",
            "ANSI message passes through when sanitization is disabled",
        )
    }

    #[cfg(feature = "ansi")]
    #[test]
    fn ansi_sanitization_can_be_disabled_for_pretty_messages() -> Result<(), TestFailure> {
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
            &output,
            "\x1b[31mTEST\x1b[0m",
            "pretty formatter message passes through when sanitization is disabled",
        )
    }
}
