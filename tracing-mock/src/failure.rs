//! Expectation failure reporting.

use std::sync::Arc;

use parking_lot::Mutex;
use tracing_core::subscriber::SubscriberError;
use tracing_core::subscriber::SubscriberResult;

/// Error type used internally for recorded expectation failures.
pub(super) type ExpectationError = SubscriberError;

/// Result returned by expectation checks.
pub(super) type ExpectationResult<T = ()> = SubscriberResult<T>;

/// Shared first-failure state for a running mock and its handle.
#[derive(Clone, Debug, Default)]
pub(super) struct SharedFailures {
  /// The first expectation error observed by the running mock.
  first: Arc<Mutex<Option<ExpectationError>>>,
}

impl SharedFailures {
  /// Records an error if no earlier failure was observed.
  pub(super) fn record(&self, error: ExpectationError) {
    let mut first = self.first.lock();
    if first.is_none() {
      *first = Some(error);
    }
  }

  /// Records an error result and returns successful values.
  pub(super) fn record_result<T>(&self, result: ExpectationResult<T>) -> Option<T> {
    match result {
      Ok(checked) => Some(checked),
      Err(error) => {
        self.record(error);
        None
      }
    }
  }

  /// Returns the first recorded failure, if any.
  pub(super) fn first(&self) -> Option<ExpectationError> {
    self.first.lock().clone()
  }
}
