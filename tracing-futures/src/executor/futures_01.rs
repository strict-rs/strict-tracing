use futures_01::Future;
use futures_01::future::ExecuteError;
use futures_01::future::ExecuteErrorKind;
use futures_01::future::Executor;

use crate::Instrument as _;
use crate::Instrumented;
use crate::WithDispatch;

impl<T, F> Executor<F> for Instrumented<T>
where
  T: Executor<Instrumented<F>>,
  F: Future<Item = (), Error = ()>,
{
  fn execute(&self, job: F) -> Result<(), ExecuteError<F>> {
    let Some(inner) = self.inner.as_ref() else {
      return Err(ExecuteError::new(ExecuteErrorKind::Shutdown, job));
    };
    let instrumented_job = job.instrument(self.span.clone());
    match inner.execute(instrumented_job) {
      Ok(()) => Ok(()),
      Err(error) => {
        let kind = error.kind();
        error
          .into_future()
          .into_inner()
          .map_or_else(|| Ok(()), |original| Err(ExecuteError::new(kind, original)))
      }
    }
  }
}

impl<T, F> Executor<F> for WithDispatch<T>
where
  T: Executor<WithDispatch<F>>,
  F: Future<Item = (), Error = ()>,
{
  fn execute(&self, job: F) -> Result<(), ExecuteError<F>> {
    let dispatched_job = self.with_dispatch(job);
    self.inner.execute(dispatched_job).map_err(|error| {
      let kind = error.kind();
      let original = error.into_future().inner;
      ExecuteError::new(kind, original)
    })
  }
}

/// `tokio_executor::Executor`/`TypedExecutor` integration for the instrumented
/// wrappers. Enabled by the `tokio-executor` feature (and by `tokio`, which
/// implies it).
#[cfg(feature = "tokio-executor")]
mod tokio_executor {
  use ::tokio_executor::Executor;
  use ::tokio_executor::SpawnError;
  use ::tokio_executor::TypedExecutor;
  use futures_01::Future;

  use crate::Instrument as _;
  use crate::Instrumented;
  use crate::WithDispatch;

  impl<T> Executor for Instrumented<T>
  where
    T: Executor,
  {
    fn spawn(&mut self, job: Box<dyn Future<Error = (), Item = ()> + 'static + Send>) -> Result<(), SpawnError> {
      let Some(inner) = self.inner.as_mut() else {
        return Err(SpawnError::shutdown());
      };
      // TODO: get rid of double box somehow?
      let instrumented_job = Box::new(job.instrument(self.span.clone()));
      inner.spawn(instrumented_job)
    }
  }

  impl<T, F> TypedExecutor<F> for Instrumented<T>
  where
    T: TypedExecutor<Instrumented<F>>,
  {
    fn spawn(&mut self, job: F) -> Result<(), SpawnError> {
      let Some(inner) = self.inner.as_mut() else {
        return Err(SpawnError::shutdown());
      };
      inner.spawn(job.instrument(self.span.clone()))
    }

    fn status(&self) -> Result<(), SpawnError> {
      let Some(inner) = self.inner.as_ref() else {
        return Err(SpawnError::shutdown());
      };
      inner.status()
    }
  }

  impl<T> Executor for WithDispatch<T>
  where
    T: Executor,
  {
    fn spawn(&mut self, job: Box<dyn Future<Error = (), Item = ()> + 'static + Send>) -> Result<(), SpawnError> {
      // TODO: get rid of double box?
      let dispatched_job = Box::new(self.with_dispatch(job));
      self.inner.spawn(dispatched_job)
    }
  }

  impl<T, F> TypedExecutor<F> for WithDispatch<T>
  where
    T: TypedExecutor<WithDispatch<F>>,
  {
    fn spawn(&mut self, job: F) -> Result<(), SpawnError> {
      self.inner.spawn(self.with_dispatch(job))
    }

    fn status(&self) -> Result<(), SpawnError> {
      self.inner.status()
    }
  }
}

/// tokio 0.1 runtime conveniences (`Runtime`/`current_thread::Runtime`).
/// Provided only by the full `tokio` feature.
#[cfg(feature = "tokio")]
mod tokio_runtime {
  use futures_01::Future;
  use tokio_01::runtime::Runtime;
  use tokio_01::runtime::TaskExecutor;
  use tokio_01::runtime::current_thread;

  use crate::Instrument as _;
  use crate::Instrumented;
  use crate::WithDispatch;

  impl Instrumented<Runtime> {
    /// Spawn an instrumented future onto the Tokio runtime.
    ///
    /// This spawns the given future onto the runtime's executor, usually a
    /// thread pool. The thread pool is then responsible for polling the
    /// future until it completes.
    ///
    /// This method simply wraps a call to `tokio::runtime::Runtime::spawn`,
    /// instrumenting the spawned future beforehand.
    ///
    /// # Errors
    ///
    /// Returns an error if the wrapped runtime has already been consumed.
    pub fn spawn_instrumented<F>(&mut self, job: F) -> Result<&mut Self, tokio_executor::SpawnError>
    where
      F: Future<Item = (), Error = ()> + Send + 'static,
    {
      let Some(inner) = self.inner.as_mut() else {
        return Err(tokio_executor::SpawnError::shutdown());
      };
      let instrumented_job = job.instrument(self.span.clone());
      let _runtime = inner.spawn(instrumented_job);
      Ok(self)
    }

    /// Run an instrumented future to completion on the Tokio runtime.
    ///
    /// This runs the given future on the runtime, blocking until it is
    /// complete, and yielding its resolved result. Any tasks or timers which
    /// the future spawns internally will be executed on the runtime.
    ///
    /// This method should not be called from an asynchronous context.
    ///
    /// This method simply wraps a call to `tokio::runtime::Runtime::block_on`,
    /// instrumenting the spawned future beforehand.
    ///
    /// # Panics
    ///
    /// This function panics if the executor is at capacity, if the provided
    /// future panics, or if called within an asynchronous execution context.
    pub fn block_on<F, R, E>(&mut self, job: F) -> Option<Result<R, E>>
    where
      F: Send + 'static + Future<Item = R, Error = E>,
      R: Send + 'static,
      E: Send + 'static,
    {
      let inner = self.inner.as_mut()?;
      let instrumented_job = job.instrument(self.span.clone());
      Some(inner.block_on(instrumented_job))
    }

    /// Return an instrumented handle to the runtime's executor.
    ///
    /// The returned handle can be used to spawn tasks that run on this runtime.
    ///
    /// The instrumented handle functions identically to a
    /// `tokio::runtime::TaskExecutor`, but instruments the spawned
    /// futures prior to spawning them.
    pub fn executor(&self) -> Option<Instrumented<TaskExecutor>> {
      Some(self.inner.as_ref()?.executor().instrument(self.span.clone()))
    }
  }

  impl Instrumented<current_thread::Runtime> {
    /// Spawn an instrumented future onto the single-threaded Tokio runtime.
    ///
    /// This method simply wraps a call to `current_thread::Runtime::spawn`,
    /// instrumenting the spawned future beforehand.
    ///
    /// # Errors
    ///
    /// Returns an error if the wrapped runtime has already been consumed.
    pub fn spawn_instrumented<F>(&mut self, job: F) -> Result<&mut Self, tokio_executor::SpawnError>
    where
      F: Future<Item = (), Error = ()> + 'static,
    {
      let Some(inner) = self.inner.as_mut() else {
        return Err(tokio_executor::SpawnError::shutdown());
      };
      let instrumented_job = job.instrument(self.span.clone());
      let _runtime = inner.spawn(instrumented_job);
      Ok(self)
    }

    /// Instruments and runs the provided future, blocking the current thread
    /// until the future completes.
    ///
    /// This function can be used to synchronously block the current thread
    /// until the provided `future` has resolved either successfully or with an
    /// error. The result of the future is then returned from this function
    /// call.
    ///
    /// Note that this function will **also** execute any spawned futures on the
    /// current thread, but will **not** block until these other spawned futures
    /// have completed. Once the function returns, any uncompleted futures
    /// remain pending in the `Runtime` instance. These futures will not run
    /// until `block_on` or `run` is called again.
    ///
    /// The caller is responsible for ensuring that other spawned futures
    /// complete execution by calling `block_on` or `run`.
    ///
    /// This method simply wraps a call to `current_thread::Runtime::block_on`,
    /// instrumenting the spawned future beforehand.
    ///
    /// # Panics
    ///
    /// This function panics if the executor is at capacity, if the provided
    /// future panics, or if called within an asynchronous execution context.
    pub fn block_on<F, R, E>(&mut self, job: F) -> Option<Result<R, E>>
    where
      F: 'static + Future<Item = R, Error = E>,
      R: 'static,
      E: 'static,
    {
      let inner = self.inner.as_mut()?;
      let instrumented_job = job.instrument(self.span.clone());
      Some(inner.block_on(instrumented_job))
    }

    /// Get a new instrumented handle to spawn futures on the single-threaded
    /// Tokio runtime
    ///
    /// Different to the runtime itself, the handle can be sent to different
    /// threads.
    ///
    /// The instrumented handle functions identically to a
    /// `tokio::runtime::current_thread::Handle`, but instruments the spawned
    /// futures prior to spawning them.
    pub fn handle(&self) -> Option<Instrumented<current_thread::Handle>> {
      Some(self.inner.as_ref()?.handle().instrument(self.span.clone()))
    }
  }

  impl WithDispatch<Runtime> {
    /// Spawn a future onto the Tokio runtime, in the context of this
    /// `WithDispatch`'s trace dispatcher.
    ///
    /// This spawns the given future onto the runtime's executor, usually a
    /// thread pool. The thread pool is then responsible for polling the
    /// future until it completes.
    ///
    /// This method simply wraps a call to `tokio::runtime::Runtime::spawn`,
    /// instrumenting the spawned future beforehand.
    pub fn spawn_with_dispatch<F>(&mut self, job: F) -> &mut Self
    where
      F: Future<Item = (), Error = ()> + Send + 'static,
    {
      let dispatched_job = self.with_dispatch(job);
      let _runtime = self.inner.spawn(dispatched_job);
      self
    }

    /// Run a future to completion on the Tokio runtime, in the context of this
    /// `WithDispatch`'s trace dispatcher.
    ///
    /// This runs the given future on the runtime, blocking until it is
    /// complete, and yielding its resolved result. Any tasks or timers which
    /// the future spawns internally will be executed on the runtime.
    ///
    /// This method should not be called from an asynchronous context.
    ///
    /// This method simply wraps a call to `tokio::runtime::Runtime::block_on`,
    /// instrumenting the spawned future beforehand.
    ///
    /// # Panics
    ///
    /// This function panics if the executor is at capacity, if the provided
    /// future panics, or if called within an asynchronous execution context.
    ///
    /// # Errors
    ///
    /// Returns the future's error if it completes unsuccessfully.
    pub fn block_on<F, R, E>(&mut self, job: F) -> Result<R, E>
    where
      F: Send + 'static + Future<Item = R, Error = E>,
      R: Send + 'static,
      E: Send + 'static,
    {
      let dispatched_job = self.with_dispatch(job);
      self.inner.block_on(dispatched_job)
    }

    /// Return a handle to the runtime's executor, in the context of this
    /// `WithDispatch`'s trace dispatcher.
    ///
    /// The returned handle can be used to spawn tasks that run on this runtime.
    ///
    /// The instrumented handle functions identically to a
    /// `tokio::runtime::TaskExecutor`, but instruments the spawned
    /// futures prior to spawning them.
    pub fn executor(&self) -> WithDispatch<TaskExecutor> {
      self.with_dispatch(self.inner.executor())
    }
  }

  impl WithDispatch<current_thread::Runtime> {
    /// Spawn a future onto the single-threaded Tokio runtime, in the context
    /// of this `WithDispatch`'s trace dispatcher.
    ///
    /// This method simply wraps a call to `current_thread::Runtime::spawn`,
    /// instrumenting the spawned future beforehand.
    pub fn spawn_with_dispatch<F>(&mut self, job: F) -> &mut Self
    where
      F: Future<Item = (), Error = ()> + 'static,
    {
      let dispatched_job = self.with_dispatch(job);
      let _runtime = self.inner.spawn(dispatched_job);
      self
    }

    /// Runs the provided future in the context of this `WithDispatch`'s trace
    /// dispatcher, blocking the current thread until the future completes.
    ///
    /// This function can be used to synchronously block the current thread
    /// until the provided `future` has resolved either successfully or with an
    /// error. The result of the future is then returned from this function
    /// call.
    ///
    /// Note that this function will **also** execute any spawned futures on the
    /// current thread, but will **not** block until these other spawned futures
    /// have completed. Once the function returns, any uncompleted futures
    /// remain pending in the `Runtime` instance. These futures will not run
    /// until `block_on` or `run` is called again.
    ///
    /// The caller is responsible for ensuring that other spawned futures
    /// complete execution by calling `block_on` or `run`.
    ///
    /// This method simply wraps a call to `current_thread::Runtime::block_on`,
    /// instrumenting the spawned future beforehand.
    ///
    /// # Panics
    ///
    /// This function panics if the executor is at capacity, if the provided
    /// future panics, or if called within an asynchronous execution context.
    ///
    /// # Errors
    ///
    /// Returns the future's error if it completes unsuccessfully.
    pub fn block_on<F, R, E>(&mut self, job: F) -> Result<R, E>
    where
      F: 'static + Future<Item = R, Error = E>,
      R: 'static,
      E: 'static,
    {
      let dispatched_job = self.with_dispatch(job);
      self.inner.block_on(dispatched_job)
    }

    /// Get a new handle to spawn futures on the single-threaded Tokio runtime,
    /// in the context of this `WithDispatch`'s trace dispatcher.\
    ///
    /// Different to the runtime itself, the handle can be sent to different
    /// threads.
    ///
    /// The instrumented handle functions identically to a
    /// `tokio::runtime::current_thread::Handle`, but the spawned
    /// futures are run in the context of the trace dispatcher.
    pub fn handle(&self) -> WithDispatch<current_thread::Handle> {
      self.with_dispatch(self.inner.handle())
    }
  }
}
