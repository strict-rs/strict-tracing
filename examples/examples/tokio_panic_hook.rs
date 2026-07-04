//! This example installs a real panic hook for external panics that reach the
//! process hook while Tokio tasks are running.
//!
//! It also demonstrates the Tokio task completion boundary, where production
//! code should classify panicked or cancelled task failures and record them as
//! structured errors. The demo tasks complete successfully and do not
//! manufacture panic hook input or intentionally panic.

use std::error::Error;
use std::panic as panic_runtime;

use futures::future::join_all;
use tokio::spawn;
use tokio::task::JoinError;
use tokio::task::yield_now;
use tracing::Level;
use tracing::error;
use tracing::info;
use tracing::subscriber::set_global_default;
use tracing::trace;

/// Run the Tokio panic hook example.
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
  let subscriber = tracing_subscriber::fmt().with_max_level(Level::TRACE).finish();

  // NOTE: Using `tracing` in a panic hook requires the use of the *global*
  // trace dispatcher (`tracing::subscriber::set_global_default`), rather than
  // the per-thread scoped dispatcher
  // (`tracing::subscriber::with_default`/`set_default`). With the scoped trace
  // dispatcher, the subscriber's thread-local context may already have been
  // torn down by unwinding by the time the panic handler is reached.
  set_global_default(subscriber)?;

  panic_runtime::set_hook(Box::new(record_panic));

  // Spawn tasks to check the numbers from 1-10.
  let tasks = (0..10).map(|number| spawn(check_number(number))).collect::<Vec<_>>();
  let task_results = join_all(tasks).await;

  // The demo tasks are successful. This boundary still records failures from
  // externally supplied task work without manufacturing panic hook input.
  let record_task_completion = |task_result: Result<(), JoinError>| {
    let Err(error) = task_result else {
      return;
    };

    let panicked = error.is_panic();
    let cancelled = error.is_cancelled();
    let failure = if panicked {
      "panic"
    } else if cancelled {
      "cancelled"
    } else {
      "unknown"
    };

    error!(
        error = %error,
        task.failure = failure,
        task.panicked = panicked,
        task.cancelled = cancelled,
        "Tokio task failed"
    );
  };

  for task_result in task_results {
    record_task_completion(task_result);
  }

  trace!("all tasks done");
  Ok(())
}

/// Record a panic with its source location when one is available.
#[allow(
  clippy::single_call_fn,
  reason = "keeps the panic-hook callback separate from hook installation"
)]
fn record_panic(panic_info: &panic_runtime::PanicHookInfo<'_>) {
  let message = panic_info.to_string();

  if let Some(location) = panic_info.location() {
    error!(
      message = message.as_str(),
      panic.source = "hook",
      panic.file = location.file(),
      panic.line = location.line(),
      panic.column = location.column(),
    );
  } else {
    error!(message = message.as_str(), panic.source = "hook",);
  }
}

/// Check one number in an async task.
#[allow(
  clippy::single_call_fn,
  reason = "keeps the instrumented task span visible around task work"
)]
#[tracing::instrument]
async fn check_number(number: i32) {
  trace!("checking number...");
  yield_now().await;

  info!(number, "number checks out");
}
