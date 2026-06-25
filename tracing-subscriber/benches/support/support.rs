//! Support types shared by the `tracing-subscriber` Criterion benchmarks.

use std::{
    sync::{Arc, Barrier},
    thread,
    time::{Duration, Instant},
};
use tracing::dispatcher::{Dispatch, with_default};

/// A synchronized four-worker benchmark driver.
#[derive(Clone, Debug)]
pub struct MultithreadedBench {
    /// The barrier that releases the main thread and workers together.
    start: Arc<Barrier>,
    /// The barrier that stops timing after all workers complete their body.
    end: Arc<Barrier>,
    /// The dispatch installed as the default inside each worker.
    dispatch: Dispatch,
}

impl MultithreadedBench {
    /// Creates a driver for four worker closures using the provided dispatch.
    #[must_use]
    pub fn new(dispatch: Dispatch) -> Self {
        Self {
            start: Arc::new(Barrier::new(5)),
            end: Arc::new(Barrier::new(5)),
            dispatch,
        }
    }

    /// Adds a worker closure that starts at the shared timing barrier.
    pub fn thread(&self, work: impl FnOnce() + Send + 'static) -> &Self {
        self.thread_with_setup(|start| {
            let _started = start.wait();
            work();
        })
    }

    /// Adds a worker closure that can prepare state before starting timing.
    pub fn thread_with_setup(&self, work: impl FnOnce(&Barrier) + Send + 'static) -> &Self {
        let this = self.clone();
        let _worker = thread::spawn(move || {
            let dispatch = this.dispatch.clone();
            with_default(&dispatch, move || {
                work(&this.start);
                let _finished = this.end.wait();
            });
        });
        self
    }

    /// Runs the registered workers and returns only the synchronized body time.
    #[must_use]
    pub fn run(&self) -> Duration {
        let _started = self.start.wait();
        let started_at = Instant::now();
        let _finished = self.end.wait();
        started_at.elapsed()
    }
}
