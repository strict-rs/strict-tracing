//! Test utilities shared by the tracing workspace.

use std::future::Future;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

/// A future that remains pending for a fixed number of polls before completing.
#[derive(Debug)]
pub struct PollN<T, E> {
  /// Result returned once the future becomes ready.
  result:               Option<Result<T, E>>,
  /// Number of pending polls before returning `result`.
  pending_before_ready: usize,
}

impl<T, E> Future for PollN<T, E>
where
  T: Unpin,
  E: Unpin,
{
  type Output = Result<T, E>;

  fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
    let this = self.get_mut();

    if this.pending_before_ready > 0 {
      this.pending_before_ready = this.pending_before_ready.saturating_sub(1);
      cx.waker().wake_by_ref();
      Poll::Pending
    } else {
      let Some(outcome) = this.result.take() else {
        return Poll::Pending;
      };
      Poll::Ready(outcome)
    }
  }
}

impl PollN<(), ()> {
  /// Returns a future that eventually resolves to `Ok(())`.
  #[must_use]
  pub const fn new_ok(finish_at: usize) -> Self {
    Self {
      result:               Some(Ok(())),
      pending_before_ready: finish_at.saturating_sub(1),
    }
  }

  /// Returns a future that eventually resolves to `Err(())`.
  #[must_use]
  pub const fn new_err(finish_at: usize) -> Self {
    Self {
      result:               Some(Err(())),
      pending_before_ready: finish_at.saturating_sub(1),
    }
  }
}

/// Blocks the current test thread until the provided future completes.
pub fn block_on_future<F>(future: F) -> F::Output
where
  F: Future,
{
  use tokio_test::task;

  let mut task = task::spawn(future);
  loop {
    if let Poll::Ready(output) = task.poll() {
      break output;
    }
  }
}

#[cfg(test)]
mod tests {
  use strict_test_support::TestFailure;
  use strict_test_support::ensure;

  use super::*;

  #[test]
  fn poll_n_eventually_returns_ok() -> Result<(), TestFailure> {
    ensure(
      block_on_future(PollN::new_ok(3)) == Ok(()),
      "PollN resolves to Ok after the configured number of polls",
    )
  }

  #[test]
  fn poll_n_eventually_returns_err() -> Result<(), TestFailure> {
    ensure(
      block_on_future(PollN::new_err(3)) == Err(()),
      "PollN resolves to Err after the configured number of polls",
    )
  }

  #[test]
  fn poll_n_finishes_on_first_poll() -> Result<(), TestFailure> {
    ensure(
      block_on_future(PollN::new_ok(1)) == Ok(()),
      "PollN can resolve to Ok on the first poll",
    )?;
    ensure(
      block_on_future(PollN::new_err(1)) == Err(()),
      "PollN can resolve to Err on the first poll",
    )
  }
}
