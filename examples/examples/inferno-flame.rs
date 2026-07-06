//! Demonstrates capturing a flamegraph-ready profile of `tracing` spans with `tracing-flame`.
//!
//! A `tracing_flame::FlameLayer` records span enter/exit timings as folded stack samples while a
//! `tracing_subscriber::fmt` layer mirrors events to the console. The example runs a workload of
//! nested spans, flushes the samples through the layer's `FlushGuard`, and reports the finished
//! file's path.
//!
//! The folded stack file is written to the path passed as the first CLI argument, defaulting to
//! `tracing-flame.folded` in the current directory:
//!
//! ```text
//! cargo run -p tracing-examples --example inferno-flame [OUTPUT_PATH]
//! ```
//!
//! Rendering the folded samples into an SVG is a separate step performed by inferno's standalone
//! CLI (installed with `cargo install inferno`):
//!
//! ```text
//! inferno-flamegraph < tracing-flame.folded > flamegraph.svg
//! ```

use std::env;
use std::error::Error;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;
use std::path::PathBuf;
use std::thread::sleep;
use std::time::Duration;

use tracing::Level;
use tracing::info;
use tracing::span;
use tracing::subscriber::set_global_default;
use tracing_flame::FlameLayer;
use tracing_flame::FlushGuard;
use tracing_subscriber::fmt;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::registry;

/// Default folded stack output file, consumed by the external `inferno-flamegraph` CLI.
const FLAME_FOLDED_FILE: &str = "tracing-flame.folded";

/// Flush guard returned by the flame layer in this example.
type ExampleFlushGuard = FlushGuard<BufWriter<File>>;

/// Fallible result type used by this example.
type ExampleResult<T> = Result<T, Box<dyn Error>>;

/// Install a global subscriber that streams folded stack samples to `path` and events to stdout.
#[allow(
  clippy::single_call_fn,
  reason = "keeps flame-layer setup separate from the simulated workload"
)]
fn setup_global_subscriber(path: &Path) -> ExampleResult<ExampleFlushGuard> {
  let (flame_layer, guard) = FlameLayer::with_file(path)?;

  let subscriber = registry().with(fmt::layer()).with(flame_layer);

  set_global_default(subscriber)?;

  Ok(guard)
}

/// Return the requested folded-output path or the default path in the current directory.
#[allow(
  clippy::single_call_fn,
  reason = "keeps CLI output-path selection separate from subscriber setup"
)]
fn folded_path() -> ExampleResult<PathBuf> {
  env::args().nth(1).map_or_else(
    || -> ExampleResult<PathBuf> {
      let mut path = env::current_dir()?;
      path.push(FLAME_FOLDED_FILE);
      Ok(path)
    },
    |arg| -> ExampleResult<PathBuf> { Ok(PathBuf::from(arg)) },
  )
}

/// Run the `tracing-flame` folded stack example.
fn main() -> Result<(), Box<dyn Error>> {
  let folded_path = folded_path()?;
  // setup the flame layer
  let guard = setup_global_subscriber(folded_path.as_path())?;

  // do a bunch of span entering and exiting to simulate a program running
  span!(Level::ERROR, "outer").in_scope(|| {
    sleep(Duration::from_millis(10));
    span!(Level::ERROR, "Inner").in_scope(|| {
      sleep(Duration::from_millis(50));
      span!(Level::ERROR, "Innermost").in_scope(|| {
        sleep(Duration::from_millis(50));
      });
    });
    sleep(Duration::from_millis(5));
  });
  sleep(Duration::from_millis(500));

  // flush the buffered samples through the guard so the folded file is complete,
  // then report the follow-up rendering step
  guard.flush()?;
  info!(folded_file = %folded_path.display(), "wrote folded stack samples");
  info!(
    "render an SVG with: inferno-flamegraph < {} > flamegraph.svg",
    folded_path.display()
  );
  Ok(())
}
