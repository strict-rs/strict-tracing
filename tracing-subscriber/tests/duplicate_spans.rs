//! Tests duplicate span filtering behavior.
#![cfg(all(feature = "env-filter", feature = "fmt"))]

#[cfg(test)]
mod tests {

  use strict_test_support::ConditionFailure;
  use strict_test_support::ensure;
  use tracing::Span;
  use tracing::subscriber::with_default;
  use tracing_subscriber::FmtSubscriber;
  use tracing_subscriber::filter::EnvFilter;

  #[test]
  fn duplicate_spans() -> Result<(), ConditionFailure> {
    let subscriber = FmtSubscriber::builder()
      .with_env_filter(EnvFilter::new("[root]=debug"))
      .finish();

    with_default(subscriber, || -> Result<(), ConditionFailure> {
      let root = tracing::debug_span!("root");
      let _root_guard = root.enter();
      // root:
      ensure(root == Span::current(), "current span is root").map(drop)?;
      let leaf = tracing::debug_span!("leaf");
      let leaf_guard = leaf.enter();
      // root:leaf:
      ensure(leaf == Span::current(), "current span is leaf").map(drop)?;
      let first_nested_root_guard = root.enter();
      // root:leaf:
      ensure(leaf == Span::current(), "current span remains leaf after entering root twice").map(drop)?;
      drop(first_nested_root_guard);
      drop(leaf_guard);
      // root:
      ensure(
        root == Span::current(),
        "current span returns to root after exiting leaf and nested root",
      )
      .map(drop)?;

      let second_nested_root_guard = root.enter();
      ensure(root == Span::current(), "current span is root").map(drop)?;
      drop(second_nested_root_guard);
      // root:
      ensure(root == Span::current(), "current span remains root after exiting nested root").map(drop)
    })
  }
}
