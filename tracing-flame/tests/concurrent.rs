//! Concurrent flame layer integration tests.

/// Test cases for concurrent flame output.
#[cfg(test)]
mod tests {
  use std::fmt;
  use std::fs;
  use std::io;
  use std::thread;
  use std::thread::sleep;
  use std::time::Duration;

  use tracing::Level;
  use tracing::span;
  use tracing::subscriber;
  use tracing::subscriber::SetGlobalDefaultError;
  use tracing_flame::FlameError;
  use tracing_flame::FlameLayer;
  use tracing_subscriber::prelude::*;
  use tracing_subscriber::registry::Registry;

  /// Expected number of folded stack lines emitted by this scenario.
  const EXPECTED_LINE_COUNT: usize = 5;

  /// Result type used by tracing-flame integration tests.
  type TestResult = Result<(), TestFailure>;

  /// Failure cases surfaced by this integration test.
  enum TestFailure {
    /// An I/O operation failed.
    IoError(
      /// The underlying I/O error.
      io::Error,
    ),
    /// Creating or flushing the flame layer failed.
    FlameLayer(
      /// The underlying flame-layer error.
      FlameError,
    ),
    /// Installing the global subscriber failed.
    Subscriber(
      /// The underlying subscriber installation error.
      SetGlobalDefaultError,
    ),
    /// The worker thread panicked before completing its span.
    ThreadPanic,
    /// The folded trace contained an unexpected number of lines.
    UnexpectedLineCount {
      /// The expected line count.
      expected: usize,
      /// The actual line count.
      actual:   usize,
    },
  }

  impl fmt::Debug for TestFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
      match *self {
        Self::IoError(ref source) => formatter.debug_tuple("IoError").field(source).finish(),
        Self::FlameLayer(ref source) => formatter.debug_tuple("FlameLayer").field(source).finish(),
        Self::Subscriber(ref source) => formatter.debug_tuple("Subscriber").field(source).finish(),
        Self::ThreadPanic => formatter.write_str("ThreadPanic"),
        Self::UnexpectedLineCount {
          expected,
          actual,
        } => formatter
          .debug_struct("UnexpectedLineCount")
          .field("expected", &expected)
          .field("actual", &actual)
          .finish(),
      }
    }
  }

  impl From<io::Error> for TestFailure {
    fn from(source: io::Error) -> Self {
      Self::IoError(source)
    }
  }

  impl From<FlameError> for TestFailure {
    fn from(source: FlameError) -> Self {
      Self::FlameLayer(source)
    }
  }

  impl From<SetGlobalDefaultError> for TestFailure {
    fn from(source: SetGlobalDefaultError) -> Self {
      Self::Subscriber(source)
    }
  }

  /// Captures spans emitted by multiple threads.
  #[test]
  fn capture_supported() -> TestResult {
    let tmp_dir = tempfile::Builder::new().prefix("tracing-flamegraph-test-").tempdir()?;
    let path = tmp_dir.path().join("tracing.folded");
    let (flame_layer, flame_guard) = FlameLayer::with_file(&path)?;

    let subscriber = Registry::default().with(flame_layer);

    subscriber::set_global_default(subscriber)?;
    let main_span = span!(Level::ERROR, "main");
    let _main_guard = main_span.enter();

    let worker_thread = span!(Level::ERROR, "outer").in_scope(|| {
      sleep(Duration::from_millis(10));
      let inner_span = span!(Level::ERROR, "Inner");
      let worker_thread = thread::spawn(move || {
        let _inner_guard = inner_span.enter();
        sleep(Duration::from_millis(50));
      });
      sleep(Duration::from_millis(20));
      worker_thread
    });

    sleep(Duration::from_millis(100));

    if worker_thread.join().is_ok() {
      flame_guard.flush()?;

      let traces = fs::read_to_string(&path)?;
      let actual_line_count = traces.lines().count();
      if actual_line_count == EXPECTED_LINE_COUNT {
        tmp_dir.close()?;
        Ok(())
      } else {
        Err(TestFailure::UnexpectedLineCount {
          expected: EXPECTED_LINE_COUNT,
          actual:   actual_line_count,
        })
      }
    } else {
      Err(TestFailure::ThreadPanic)
    }
  }
}
