//! Collapsed flame layer integration tests.

/// Test cases for collapsed flame output.
#[cfg(test)]
mod tests {
    use std::fmt;
    use std::io;
    use std::thread::sleep;
    use std::time::Duration;
    use tracing::subscriber::{self, SetGlobalDefaultError};
    use tracing::{Level, span};
    use tracing_flame::{FlameError, FlameLayer};
    use tracing_subscriber::{prelude::*, registry::Registry};

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
    }

    impl fmt::Debug for TestFailure {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            match *self {
                Self::IoError(ref source) => {
                    formatter.debug_tuple("IoError").field(source).finish()
                }
                Self::FlameLayer(ref source) => {
                    formatter.debug_tuple("FlameLayer").field(source).finish()
                }
                Self::Subscriber(ref source) => {
                    formatter.debug_tuple("Subscriber").field(source).finish()
                }
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

    /// Captures a nested span tree without panicking.
    #[test]
    fn capture_supported() -> TestResult {
        let tmp_dir = tempfile::Builder::new()
            .prefix("tracing-flamegraph-test-")
            .tempdir()?;
        let (flame_layer, _flame_guard) =
            FlameLayer::with_file(tmp_dir.path().join("tracing.folded"))?;

        let subscriber = Registry::default().with(flame_layer);

        subscriber::set_global_default(subscriber)?;

        let outer_span = span!(Level::ERROR, "outer");
        let outer_guard = outer_span.enter();
        sleep(Duration::from_millis(10));

        let inner_span = span!(Level::ERROR, "Inner");
        let inner_guard = inner_span.enter();
        sleep(Duration::from_millis(50));

        let innermost_span = span!(Level::ERROR, "Innermost");
        let innermost_guard = innermost_span.enter();
        sleep(Duration::from_millis(50));
        drop(innermost_guard);

        drop(inner_guard);
        sleep(Duration::from_millis(5));
        drop(outer_guard);

        sleep(Duration::from_millis(500));

        tmp_dir.close()?;
        Ok(())
    }
}
