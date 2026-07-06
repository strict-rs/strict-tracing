//! Tests for `std::future::Future` instrumentation.

#[cfg(test)]
mod tests {
  use std::future::Future;
  use std::pin::Pin;
  #[cfg(feature = "futures-03")]
  use std::sync::Arc;
  #[cfg(feature = "futures-03")]
  use std::sync::atomic::AtomicBool;
  #[cfg(feature = "futures-03")]
  use std::sync::atomic::Ordering;
  use std::task;

  use futures::FutureExt as _;
  #[cfg(feature = "futures-03")]
  use futures::executor::block_on;
  #[cfg(feature = "futures-03")]
  use futures::task::FutureObj;
  #[cfg(feature = "futures-03")]
  use futures::task::LocalFutureObj;
  #[cfg(feature = "futures-03")]
  use futures::task::LocalSpawn;
  #[cfg(feature = "futures-03")]
  use futures::task::Spawn;
  #[cfg(feature = "futures-03")]
  use futures::task::SpawnError;
  use strict_test_support::TestFailure;
  use strict_test_support::ensure;
  use strict_test_support::ensure_contains;
  use strict_test_support::ensure_eq;
  use strict_test_support::ensure_ok;
  use strict_test_support::ensure_some;
  use tracing::Level;
  use tracing::subscriber::with_default;
  use tracing_futures::Instrument as _;
  use tracing_futures::WithSubscriber as _;
  use tracing_mock::expect;
  use tracing_mock::subscriber;
  use tracing_test::PollN;
  use tracing_test::block_on_future;

  #[cfg(feature = "futures-03")]
  #[derive(Clone, Debug, Default)]
  struct RecordingSpawn {
    shutdown: Arc<AtomicBool>,
  }

  #[derive(Clone, Debug)]
  struct AccessorFuture {
    value: u64,
  }

  impl Future for AccessorFuture {
    type Output = u64;

    fn poll(self: Pin<&mut Self>, _: &mut task::Context<'_>) -> task::Poll<Self::Output> {
      task::Poll::Ready(self.value)
    }
  }

  #[cfg(feature = "futures-03")]
  impl RecordingSpawn {
    fn with_shutdown(shutdown: bool) -> Self {
      Self {
        shutdown: Arc::new(AtomicBool::new(shutdown)),
      }
    }

    fn ensure_running(&self) -> Result<(), SpawnError> {
      if self.shutdown.load(Ordering::Acquire) {
        Err(SpawnError::shutdown())
      } else {
        Ok(())
      }
    }
  }

  #[cfg(feature = "futures-03")]
  impl Spawn for RecordingSpawn {
    fn spawn_obj(&self, task: FutureObj<'static, ()>) -> Result<(), SpawnError> {
      self.ensure_running()?;
      block_on(task);
      Ok(())
    }

    fn status(&self) -> Result<(), SpawnError> {
      self.ensure_running()
    }
  }

  #[cfg(feature = "futures-03")]
  impl LocalSpawn for RecordingSpawn {
    fn spawn_local_obj(&self, task: LocalFutureObj<'static, ()>) -> Result<(), SpawnError> {
      self.ensure_running()?;
      block_on(task);
      Ok(())
    }

    fn status_local(&self) -> Result<(), SpawnError> {
      self.ensure_running()
    }
  }

  #[cfg(feature = "futures-03")]
  fn future_task() -> FutureObj<'static, ()> {
    FutureObj::new(Box::new(async {
      tracing::info!("spawned task");
    }))
  }

  #[cfg(feature = "futures-03")]
  fn local_task() -> LocalFutureObj<'static, ()> {
    LocalFutureObj::new(Box::new(async {
      tracing::info!("spawned task");
    }))
  }

  fn polled_future_enters_and_exits_span(future: PollN<(), ()>, expected_ready: bool) -> Result<(), TestFailure> {
    let (subscriber, handle) = subscriber::mock()
      .enter(expect::span().named("foo"))
      .exit(expect::span().named("foo"))
      .enter(expect::span().named("foo"))
      .exit(expect::span().named("foo"))
      .enter(expect::span().named("foo"))
      .exit(expect::span().named("foo"))
      .close_span(expect::span().named("foo"))
      .only()
      .run_with_handle();
    with_default(subscriber, || {
      let instrumented = future.instrument(tracing::span!(Level::TRACE, "foo"));
      ensure(
        block_on_future(instrumented).is_ok() == expected_ready,
        "instrumented std future returns the expected polarity",
      )
    })?;
    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[cfg(feature = "futures-03")]
  fn instrumented_spawn_runs_task_under_span(
    spawn_task: impl FnOnce(&tracing_futures::Instrumented<RecordingSpawn>) -> Result<(), SpawnError>,
  ) -> Result<(), TestFailure> {
    let (subscriber, handle) = subscriber::mock()
      .new_span(expect::span().named("spawned"))
      .enter(expect::span().named("spawned"))
      .event(
        expect::event()
          .with_ancestry(expect::has_contextual_parent("spawned"))
          .at_level(Level::INFO),
      )
      .exit(expect::span().named("spawned"))
      .enter(expect::span().named("spawned"))
      .exit(expect::span().named("spawned"))
      .enter(expect::span().named("spawned"))
      .exit(expect::span().named("spawned"))
      .close_span(expect::span().named("spawned"))
      .only()
      .run_with_handle();
    with_default(subscriber, || {
      let executor = RecordingSpawn::default().instrument(tracing::info_span!("spawned"));
      ensure_ok(spawn_task(&executor), "instrumented spawn should succeed")?;
      Ok::<(), TestFailure>(())
    })?;
    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[test]
  fn enter_exit_is_reasonable() -> Result<(), TestFailure> {
    polled_future_enters_and_exits_span(PollN::new_ok(2), true)
  }

  #[test]
  fn error_ends_span() -> Result<(), TestFailure> {
    polled_future_enters_and_exits_span(PollN::new_err(2), false)
  }

  #[test]
  #[cfg(feature = "futures-03")]
  fn instrumented_spawn_obj_runs_task_under_span() -> Result<(), TestFailure> {
    instrumented_spawn_runs_task_under_span(|executor| executor.spawn_obj(future_task()))
  }

  #[test]
  #[cfg(feature = "futures-03")]
  fn instrumented_spawn_local_obj_runs_task_under_span() -> Result<(), TestFailure> {
    instrumented_spawn_runs_task_under_span(|executor| executor.spawn_local_obj(local_task()))
  }

  #[test]
  #[cfg(feature = "futures-03")]
  fn instrumented_spawn_status_reflects_inner_executor() -> Result<(), TestFailure> {
    let span = tracing::info_span!("spawned");
    let running = RecordingSpawn::default().instrument(span.clone());
    ensure_ok(running.status(), "running executor reports spawn capacity")?;

    let stopped = RecordingSpawn::with_shutdown(true).instrument(span);
    ensure(stopped.status().is_err(), "shutdown executor status is propagated")
  }

  #[test]
  #[cfg(feature = "futures-03")]
  fn instrumented_local_spawn_status_reflects_inner_executor() -> Result<(), TestFailure> {
    let span = tracing::info_span!("spawned");
    let running = RecordingSpawn::default().instrument(span.clone());
    ensure_ok(running.status_local(), "running local executor reports spawn capacity")?;

    let stopped = RecordingSpawn::with_shutdown(true).instrument(span);
    ensure(stopped.status_local().is_err(), "shutdown local executor status is propagated")
  }

  #[test]
  fn instrumented_accessors_expose_span_and_inner_future() -> Result<(), TestFailure> {
    let mut instrumented = AccessorFuture {
      value: 5
    }
    .instrument(tracing::info_span!("accessor"));

    ensure(
      instrumented.span().metadata().is_some(),
      "instrumented futures expose their span metadata",
    )?;
    *instrumented.span_mut() = tracing::info_span!("replacement_accessor");
    ensure(
      instrumented
        .span()
        .metadata()
        .is_some_and(|metadata| metadata.name() == "replacement_accessor"),
      "instrumented futures expose mutable span access",
    )?;
    ensure_eq(
      &ensure_some(instrumented.inner(), "instrumented future exposes inner refs")?.value,
      &5,
      "instrumented inner refs expose the wrapped future",
    )?;
    ensure_some(instrumented.inner_mut(), "instrumented future exposes inner mutable refs")?.value = 6;
    ensure_eq(
      &ensure_some(Pin::new(&instrumented).inner_pin_ref(), "instrumented future exposes pinned refs")?.value,
      &6,
      "instrumented pinned refs expose the wrapped future",
    )?;
    ensure_some(
      Pin::new(&mut instrumented).inner_pin_mut(),
      "instrumented future exposes pinned mutable refs",
    )?
    .value = 7;

    let inner = ensure_some(instrumented.into_inner(), "instrumented future returns its inner future")?;
    ensure_eq(&inner.value, &7, "instrumented into_inner returns the mutated future")
  }

  #[test]
  fn with_dispatch_accessors_expose_dispatch_and_inner_future() -> Result<(), TestFailure> {
    let subscriber = subscriber::mock().only().run();
    let mut wrapped = AccessorFuture {
      value: 11
    }
    .with_subscriber(subscriber);

    ensure_contains(
      &format!("{:?}", wrapped.dispatch()),
      "Dispatch::Scoped",
      "with-dispatch exposes the captured dispatch",
    )?;
    ensure_eq(&wrapped.inner().value, &11, "with-dispatch exposes inner refs")?;
    wrapped.inner_mut().value = 12;
    ensure_eq(&Pin::new(&wrapped).inner_pin_ref().value, &12, "with-dispatch exposes pinned refs")?;
    Pin::new(&mut wrapped).inner_pin_mut().value = 13;

    let sibling = wrapped.with_dispatch(AccessorFuture {
      value: 21
    });
    ensure_eq(
      &sibling.inner().value,
      &21,
      "with_dispatch wraps a new inner future with the same dispatch",
    )?;
    ensure_contains(
      &format!("{:?}", sibling.dispatch()),
      "Dispatch::Scoped",
      "with_dispatch preserves the captured dispatch",
    )?;

    let inner = wrapped.into_inner();
    ensure_eq(&inner.value, &13, "with-dispatch into_inner returns the mutated future")
  }

  #[test]
  #[cfg(feature = "futures-03")]
  fn with_dispatch_spawn_obj_runs_task_on_captured_dispatch() -> Result<(), TestFailure> {
    let (captured, handle) = subscriber::mock()
      .event(expect::event().at_level(Level::INFO))
      .only()
      .run_with_handle();
    let ambient = subscriber::mock().only().run();
    let executor = RecordingSpawn::default().with_subscriber(captured);

    with_default(ambient, || {
      ensure_ok(executor.spawn_obj(future_task()), "captured dispatch spawn should succeed")
    })?;
    ensure_ok(handle.finished(), "captured dispatch expectations should finish")?;
    Ok(())
  }

  #[test]
  #[cfg(feature = "futures-03")]
  fn with_dispatch_spawn_local_obj_runs_task_on_captured_dispatch() -> Result<(), TestFailure> {
    let (captured, handle) = subscriber::mock()
      .event(expect::event().at_level(Level::INFO))
      .only()
      .run_with_handle();
    let ambient = subscriber::mock().only().run();
    let executor = RecordingSpawn::default().with_subscriber(captured);

    with_default(ambient, || {
      ensure_ok(
        executor.spawn_local_obj(local_task()),
        "captured dispatch local spawn should succeed",
      )
    })?;
    ensure_ok(handle.finished(), "captured dispatch expectations should finish")?;
    Ok(())
  }

  #[test]
  fn span_on_drop() -> Result<(), TestFailure> {
    #[derive(Clone, Debug)]
    struct AssertSpanOnDrop;

    impl Drop for AssertSpanOnDrop {
      fn drop(&mut self) {
        tracing::info!("Drop");
      }
    }

    struct Fut {
      span_on_drop: Option<AssertSpanOnDrop>,
    }

    impl Future for Fut {
      type Output = ();

      fn poll(mut self: Pin<&mut Self>, _: &mut task::Context<'_>) -> task::Poll<Self::Output> {
        drop(self.span_on_drop.take());
        task::Poll::Ready(())
      }
    }

    let subscriber = subscriber::mock()
      .enter(expect::span().named("foo"))
      .event(
        expect::event()
          .with_ancestry(expect::has_contextual_parent("foo"))
          .at_level(Level::INFO),
      )
      .exit(expect::span().named("foo"))
      .enter(expect::span().named("foo"))
      .exit(expect::span().named("foo"))
      .close_span(expect::span().named("foo"))
      .enter(expect::span().named("bar"))
      .event(
        expect::event()
          .with_ancestry(expect::has_contextual_parent("bar"))
          .at_level(Level::INFO),
      )
      .exit(expect::span().named("bar"))
      .close_span(expect::span().named("bar"))
      .only()
      .run();

    with_default(subscriber, || {
      // polled once
      ensure_some(
        Fut {
          span_on_drop: Some(AssertSpanOnDrop),
        }
        .instrument(tracing::span!(Level::TRACE, "foo"))
        .now_or_never(),
        "instrumented std future resolves synchronously",
      )?;

      // never polled
      drop(
        Fut {
          span_on_drop: Some(AssertSpanOnDrop),
        }
        .instrument(tracing::span!(Level::TRACE, "bar")),
      );
      Ok(())
    })
  }
}
