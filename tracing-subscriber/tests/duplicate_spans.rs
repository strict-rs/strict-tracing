//! Tests duplicate span filtering behavior.
#![cfg(all(feature = "env-filter", feature = "fmt"))]

#[cfg(test)]
mod tests {
  use strict_test_support::TestFailure;
  use strict_test_support::ensure;
  use tracing::Span;
  use tracing::subscriber::with_default;
  use tracing::{
    self,
  };
  use tracing_subscriber::FmtSubscriber;
  use tracing_subscriber::filter::EnvFilter;

  #[test]
  fn duplicate_spans() -> Result<(), TestFailure> {
    let subscriber = FmtSubscriber::builder()
      .with_env_filter(EnvFilter::new("[root]=debug"))
      .finish();

    with_default(subscriber, || -> Result<(), TestFailure> {
      let root = tracing::debug_span!("root");
      root.in_scope(|| -> Result<(), TestFailure> {
        // root:
        ensure(root == Span::current(), "current span is root")?;
        let leaf = tracing::debug_span!("leaf");
        leaf.in_scope(|| -> Result<(), TestFailure> {
          // root:leaf:
          ensure(leaf == Span::current(), "current span is leaf")?;
          root.in_scope(|| -> Result<(), TestFailure> {
            // root:leaf:
            ensure(leaf == Span::current(), "current span remains leaf after entering root twice")
          })
        })?;
        // root:
        ensure(
          root == Span::current(),
          "current span returns to root after exiting leaf and nested root",
        )?;

        root.in_scope(|| ensure(root == Span::current(), "current span is root"))?;
        // root:
        ensure(root == Span::current(), "current span remains root after exiting nested root")
      })
    })
  }
}
