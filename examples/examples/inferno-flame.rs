//! Example binary for tracing workspace checks.

use std::env;
use std::error::Error;
use std::fs::File;
use std::io::BufReader;
use std::io::BufWriter;
use std::io::Write as _;
use std::io::stdout;
use std::path::Path;
use std::path::PathBuf;
use std::thread::sleep;
use std::time::Duration;

use inferno::flamegraph::Options;
use inferno::flamegraph::{
  self,
};
use tempfile::Builder as TempDirBuilder;
use tracing::Level;
use tracing::span;
use tracing::subscriber::set_global_default;
use tracing_flame::FlameLayer;
use tracing_subscriber::prelude::*;
use tracing_subscriber::registry::Registry;

/// Folded stack output file consumed by `inferno`.
const FLAME_FOLDED_FILE: &str = "flame.folded";

/// Install a global subscriber that writes folded stack samples.
#[allow(
  clippy::single_call_fn,
  reason = "keeps flame-layer setup separate from the simulated workload"
)]
fn setup_global_subscriber(dir: &Path) -> Result<impl Drop + use<>, Box<dyn Error>> {
  let (flame_layer, guard) = FlameLayer::with_file(dir.join(FLAME_FOLDED_FILE))?;

  let subscriber = Registry::default().with(flame_layer);

  set_global_default(subscriber)?;

  Ok(guard)
}

/// Render the folded samples into an SVG flamegraph.
#[allow(
  clippy::single_call_fn,
  reason = "keeps inferno rendering separate from folded-sample generation"
)]
fn make_flamegraph(tmpdir: &Path, output_path: &Path) -> Result<(), Box<dyn Error>> {
  let mut output = stdout().lock();
  let displayed_path = output_path.display();
  writeln!(output, "outputting flamegraph to {displayed_path}")?;
  let folded_file = File::open(tmpdir.join(FLAME_FOLDED_FILE))?;
  let reader = BufReader::new(folded_file);

  let output_file = File::create(output_path)?;
  let writer = BufWriter::new(output_file);

  let mut options = Options::default();
  flamegraph::from_reader(&mut options, reader, writer)?;
  Ok(())
}

/// Return the requested SVG path or the default path in the current directory.
#[allow(
  clippy::single_call_fn,
  reason = "keeps CLI output-path selection separate from flamegraph rendering"
)]
fn output_path() -> Result<PathBuf, Box<dyn Error>> {
  let path = env::args().nth(1).map_or_else(
    || -> Result<PathBuf, Box<dyn Error>> {
      let mut path = env::current_dir()?;
      path.push("tracing-flame-inferno.svg");
      Ok(path)
    },
    |arg| -> Result<PathBuf, Box<dyn Error>> { Ok(PathBuf::from(arg)) },
  )?;
  Ok(path)
}

/// Run the `inferno` flamegraph example.
fn main() -> Result<(), Box<dyn Error>> {
  let output_path = output_path()?;
  // setup the flame layer
  let tmp_dir = TempDirBuilder::new().prefix("flamegraphs").tempdir()?;
  let guard = setup_global_subscriber(tmp_dir.path())?;

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

  // drop the guard to make sure the layer flushes its output then read the
  // output to create the flamegraph
  drop(guard);
  make_flamegraph(tmp_dir.path(), output_path.as_path())?;
  Ok(())
}
