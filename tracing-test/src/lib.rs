//! Test utilities shared by the tracing workspace.

use std::future::Future;
use std::{
    pin::Pin,
    task::{Context, Poll},
};

/// A future that remains pending for a fixed number of polls before completing.
#[derive(Debug)]
pub struct PollN<T, E> {
    and_return: Option<Result<T, E>>,
    finish_at: usize,
    polls: usize,
}

impl<T, E> Future for PollN<T, E>
where
    T: Unpin,
    E: Unpin,
{
    type Output = Result<T, E>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();

        this.polls += 1;
        if this.polls == this.finish_at {
            let value = this.and_return.take().expect("polled after ready");

            Poll::Ready(value)
        } else {
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

impl PollN<(), ()> {
    /// Returns a future that eventually resolves to `Ok(())`.
    pub fn new_ok(finish_at: usize) -> Self {
        Self {
            and_return: Some(Ok(())),
            finish_at,
            polls: 0,
        }
    }

    /// Returns a future that eventually resolves to `Err(())`.
    pub fn new_err(finish_at: usize) -> Self {
        Self {
            and_return: Some(Err(())),
            finish_at,
            polls: 0,
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
        if let Poll::Ready(v) = task.poll() {
            break v;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poll_n_eventually_returns_ok() {
        assert_eq!(block_on_future(PollN::new_ok(3)), Ok(()));
    }

    #[test]
    fn poll_n_eventually_returns_err() {
        assert_eq!(block_on_future(PollN::new_err(3)), Err(()));
    }
}
