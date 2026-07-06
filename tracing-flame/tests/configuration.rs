//! Configuration and writer behavior contracts for `tracing-flame`.

#[cfg(test)]
mod tests {
  use std::error::Error as _;
  use std::fs;
  use std::fs::File;
  use std::io;
  use std::io::Write;
  use std::mem::ManuallyDrop;
  use std::sync::mpsc;

  use strict_test_support::TestFailure;
  use strict_test_support::ensure;
  use strict_test_support::ensure_contains;
  use strict_test_support::ensure_eq;
  use strict_test_support::ensure_ok;
  use strict_test_support::ensure_some;
  use tracing::subscriber::with_default;
  use tracing_flame::FlameLayer;
  use tracing_subscriber::prelude::*;
  use tracing_subscriber::registry::Registry;

  #[derive(Debug)]
  struct RecordingSink {
    chunks: mpsc::Receiver<Vec<u8>>,
  }

  impl RecordingSink {
    fn lines(&self) -> Result<Vec<String>, TestFailure> {
      let mut bytes = Vec::new();
      for chunk in self.chunks.try_iter() {
        bytes.extend(chunk);
      }
      let output = ensure_ok(String::from_utf8(bytes), "recorded flame output is UTF-8")?;
      Ok(output.lines().map(str::to_owned).collect())
    }
  }

  #[derive(Clone, Debug)]
  struct RecordingWriter {
    chunks: mpsc::Sender<Vec<u8>>,
  }

  impl Write for RecordingWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
      let _send_result: Result<(), mpsc::SendError<Vec<u8>>> = self.chunks.send(buffer.to_vec());
      Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
      Ok(())
    }
  }

  #[derive(Debug, Default)]
  struct FailingFlushWriter;

  impl Write for FailingFlushWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
      Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
      Err(io::Error::from(io::ErrorKind::BrokenPipe))
    }
  }

  fn capture_lines(
    configure: impl FnOnce(FlameLayer<Registry, RecordingWriter>) -> FlameLayer<Registry, RecordingWriter>,
    run: impl FnOnce(),
  ) -> Result<Vec<String>, TestFailure> {
    let (chunks, receiver) = mpsc::channel();
    let writer = RecordingWriter {
      chunks,
    };
    let sink = RecordingSink {
      chunks: receiver
    };
    let layer = configure(FlameLayer::new(writer));
    let guard = layer.flush_on_drop();
    let subscriber = Registry::default().with(layer);

    with_default(subscriber, run);
    ensure_ok(guard.flush(), "flame guard flushes recorded output")?;
    sink.lines()
  }

  fn ensure_line_contains<'a>(lines: &'a [String], needle: &str, context: &'static str) -> Result<&'a str, TestFailure> {
    let line = ensure_some(lines.iter().find(|line| line.contains(needle)), context)?;
    Ok(line.as_str())
  }

  fn create_nested_spans() {
    let outer = tracing::info_span!("outer");
    let outer_guard = outer.enter();
    let inner = tracing::info_span!("Inner");
    let inner_guard = inner.enter();
    drop(inner_guard);
    drop(outer_guard);
  }

  #[test]
  fn configuration_flags_shape_folded_output() -> Result<(), TestFailure> {
    let lines = capture_lines(
      |layer| {
        layer
          .with_empty_samples(false)
          .with_threads_collapsed(true)
          .with_module_path(false)
          .with_file_and_line(true)
      },
      || {
        let root = tracing::info_span!("root_span");
        let _root_guard = root.enter();
      },
    )?;

    ensure_eq(&lines.len(), &1_usize, "empty root-entry sample is omitted")?;
    let root_line = ensure_line_contains(&lines, "root_span", "root span exit sample is recorded")?;
    ensure(
      root_line.starts_with("all-threads;"),
      "collapsed thread output uses the synthetic thread prefix",
    )?;
    ensure_contains(root_line, "configuration.rs:", "file and line output includes the test file")?;
    ensure(
      !root_line.contains("configuration::"),
      "module path is omitted when module path output is disabled",
    )
  }

  #[test]
  fn manual_flush_writes_all_previously_emitted_lines() -> Result<(), TestFailure> {
    let lines = capture_lines(|layer| layer, create_nested_spans)?;

    ensure(!lines.is_empty(), "manual flush writes at least one folded sample")?;
    let outer_line = ensure_line_contains(&lines, "outer", "outer span appears in folded output")?;
    let inner_line = ensure_line_contains(&lines, "Inner", "inner span appears in folded output")?;
    ensure_contains(inner_line, "outer", "nested span stack includes the parent span")?;
    ensure(
      outer_line.split_whitespace().last().is_some_and(|sample| !sample.is_empty()),
      "folded output includes a non-empty sample suffix",
    )
  }

  #[test]
  fn with_file_creates_and_flushes_folded_output() -> Result<(), TestFailure> {
    let temp_dir = ensure_ok(
      tempfile::Builder::new().prefix("tracing-flame-file-").tempdir(),
      "temp dir is created",
    )?;
    let path = temp_dir.path().join("tracing.folded");
    let (layer, guard) = ensure_ok(FlameLayer::with_file(&path), "file-backed flame layer is created")?;
    let subscriber = Registry::default().with(layer);

    with_default(subscriber, create_nested_spans);
    ensure_ok(guard.flush(), "file-backed flame guard flushes")?;

    let output = ensure_ok(fs::read_to_string(&path), "folded output file is readable")?;
    ensure_contains(&output, "outer", "file-backed output contains outer span")?;
    ensure_contains(&output, "Inner", "file-backed output contains inner span")?;
    ensure_ok(temp_dir.close(), "temporary flame directory closes")
  }

  #[test]
  fn with_file_reports_create_errors_with_source() -> Result<(), TestFailure> {
    let temp_dir = ensure_ok(
      tempfile::Builder::new().prefix("tracing-flame-missing-").tempdir(),
      "temp dir is created",
    )?;
    let missing_path = temp_dir.path().join("missing").join("tracing.folded");

    let error = ensure_some(
      FlameLayer::<Registry, io::BufWriter<File>>::with_file(&missing_path).err(),
      "missing output directory returns an error",
    )?;
    ensure_contains(
      &error.to_string(),
      "cannot create output file",
      "create-file error display names the failed operation",
    )?;
    ensure(error.source().is_some(), "create-file error preserves the I/O source")?;
    ensure_ok(temp_dir.close(), "temporary flame directory closes")
  }

  #[test]
  fn flush_guard_reports_flush_errors_with_source() -> Result<(), TestFailure> {
    let layer = FlameLayer::<Registry, FailingFlushWriter>::new(FailingFlushWriter);
    let guard = ManuallyDrop::new(layer.flush_on_drop());

    let error = ensure_some(guard.flush().err(), "failing writer returns a flush error")?;
    ensure_eq(
      &error.to_string(),
      &"cannot flush output buffer".to_owned(),
      "flush error display names the failed operation",
    )?;
    ensure(error.source().is_some(), "flush error preserves the I/O source")
  }

  #[test]
  fn thread_collapse_prefixes_every_emitted_line() -> Result<(), TestFailure> {
    let lines = capture_lines(
      |layer| layer.with_threads_collapsed(true).with_empty_samples(false),
      create_nested_spans,
    )?;
    ensure(!lines.is_empty(), "collapsed-thread output emits folded lines")?;
    ensure(
      lines.iter().all(|line| line.starts_with("all-threads;")),
      "all collapsed-thread lines use the synthetic prefix",
    )
  }
}
