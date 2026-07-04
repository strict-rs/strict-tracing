use strict_test_support::TestFailure;
use strict_test_support::ensure_ok;
use tracing::subscriber::set_default;
use tracing_mock::layer::MockLayer;
use tracing_subscriber::Layer;
use tracing_subscriber::filter;
use tracing_subscriber::prelude::*;

use super::*;

fn layer() -> (MockLayer, subscriber::MockHandle) {
  layer::mock().only().run_with_handle()
}

fn filter<S>() -> filter::DynFilterFn<S> {
  // Use dynamic filter fn to disable interest caching and max-level hints,
  // allowing us to put all of these tests in the same file.
  filter::dynamic_filter_fn(|_, _| false)
}

/// reproduces <https://github.com/tokio-rs/tracing/issues/1563#issuecomment-921363629>
#[test]
fn box_works() -> Result<(), TestFailure> {
  let (mock_layer, handle) = layer();
  let filtered_layer = Box::new(mock_layer.with_filter(filter()));

  let _guard = set_default(tracing_subscriber::registry().with(filtered_layer));

  for i in 0..2 {
    tracing::info!(i);
  }

  ensure_ok(handle.finished(), "mock expectations should finish")?;
  Ok(())
}

/// the same as `box_works` but with a type-erased `Box`.
#[test]
fn dyn_box_works() -> Result<(), TestFailure> {
  let (mock_layer, handle) = layer();
  let filtered_layer: Box<dyn Layer<_> + Send + Sync + 'static> = Box::new(mock_layer.with_filter(filter()));

  let _guard = set_default(tracing_subscriber::registry().with(filtered_layer));

  for i in 0..2 {
    tracing::info!(i);
  }

  ensure_ok(handle.finished(), "mock expectations should finish")?;
  Ok(())
}
