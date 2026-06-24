use crate::{Instrument, Instrumented, WithDispatch};
use futures_01::{
    Future,
    future::{ExecuteError, ExecuteErrorKind, Executor},
};

impl<T, F> Executor<F> for Instrumented<T>
where
    T: Executor<Instrumented<F>>,
    F: Future<Item = (), Error = ()>,
{
    fn execute(&self, future: F) -> Result<(), ExecuteError<F>> {
        let Some(inner) = self.inner.as_ref() else {
            return Err(ExecuteError::new(ExecuteErrorKind::Shutdown, future));
        };
        let future = future.instrument(self.span.clone());
        match inner.execute(future) {
            Ok(()) => Ok(()),
            Err(e) => {
                let kind = e.kind();
                match e.into_future().into_inner() {
                    Some(future) => Err(ExecuteError::new(kind, future)),
                    None => Ok(()),
                }
            }
        }
    }
}

impl<T, F> Executor<F> for WithDispatch<T>
where
    T: Executor<WithDispatch<F>>,
    F: Future<Item = (), Error = ()>,
{
    fn execute(&self, future: F) -> Result<(), ExecuteError<F>> {
        let future = self.with_dispatch(future);
        self.inner.execute(future).map_err(|e| {
            let kind = e.kind();
            let future = e.into_future().inner;
            ExecuteError::new(kind, future)
        })
    }
}

#[cfg(feature = "tokio-executor")]
pub use self::tokio_executor::*;

/// `tokio_executor::Executor`/`TypedExecutor` integration for the instrumented
/// wrappers. Enabled by the `tokio-executor` feature (and by `tokio`, which
/// implies it).
#[cfg(feature = "tokio-executor")]
mod tokio_executor {
    use crate::{Instrument, Instrumented, WithDispatch};
    use ::tokio_executor::{Executor, SpawnError, TypedExecutor};
    use futures_01::Future;

    impl<T> Executor for Instrumented<T>
    where
        T: Executor,
    {
        fn spawn(
            &mut self,
            future: Box<dyn Future<Error = (), Item = ()> + 'static + Send>,
        ) -> Result<(), SpawnError> {
            let Some(inner) = self.inner.as_mut() else {
                return Err(SpawnError::shutdown());
            };
            // TODO: get rid of double box somehow?
            let future = Box::new(future.instrument(self.span.clone()));
            inner.spawn(future)
        }
    }

    impl<T, F> TypedExecutor<F> for Instrumented<T>
    where
        T: TypedExecutor<Instrumented<F>>,
    {
        fn spawn(&mut self, future: F) -> Result<(), SpawnError> {
            let Some(inner) = self.inner.as_mut() else {
                return Err(SpawnError::shutdown());
            };
            inner.spawn(future.instrument(self.span.clone()))
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
        fn spawn(
            &mut self,
            future: Box<dyn Future<Error = (), Item = ()> + 'static + Send>,
        ) -> Result<(), SpawnError> {
            // TODO: get rid of double box?
            let future = Box::new(self.with_dispatch(future));
            self.inner.spawn(future)
        }
    }

    impl<T, F> TypedExecutor<F> for WithDispatch<T>
    where
        T: TypedExecutor<WithDispatch<F>>,
    {
        fn spawn(&mut self, future: F) -> Result<(), SpawnError> {
            self.inner.spawn(self.with_dispatch(future))
        }

        fn status(&self) -> Result<(), SpawnError> {
            self.inner.status()
        }
    }
}

#[cfg(feature = "tokio")]
pub use self::tokio_runtime::*;

/// tokio 0.1 runtime conveniences (`Runtime`/`current_thread::Runtime`).
/// Provided only by the full `tokio` feature.
#[cfg(feature = "tokio")]
mod tokio_runtime {
    use crate::{Instrument, Instrumented, WithDispatch};
    use futures_01::Future;
    use tokio_01::runtime::{Runtime, TaskExecutor, current_thread};

    impl Instrumented<Runtime> {
        /// Spawn an instrumented future onto the Tokio runtime.
        ///
        /// This spawns the given future onto the runtime's executor, usually a
        /// thread pool. The thread pool is then responsible for polling the
        /// future until it completes.
        ///
        /// This method simply wraps a call to `tokio::runtime::Runtime::spawn`,
        /// instrumenting the spawned future beforehand.
        pub fn spawn<F>(&mut self, future: F) -> Result<&mut Self, tokio_executor::SpawnError>
        where
            F: Future<Item = (), Error = ()> + Send + 'static,
        {
            let Some(inner) = self.inner.as_mut() else {
                return Err(tokio_executor::SpawnError::shutdown());
            };
            let future = future.instrument(self.span.clone());
            let _runtime = inner.spawn(future);
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
        pub fn block_on<F, R, E>(&mut self, future: F) -> Option<Result<R, E>>
        where
            F: Send + 'static + Future<Item = R, Error = E>,
            R: Send + 'static,
            E: Send + 'static,
        {
            let inner = self.inner.as_mut()?;
            let future = future.instrument(self.span.clone());
            Some(inner.block_on(future))
        }

        /// Return an instrumented handle to the runtime's executor.
        ///
        /// The returned handle can be used to spawn tasks that run on this runtime.
        ///
        /// The instrumented handle functions identically to a
        /// `tokio::runtime::TaskExecutor`, but instruments the spawned
        /// futures prior to spawning them.
        pub fn executor(&self) -> Option<Instrumented<TaskExecutor>> {
            Some(
                self.inner
                    .as_ref()?
                    .executor()
                    .instrument(self.span.clone()),
            )
        }
    }

    impl Instrumented<current_thread::Runtime> {
        /// Spawn an instrumented future onto the single-threaded Tokio runtime.
        ///
        /// This method simply wraps a call to `current_thread::Runtime::spawn`,
        /// instrumenting the spawned future beforehand.
        pub fn spawn<F>(&mut self, future: F) -> Result<&mut Self, tokio_executor::SpawnError>
        where
            F: Future<Item = (), Error = ()> + 'static,
        {
            let Some(inner) = self.inner.as_mut() else {
                return Err(tokio_executor::SpawnError::shutdown());
            };
            let future = future.instrument(self.span.clone());
            let _runtime = inner.spawn(future);
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
        pub fn block_on<F, R, E>(&mut self, future: F) -> Option<Result<R, E>>
        where
            F: 'static + Future<Item = R, Error = E>,
            R: 'static,
            E: 'static,
        {
            let inner = self.inner.as_mut()?;
            let future = future.instrument(self.span.clone());
            Some(inner.block_on(future))
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
        pub fn spawn<F>(&mut self, future: F) -> &mut Self
        where
            F: Future<Item = (), Error = ()> + Send + 'static,
        {
            let future = self.with_dispatch(future);
            let _runtime = self.inner.spawn(future);
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
        pub fn block_on<F, R, E>(&mut self, future: F) -> Result<R, E>
        where
            F: Send + 'static + Future<Item = R, Error = E>,
            R: Send + 'static,
            E: Send + 'static,
        {
            let future = self.with_dispatch(future);
            self.inner.block_on(future)
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
        pub fn spawn<F>(&mut self, future: F) -> &mut Self
        where
            F: Future<Item = (), Error = ()> + 'static,
        {
            let future = self.with_dispatch(future);
            let _runtime = self.inner.spawn(future);
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
        pub fn block_on<F, R, E>(&mut self, future: F) -> Result<R, E>
        where
            F: 'static + Future<Item = R, Error = E>,
            R: 'static,
            E: 'static,
        {
            let future = self.with_dispatch(future);
            self.inner.block_on(future)
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
