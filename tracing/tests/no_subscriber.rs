#![cfg(feature = "std")]
//! No-subscriber behavior coverage.

#[cfg(test)]
mod tests {
  use strict_test_support::TestFailure;
  use strict_test_support::ensure_ok;
  use tracing::subscriber::NoSubscriber;
  use tracing::subscriber::set_global_default;
  use tracing::subscriber::with_default;
  use tracing_mock::subscriber;

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn no_subscriber_disables_global() -> Result<(), TestFailure> {
    // Reproduces https://github.com/tokio-rs/tracing/issues/1999
    let (subscriber, handle) = subscriber::mock().only().run_with_handle();
    ensure_ok(set_global_default(subscriber), "setting global default must succeed")?;
    with_default(NoSubscriber::default(), || {
      tracing::info!("this should not be recorded");
    });
    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
  }
}
