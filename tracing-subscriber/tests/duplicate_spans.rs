//! Tests duplicate span filtering behavior.
#![cfg(all(feature = "env-filter", feature = "fmt"))]

#[cfg(test)]
mod tests {
  use strict_test_support::TestFailure;
  use strict_test_support::ensure;
  use tracing::Span;
  use tracing::subscriber::with_default;
  use tracing_subscriber::FmtSubscriber;
  use tracing_subscriber::filter::EnvFilter;

  #[test]
  fn duplicate_spans() -> Result<(), TestFailure> {
    let subscriber = FmtSubscriber::builder()
      .with_env_filter(EnvFilter::new("[root]=debug"))
      .finish();

    with_default(subscriber, || -> Result<(), TestFailure> {
      let root = tracing::debug_span!("root");
      let _root_guard = root.enter();
      // root:
      ensure(root == Span::current(), "current span is root")?;
      let leaf = tracing::debug_span!("leaf");
      let leaf_guard = leaf.enter();
      // root:leaf:
      ensure(leaf == Span::current(), "current span is leaf")?;
      let first_nested_root_guard = root.enter();
      // root:leaf:
      ensure(leaf == Span::current(), "current span remains leaf after entering root twice")?;
      drop(first_nested_root_guard);
      drop(leaf_guard);
      // root:
      ensure(
        root == Span::current(),
        "current span returns to root after exiting leaf and nested root",
      )?;

      let second_nested_root_guard = root.enter();
      ensure(root == Span::current(), "current span is root")?;
      drop(second_nested_root_guard);
      // root:
      ensure(root == Span::current(), "current span remains root after exiting nested root")
    })
  }
}
