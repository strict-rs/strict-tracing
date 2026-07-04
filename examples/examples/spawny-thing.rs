//! Example binary for tracing workspace checks.
#![deny(rust_2018_idioms)]

use std::error::Error;

/// This is a example showing how information is scoped.
///
/// You can run this example by running the following command in a terminal
///
/// ```
/// cargo run --example spawny_thing
/// ```
use futures::future::join_all;
use tracing::debug;
use tracing::info;
use tracing_attributes::instrument;

/// Spawn and await a set of subtasks before logging their combined result.
#[allow(
  clippy::single_call_fn,
  reason = "keeps the parent task span visible in the async task-scoping example"
)]
#[instrument]
async fn parent_task(subtasks: usize) {
  info!("spawning subtasks...");
  let subtask_futures = (1..=subtasks)
    .map(|number| {
      debug!(message = "creating subtask;", number);
      subtask(number)
    })
    .collect::<Vec<_>>();

  let result = join_all(subtask_futures).await;

  debug!("all subtasks completed");
  let sum: usize = result.into_iter().sum();
  info!(sum);
}

/// Return the subtask number after recording that it was polled.
#[allow(
  clippy::single_call_fn,
  reason = "keeps each subtask as its own instrumented span in the scoping example"
)]
#[instrument]
async fn subtask(number: usize) -> usize {
  info!("polling subtask...");
  number
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
  tracing_subscriber::fmt().with_max_level(tracing::Level::DEBUG).try_init()?;
  parent_task(10).await;
  Ok(())
}
