//! Example binary for tracing workspace checks.
#![deny(rust_2018_idioms)]
use std::error::Error;

/// This is a example showing how information is scoped with tokio's
/// `task::spawn`.
///
/// You can run this example by running the following command in a terminal
///
/// ```
/// cargo run --example tokio-spawny-thing
/// ```
use futures::future::try_join_all;
use tracing::Instrument as _;
use tracing::Level;
use tracing::debug;
use tracing::info;
use tracing::instrument;
use tracing::span;

/// Result type used by the fallible async example.
type ExampleResult = Result<(), Box<dyn Error + Send + Sync + 'static>>;

/// Spawn instrumented Tokio tasks and log the sum of their outputs.
#[allow(
  clippy::single_call_fn,
  reason = "keeps the parent task span visible in the Tokio task-scoping example"
)]
#[instrument]
async fn parent_task(subtasks: usize) -> ExampleResult {
  info!("spawning subtasks...");
  let subtask_handles = (1..=subtasks)
    .map(|number| {
      let span = span!(Level::INFO, "subtask", %number);
      debug!(message = "creating subtask;", number);
      tokio::spawn(
        async move {
          info!(%number, "polling subtask");
          number
        }
        .instrument(span),
      )
    })
    .collect::<Vec<_>>();

  // the returnable error would be if one of the subtasks panicked.
  let sum: usize = try_join_all(subtask_handles).await?.iter().sum();
  info!(%sum, "all subtasks completed; calculated sum");
  Ok(())
}

#[tokio::main]
async fn main() -> ExampleResult {
  tracing_subscriber::fmt().with_max_level(Level::DEBUG).try_init()?;
  parent_task(10).await?;
  Ok(())
}
